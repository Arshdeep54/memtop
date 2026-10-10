use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::thread;
use std::time::Duration;

use crate::cli::{Args, GuardAction};
use crate::kill::kill_pids;
use crate::mem::{self, Pressure};
use crate::proc::collect_processes;
use crate::types::MemInfo;

pub(crate) const SERVICE: &str = "memtop-guard";

const EXAMPLE: &str = r#"# memtop guard config. Edit the thresholds and app list before starting.
# After editing an active config, run `memtop guard start` again to apply it.
#
# The guard acts only when RAM is nearly gone AND the system is under memory
# pressure. Low RAM alone is normal: Linux uses spare memory as cache.
#
# Trigger condition:
#   available RAM < mem_available_below_pct
#     AND (swap used > swap_used_above_pct
#          OR memory PSI full avg10 > psi_full_avg10_above)

# Available RAM threshold as a percent of total RAM. Higher values act sooner.
# 5 means the guard can act when less than 5% is available.
mem_available_below_pct = 5

# Swap usage threshold as a percent of total swap. Without swap, this signal
# stays at 0; the PSI threshold can still trigger the guard.
swap_used_above_pct = 90

# Memory stall threshold from /proc/pressure/memory (full avg10), in percent.
# It measures how much of the last 10 seconds all non-idle tasks stalled on
# memory. Lower values act sooner when the system starts freezing.
psi_full_avg10_above = 5

# Check memory this often, in seconds.
interval_secs = 1

# Wait this long after a trigger before checking again, so memory can recover.
# If pressure remains high, the next app in the list may be killed.
cooldown_secs = 5

# Send SIGTERM first; after this many seconds, send SIGKILL to survivors.
grace_secs = 3

# Apps to kill, in priority order: one app per trigger, first running match
# first. All matching processes are killed. Replace these examples with apps
# you are willing to close; put the least important app first.
# `memtop -g` shows app names. "zen" matches zen, zen-bin, zen_helper, but
# not "frozen" or "zenity".
apps = ["zen", "slack"]"#;

#[derive(Debug, PartialEq)]
pub(crate) struct Config {
    mem_available_below_pct: f64,
    swap_used_above_pct: f64,
    psi_full_avg10_above: f64,
    interval_secs: f64,
    cooldown_secs: f64,
    grace_secs: f64,
    /// killed in this order, one app per trigger
    apps: Vec<String>,
}

pub(crate) fn parse_config(text: &str) -> Result<Config, String> {
    let mut cfg = Config {
        mem_available_below_pct: 8.0,
        swap_used_above_pct: 85.0,
        psi_full_avg10_above: 5.0,
        interval_secs: 1.0,
        cooldown_secs: 5.0,
        grace_secs: 3.0,
        apps: Vec::new(),
    };
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = |m: &str| format!("line {}: {m}", n + 1);
        let (key, val) = line
            .split_once('=')
            .ok_or_else(|| at("expected `key = value`"))?;
        let (key, val) = (key.trim(), val.trim());
        if key == "apps" {
            let inner = val
                .strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .ok_or_else(|| at("apps must look like [\"zen\", \"chrome\"]"))?;
            cfg.apps = inner
                .split(',')
                .map(|a| a.trim().trim_matches('"').to_string())
                .filter(|a| !a.is_empty())
                .collect();
            continue;
        }
        let num: f64 = val.parse().map_err(|_| at("value must be a number"))?;
        match key {
            "mem_available_below_pct" => cfg.mem_available_below_pct = num,
            "swap_used_above_pct" => cfg.swap_used_above_pct = num,
            "psi_full_avg10_above" => cfg.psi_full_avg10_above = num,
            "interval_secs" => cfg.interval_secs = num,
            "cooldown_secs" => cfg.cooldown_secs = num,
            "grace_secs" => cfg.grace_secs = num,
            _ => return Err(at(&format!("unknown key `{key}`"))),
        }
    }
    if cfg.apps.is_empty() {
        return Err("`apps` is empty, nothing to guard".to_string());
    }
    // Duration::from_secs_f64 panics on negative/NaN, and it would do so right after a kill
    for (name, v) in [
        ("cooldown_secs", cfg.cooldown_secs),
        ("grace_secs", cfg.grace_secs),
    ] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(format!("{name} must be zero or positive"));
        }
    }
    if !(cfg.interval_secs.is_finite() && cfg.interval_secs > 0.0) {
        return Err("interval_secs must be positive".to_string());
    }
    Ok(cfg)
}

