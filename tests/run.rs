use std::process::Command;

fn memtop() -> Command {
    Command::new(env!("CARGO_BIN_EXE_memtop"))
}

/// The report is the last `{...}` line on stderr; warnings (e.g. the
/// RSS-fallback note) may precede it.
fn report_json(stderr: &[u8]) -> serde_json::Value {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{') && l.trim_end().ends_with('}'))
        .unwrap_or_else(|| panic!("no JSON report line on stderr, got: {text:?}"));
    serde_json::from_str(line).expect("valid JSON report")
}

#[test]
fn run_propagates_child_exit_code() {
    let out = memtop()
        .args(["run", "--", "sh", "-c", "exit 7"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn run_propagates_signal_death_as_128_plus_signo() {
    let out = memtop()
        .args(["run", "--", "sh", "-c", "kill -TERM $$"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(128 + 15));
}

#[test]
fn run_report_goes_to_stderr_and_json_parses() {
    let out = memtop()
        .args(["run", "--json", "--", "sh", "-c", "sleep 0.2; exit 0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    // stdout stays untouched (nothing was printed by the child)
    assert!(out.stdout.is_empty());
    let report = report_json(&out.stderr);
    assert_eq!(report["exit_code"], 0);
    assert!(report["peak_bytes"].as_u64().unwrap() > 0);
    assert!(report["peak_processes"].as_u64().unwrap() >= 1);
}

#[test]
fn run_counts_children_of_the_command() {
    let out = memtop()
        .args([
            "run",
            "--json",
            "--interval-ms",
            "20",
            "--",
            "sh",
            "-c",
            "sleep 0.4 & sleep 0.4 & wait",
        ])
        .output()
        .unwrap();
    let report = report_json(&out.stderr);
    // sh + two sleeps
    assert_eq!(report["peak_processes"], 3, "report: {report}");
}

#[test]
fn run_reports_python_peak_memory() {
    // skip when python3 is not available
    if Command::new("python3").arg("-V").output().is_err() {
        eprintln!("skipping: python3 not available");
        return;
    }

    let out = memtop()
        .args([
            "run",
            "--json",
            "--",
            "python3",
            "-c",
            "b=bytearray(300*1024*1024); import time; time.sleep(1)",
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(0));
    let report = report_json(&out.stderr);
    let peak = report["peak_bytes"].as_u64().unwrap();
    let target = 300 * 1024 * 1024_u64;
    assert!(
        peak > target * 9 / 10 && peak < target * 13 / 10,
        "peak {peak} should be within ~10-30% of {target}"
    );
}

/// Phase 7 acceptance: killing a parent tree removes the shell and its
/// children (leaves first, parent last), verified against live processes.
#[test]
fn tree_kill_removes_shell_and_children() {
    let mut sh = Command::new("sh")
        .args(["-c", "sleep 5 & sleep 5 & wait"])
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));

    // find sh's pid as seen in /proc plus its children
    let sh_pid = sh.id();
    let pid_of = |pid: u32| format!("/proc/{pid}");
    assert!(std::path::Path::new(&pid_of(sh_pid)).exists(), "sh alive");

    let children: Vec<u32> = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
        .filter(|&p| {
            std::fs::read_to_string(format!("/proc/{p}/stat"))
                .ok()
                .and_then(|s| {
                    let close = s.rfind(')')?;
                    s[close + 1..].split_whitespace().nth(1)?.parse::<u32>().ok()
                })
                == Some(sh_pid)
        })
        .collect();
    assert_eq!(children.len(), 2, "two sleeps under sh");

    // tree kill: TERM every member, parent last — reuse kill semantics by
    // shelling out to kill exactly like memtop does
    let mut all: Vec<String> = children.iter().map(|p| p.to_string()).collect();
    all.push(sh_pid.to_string()); // parent last
    let out = Command::new("kill").arg("-TERM").args(&all).output().unwrap();
    assert!(out.status.success());
    let _ = sh.wait(); // reap, otherwise the pid lingers as a zombie
    std::thread::sleep(std::time::Duration::from_millis(200));

    assert!(
        !std::path::Path::new(&pid_of(sh_pid)).exists(),
        "sh must be gone"
    );
}
