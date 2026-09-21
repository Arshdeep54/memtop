use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::types::MemInfo;

pub(crate) fn read_meminfo() -> MemInfo {
    let mut info = MemInfo {
        total: 0,
        available: 0,
        swap_total: 0,
        swap_free: 0,
    };

    let Ok(content) = std::fs::read_to_string("/proc/meminfo") else {
        return info;
    };

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let (Some(key), Some(val_kib)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(val) = val_kib.parse::<u64>().ok() else {
            continue;
        };
        let bytes = val * 1024;
        match key {
            "MemTotal:" => info.total = bytes,
            "MemAvailable:" => info.available = bytes,
            "SwapTotal:" => info.swap_total = bytes,
            "SwapFree:" => info.swap_free = bytes,
            _ => {}
        }
    }

    info
}

/// PSI averages from `/proc/pressure/memory` (kernel 4.20+): `some` and
/// `full`, each avg10/avg60/avg300 in percent.
#[derive(Clone, Copy)]
pub(crate) struct Pressure {
    pub(crate) some: [f64; 3],
    pub(crate) full: [f64; 3],
}

pub(crate) fn read_pressure(root: &Path) -> Option<Pressure> {
    let text = std::fs::read_to_string(root.join("pressure/memory")).ok()?;
    parse_pressure(&text)
}

pub(crate) fn parse_pressure(text: &str) -> Option<Pressure> {
    let mut out = Pressure {
        some: [0.0; 3],
        full: [0.0; 3],
    };
    let mut seen = false;
    for line in text.lines() {
        let Some((kind, rest)) = line.split_once(' ') else {
            continue;
        };
        let mut vals = [0.0f64; 3];
        for part in rest.split_whitespace() {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let Ok(v) = value.parse() else {
                continue;
            };
            match key {
                "avg10" => vals[0] = v,
                "avg60" => vals[1] = v,
                "avg300" => vals[2] = v,
                _ => {}
            }
        }
        match kind {
            "some" => {
                out.some = vals;
                seen = true;
            }
            "full" => {
                out.full = vals;
                seen = true;
            }
            _ => {}
        }
    }
    if seen {
        Some(out)
    } else {
        None
    }
}

// ponytail: hardcode page size; sysconf(_SC_PAGESIZE) is the upgrade path.
// 4096 is the value on every supported Linux platform in practice.
const PAGE_SIZE: u64 = 4096;

/// Cumulative `pswpin`/`pswpout` page counters from `/proc/vmstat`.
fn vmstat_counters() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/vmstat").ok()?;
    parse_vmstat(&text)
}

pub(crate) fn parse_vmstat(text: &str) -> Option<(u64, u64)> {
    let mut pin = None;
    let mut pout = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next()? {
            "pswpin" => pin = parts.next().and_then(|v| v.parse().ok()),
            "pswpout" => pout = parts.next().and_then(|v| v.parse().ok()),
            _ => {}
        }
    }
    Some((pin?, pout?))
}

/// Swap-in/out rate in bytes/s, sampled 1 s apart. Only meaningful when
/// swap is actually in use; callers gate on `swap_total > swap_free`.
pub(crate) fn swap_rates() -> Option<(f64, f64)> {
    let (in_a, out_a) = vmstat_counters()?;
    thread::sleep(Duration::from_secs(1));
    let (in_b, out_b) = vmstat_counters()?;
    Some((
        in_b.saturating_sub(in_a) as f64 * PAGE_SIZE as f64,
        out_b.saturating_sub(out_a) as f64 * PAGE_SIZE as f64,
    ))
}

/// One parsed OOM kill from the kernel log.
pub(crate) struct OomEvent {
    pub(crate) time: String,
    pub(crate) pid: u32,
    pub(crate) name: String,
    pub(crate) rss: Option<u64>,
}