/// RAM is nearly gone AND the system is actually suffering for it (swap
/// filling up, or tasks stalling on memory). Low RAM alone is normal: the
/// kernel keeps spare memory as cache.
pub(crate) fn should_trigger(cfg: &Config, mem: &MemInfo, psi: Option<&Pressure>) -> bool {
    if mem.total == 0 {
        return false;
    }
    let avail_pct = mem.available as f64 * 100.0 / mem.total as f64;
    let swap_pct = if mem.swap_total == 0 {
        0.0
    } else {
        (mem.swap_total - mem.swap_free) as f64 * 100.0 / mem.swap_total as f64
    };
    let stalling = psi.is_some_and(|p| p.full[0] > cfg.psi_full_avg10_above);
    avail_pct < cfg.mem_available_below_pct && (swap_pct > cfg.swap_used_above_pct || stalling)
}

/// `zen` matches `zen`, `zen-bin`, `zen_helper`, not `frozen`.
pub(crate) fn matches_app(argv: &str, app: &str) -> bool {
    let first = argv.split_whitespace().next().unwrap_or("");
    let base = first.rsplit('/').next().unwrap_or(first).to_lowercase();
    let app = app.to_lowercase();
    base == app
        || base
            .strip_prefix(&app)
            .is_some_and(|rest| rest.starts_with(['-', '_']))
}

fn app_targets(args: &Args, app: &str) -> Vec<(u32, u64)> {
    let me = std::process::id();
    collect_processes(args)
        .iter()
        .filter(|r| r.pid != me && matches_app(&r.args, app))
        .map(|r| (r.pid, r.start_time))
        .collect()
}

fn default_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("memtop/guard.toml"))
}

/// First app in priority order that is running: TERM it, wait, KILL survivors.
fn act(args: &Args, cfg: &Config, dry_run: bool) -> bool {
    for app in &cfg.apps {
        let targets = app_targets(args, app);
        if targets.is_empty() {
            continue;
        }
        if dry_run {
            eprintln!("guard: dry-run, would kill {app} ({} pids)", targets.len());
            return true;
        }
        eprintln!("guard: killing {app} ({} pids)", targets.len());
        if let Err(e) = kill_pids(&targets, "TERM") {
            eprintln!("guard: TERM: {e}");
        }
        thread::sleep(Duration::from_secs_f64(cfg.grace_secs));
        let left = app_targets(args, app);
        if !left.is_empty() {
            eprintln!("guard: {} {app} pids survived TERM, sending KILL", left.len());
            if let Err(e) = kill_pids(&left, "KILL") {
                eprintln!("guard: KILL: {e}");
            }
        }
        let _ = Command::new("notify-send")
            .args([
                "-u",
                "normal",
                "-t",
                "10000",
                "memtop guard",
                &format!("memory critical: killed {app}"),
            ])
            .status();
        return true;
    }
    eprintln!("guard: memory critical but none of the configured apps are running");
    false
}

fn systemctl(args: &[&str]) -> bool {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) => s.success(),
        Err(e) => {
            eprintln!("error: failed to run systemctl: {e}");
            false
        }
    }
}

fn unit_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(format!(".config/systemd/user/{SERVICE}.service"))
}

/// Install the user unit and enable it: runs now and on every login.
fn start(path: &Path) {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, format!("{EXAMPLE}\n")).unwrap_or_else(|e| {
            eprintln!("error: cannot write {}: {e}", path.display());
            exit(1);
        });
        eprintln!(
            "created {} (service not started). Review the thresholds and replace the example apps, then run `memtop guard start` again",
            path.display()
        );
        return;
    }
    let invalid = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| parse_config(&t).map(|_| ()));
    if let Err(e) = invalid {
        eprintln!("error: {}: {e}", path.display());
        exit(2);
    }
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("error: current exe: {e}");
        exit(1);
    });
    let unit = format!(
        "[Unit]\nDescription=memtop guard: kill configured apps before memory exhaustion\n\n\
         [Service]\nExecStart=\"{}\" guard --config \"{}\"\nRestart=on-failure\n\
         CPUWeight=1000\nMemoryLow=64M\n\n\
         [Install]\nWantedBy=default.target\n",
        exe.display(),
        path.display()
    );
    let unit_file = unit_path();
    if let Some(dir) = unit_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&unit_file, unit) {
        eprintln!("error: cannot write {}: {e}", unit_file.display());
        exit(1);
    }
    if !(systemctl(&["daemon-reload"]) && systemctl(&["enable", SERVICE])
        && systemctl(&["restart", SERVICE])) {
        exit(1);
    }
    println!("guard running and enabled at login. stop it with `memtop guard stop`");
}

