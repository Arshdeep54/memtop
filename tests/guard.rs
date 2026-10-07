use std::process::{Command, Stdio};
use std::time::Duration;

/// Thresholds that always trigger: avail < 101% and swap used > -1%.
const FORCED: &str = "mem_available_below_pct = 101\nswap_used_above_pct = -1\ngrace_secs = 0.2\n";

/// copy+spawn must not overlap across tests: a fork can inherit another
/// test's open write fd and the exec then fails with "Text file busy"
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A copy of `sleep` under a unique name, so only our victim matches.
fn victim(tag: &str) -> (std::process::Child, std::path::PathBuf, String) {
    let _lock = SPAWN.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("memtop-guard-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let name = format!("guardvictim{tag}");
    let bin = dir.join(&name);
    std::fs::copy("/bin/sleep", &bin).unwrap();
    let child = Command::new(&bin).arg("60").stdout(Stdio::null()).spawn().unwrap();
    let conf = dir.join("guard.toml");
    std::fs::write(&conf, format!("{FORCED}apps = [\"nosuchapp\", \"{name}\"]\n")).unwrap();
    (child, conf, name)
}

fn guard(conf: &std::path::Path, extra: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_memtop"))
        .args(["guard", "--once", "--config"])
        .arg(conf)
        .args(extra)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn guard_dry_run_leaves_process_alone() {
    let (mut child, conf, _) = victim("dry");
    guard(&conf, &["--dry-run"]);
    std::thread::sleep(Duration::from_millis(100));
    assert!(child.try_wait().unwrap().is_none(), "dry-run killed the process");
    child.kill().unwrap();
}

#[test]
fn guard_kills_first_running_app_in_list() {
    let (mut child, conf, _) = victim("kill");
    guard(&conf, &[]);
    let status = child.wait().unwrap(); // would hang 60s if not killed
    assert!(!status.success());
}