/// Kernel log lines like
/// `... kernel: Out of memory: Killed process 1234 (node) total-vm:...kB, anon-rss:1200kB, ...`
/// (or the older `Kill process 1234 (chrome) score 900 ...`).
pub(crate) fn parse_oom_log_line(line: &str) -> Option<OomEvent> {
    let marker = line.find("Out of memory:")?;
    let after = &line[marker..];

    let time = line[..marker]
        .split_whitespace()
        .filter(|t| *t != "kernel:")
        .collect::<Vec<_>>()
        .join(" ");

    let rest = after
        .find("Killed process ")
        .map(|i| &after[i + "Killed process ".len()..])
        .or_else(|| {
            after
                .find("Kill process ")
                .map(|i| &after[i + "Kill process ".len()..])
        })?;

    let pid = rest.split_whitespace().next()?.parse().ok()?;
    let open = rest.find('(')?;
    let close = rest[open..].find(')')? + open;
    let name = rest[open + 1..close].to_string();

    let rss = rest
        .split_whitespace()
        .find_map(|t| t.strip_prefix("anon-rss:"))
        .and_then(|v| v.trim_end_matches(',').strip_suffix("kB"))
        .and_then(|v| v.parse::<u64>().ok())
        .map(|kb| kb * 1024);

    Some(OomEvent { time, pid, name, rss })
}

/// OOM kills from `journalctl -k`, falling back to `dmesg`. Both may need
/// privileges (`dmesg_restrict`); the error explains what to try.
pub(crate) fn oom_log() -> Result<Vec<OomEvent>, String> {
    let journal = run_oom_cmd("journalctl", &["-k", "--no-pager"]);
    let text = match journal {
        Ok(t) => t,
        Err(je) => match run_oom_cmd("dmesg", &[]) {
            Ok(t) => t,
            Err(de) => {
                return Err(format!(
                    "could not read the kernel log (journalctl: {je}; dmesg: {de}); try running as root"
                ));
            }
        },
    };
    Ok(text.lines().filter_map(parse_oom_log_line).collect())
}

fn run_oom_cmd(cmd: &str, args: &[&str]) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;

    // journalctl can block indefinitely on a stuck journal; cap it
    let timeout = Duration::from_secs(5);
    let mut child = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut p) = child.stdout.take() {
                    let _ = p.read_to_string(&mut out);
                }
                if status.success() {
                    return Ok(out);
                }
                let mut err = String::new();
                if let Some(mut p) = child.stderr.take() {
                    let _ = p.read_to_string(&mut err);
                }
                let err = err.trim().to_string();
                return if err.is_empty() {
                    Err(format!("exit status {status}"))
                } else {
                    Err(err)
                };
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timed out".to_string());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pressure_some_and_full() {
        let text = "some avg10=1.23 avg60=0.50 avg300=0.10 total=123456\nfull avg10=0.00 avg60=0.01 avg300=0.00 total=789\n";
        let p = parse_pressure(text).unwrap();
        assert_eq!(p.some, [1.23, 0.50, 0.10]);
        assert_eq!(p.full, [0.00, 0.01, 0.00]);
    }

    #[test]
    fn parse_pressure_empty_is_none() {
        assert!(parse_pressure("").is_none());
        assert!(parse_pressure("junk\n").is_none());
    }

    #[test]
    fn parse_vmstat_swap_counters() {
        let text = "pgfault 123\npswpin 45\npswpout 67\nother 1\n";
        assert_eq!(parse_vmstat(text), Some((45, 67)));
        assert_eq!(parse_vmstat("pgfault 1\n"), None);
    }

    #[test]
    fn parse_oom_log_modern_line() {
        let line = "Sep 21 03:12:45 host kernel: Out of memory: Killed process 22154 (node) total-vm:2345678kB, anon-rss:123456kB, shmem-rss:0kB, UID:1000, pgtables:2100kB, oom_score_adj:0";
        let e = parse_oom_log_line(line).unwrap();
        assert_eq!(e.time, "Sep 21 03:12:45 host");
        assert_eq!(e.pid, 22154);
        assert_eq!(e.name, "node");
        assert_eq!(e.rss, Some(123456 * 1024));
    }

    #[test]
    fn parse_oom_log_legacy_line() {
        let line = "[ 1234.567890] Out of memory: Kill process 999 (chrome) score 900 or a child";
        let e = parse_oom_log_line(line).unwrap();
        assert_eq!(e.time, "[ 1234.567890]");
        assert_eq!(e.pid, 999);
        assert_eq!(e.name, "chrome");
        assert_eq!(e.rss, None);
    }

    #[test]
    fn parse_oom_log_non_oom_line_is_none() {
        assert!(parse_oom_log_line("Sep 21 03:12:45 host kernel: usb 1-1: new device").is_none());
    }
}
