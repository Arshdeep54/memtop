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