pub(crate) fn guard(
    args: &Args,
    config: Option<PathBuf>,
    dry_run: bool,
    once: bool,
    action: Option<GuardAction>,
) {
    let Some(path) = config.or_else(default_config_path) else {
        eprintln!("error: no --config given and HOME is unset");
        exit(2);
    };
    match action {
        Some(GuardAction::Start) => return start(&path),
        Some(GuardAction::Stop) => {
            // disable too, or it would come back at the next login
            if !systemctl(&["disable", "--now", SERVICE]) {
                exit(1);
            }
            return println!("guard stopped and disabled");
        }
        Some(GuardAction::Status) => exit(if systemctl(&["status", "--no-pager", SERVICE]) { 0 } else { 3 }),
        None => {}
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!(
            "error: cannot read {}: {e}\nexample config:\n{EXAMPLE}",
            path.display()
        );
        exit(2);
    });
    let cfg = parse_config(&text).unwrap_or_else(|e| {
        eprintln!("error: {}: {e}", path.display());
        exit(2);
    });
    eprintln!("guard: watching, apps in kill order: {}", cfg.apps.join(", "));
    // catches typos and wrong process names now, not during the emergency
    for app in cfg.apps.iter().filter(|a| app_targets(args, a).is_empty()) {
        eprintln!("guard: note: no running process matches `{app}` (fine if it is just closed)");
    }

    loop {
        let psi = mem::read_pressure(Path::new("/proc"));
        if should_trigger(&cfg, &mem::read_meminfo(), psi.as_ref()) {
            act(args, &cfg, dry_run);
            if !once {
                thread::sleep(Duration::from_secs_f64(cfg.cooldown_secs));
            }
        }
        if once {
            return;
        }
        thread::sleep(Duration::from_secs_f64(cfg.interval_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(avail: u64, swap_used: u64) -> MemInfo {
        MemInfo {
            total: 1000,
            available: avail,
            swap_total: 1000,
            swap_free: 1000 - swap_used,
        }
    }

    fn psi(full10: f64) -> Pressure {
        Pressure {
            some: [0.0; 3],
            full: [full10, 0.0, 0.0],
        }
    }

    fn cfg() -> Config {
        parse_config("apps = [\"zen\"]").unwrap()
    }

    #[test]
    fn parse_example_and_values() {
        let c = parse_config(EXAMPLE).unwrap();
        assert_eq!(c.apps, ["zen", "slack"]);
        assert_eq!(
            c,
            parse_config(
                "mem_available_below_pct = 5\nswap_used_above_pct = 90\npsi_full_avg10_above = 5\ninterval_secs = 1\ncooldown_secs = 5\ngrace_secs = 3\napps = [\"zen\", \"slack\"]"
            )
            .unwrap()
        );
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse_config("").unwrap_err().contains("apps"));
        assert!(parse_config("apps = [\"a\"]\ntypo = 1").unwrap_err().contains("unknown key"));
        assert!(parse_config("apps = [\"a\"]\ninterval_secs = x").unwrap_err().contains("number"));
        assert!(parse_config("apps = zen").unwrap_err().contains("apps must"));
        for bad in ["grace_secs = -1", "cooldown_secs = nan", "interval_secs = 0"] {
            assert!(parse_config(&format!("apps = [\"a\"]\n{bad}")).is_err(), "{bad}");
        }
    }

    #[test]
    fn trigger_needs_low_ram_and_a_second_signal() {
        let c = cfg();
        // plenty of RAM: never, even with swap full and stalls
        assert!(!should_trigger(&c, &mem(500, 1000), Some(&psi(50.0))));
        // low RAM alone is normal cache behaviour
        assert!(!should_trigger(&c, &mem(50, 100), Some(&psi(0.0))));
        // low RAM + swap nearly full
        assert!(should_trigger(&c, &mem(50, 900), None));
        // low RAM + stalling
        assert!(should_trigger(&c, &mem(50, 0), Some(&psi(9.0))));
        // no swap configured and no PSI: cannot trigger
        let no_swap = MemInfo { swap_total: 0, swap_free: 0, ..mem(50, 0) };
        assert!(!should_trigger(&c, &no_swap, None));
    }

    #[test]
    fn app_matching() {
        assert!(matches_app("/opt/zen/zen-bin -contentproc 1", "zen"));
        assert!(matches_app("zen", "ZEN"));
        assert!(matches_app("/usr/bin/zen_helper", "zen"));
        assert!(!matches_app("/usr/bin/frozen", "zen"));
        assert!(!matches_app("/usr/bin/zenity", "zen"));
        assert!(!matches_app("", "zen"));
    }
}
