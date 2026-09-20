use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub(crate) struct StatInfo {
    #[allow(dead_code)]
    pub(crate) state: char,
    pub(crate) ppid: u32,
    pub(crate) tty_nr: u64,
    pub(crate) utime: u64,
    pub(crate) stime: u64,
    pub(crate) num_threads: usize,
    pub(crate) starttime: u64,
}

pub(crate) struct StatusInfo {
    pub(crate) uid: u32,
    pub(crate) rss: Option<u64>,
    pub(crate) virt: Option<u64>,
}

/// Numeric directory names under the proc root.
pub(crate) fn list_pids(root: &Path) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if let Ok(pid) = name.parse::<u32>() {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}

/// `/proc/<pid>/stat`. The `comm` field is parenthesised and may contain
/// spaces and `)`, so split at the last `)`.
pub(crate) fn read_stat(root: &Path, pid: u32) -> Option<StatInfo> {
    let path = root.join(pid.to_string()).join("stat");
    let text = fs::read_to_string(path).ok()?;
    parse_stat(&text)
}

pub(crate) fn parse_stat(s: &str) -> Option<StatInfo> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    if close < open {
        return None;
    }
    let _pid: u32 = s[..open].trim().parse().ok()?;
    let fields: Vec<&str> = s[close + 1..].split_whitespace().collect();
    if fields.len() < 20 {
        return None;
    }
    Some(StatInfo {
        state: fields[0].chars().next()?,
        ppid: fields[1].parse().ok()?,
        tty_nr: fields[4].parse().ok()?,
        utime: fields[11].parse().ok()?,
        stime: fields[12].parse().ok()?,
        num_threads: fields[17].parse().ok()?,
        starttime: fields[19].parse().ok()?,
    })
}

/// `/proc/<pid>/status`: `Uid` (first value is the real uid), `VmRSS`,
/// `VmSize` (kB values converted to bytes).
pub(crate) fn read_status(root: &Path, pid: u32) -> Option<StatusInfo> {
    let path = root.join(pid.to_string()).join("status");
    let text = fs::read_to_string(path).ok()?;
    parse_status(&text)
}

pub(crate) fn parse_status(s: &str) -> Option<StatusInfo> {
    let mut uid = None;
    let mut rss = None;
    let mut virt = None;
    for line in s.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "Uid" => {
                uid = rest.split_whitespace().next()?.parse().ok();
            }
            "VmRSS" => {
                rss = parse_kb_line(rest);
            }
            "VmSize" => {
                virt = parse_kb_line(rest);
            }
            _ => {}
        }
    }
    Some(StatusInfo {
        uid: uid?,
        rss,
        virt,
    })
}

fn parse_kb_line(rest: &str) -> Option<u64> {
    let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
    Some(value * 1024)
}

/// NUL-separated argv. An empty cmdline means a kernel thread (or a zombie);
/// callers skip those, matching the old "skip `[...]`" behaviour.
pub(crate) fn read_cmdline(root: &Path, pid: u32) -> Option<Vec<String>> {
    let path = root.join(pid.to_string()).join("cmdline");
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    let argv: Vec<String> = bytes
        .split(|&b| b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    Some(argv)
}

/// First field of `/proc/uptime`, seconds since boot.
pub(crate) fn read_uptime(root: &Path) -> f64 {
    let path = root.join("uptime");
    let Ok(text) = fs::read_to_string(path) else {
        return 0.0;
    };
    parse_uptime(&text)
}

pub(crate) fn parse_uptime(s: &str) -> f64 {
    s.split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

/// uid → username from `/etc/passwd`, parsed once per scan. Falls back to
/// the numeric uid at the call site for unknown ids.
pub(crate) fn read_uid_map() -> HashMap<u32, String> {
    match fs::read_to_string("/etc/passwd") {
        Ok(text) => parse_passwd(&text),
        Err(_) => HashMap::new(),
    }
}

pub(crate) fn parse_passwd(text: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3
            && let (Ok(uid), false) = (fields[2].parse::<u32>(), fields[0].is_empty())
        {
            map.insert(uid, fields[0].to_string());
        }
    }
    map
}

/// `Pss:`, `Private_Clean:` + `Private_Dirty:` (USS), `Swap:` from
/// `/proc/<pid>/smaps_rollup` (kernel 4.14+), in bytes. Unreadable
/// (`EACCES`, `ENOENT`) returns `None`.
pub(crate) fn read_smaps_rollup(root: &Path, pid: u32) -> Option<SmapsRollup> {
    let path = root.join(pid.to_string()).join("smaps_rollup");
    let text = fs::read_to_string(path).ok()?;
    parse_smaps_rollup(&text)
}

#[derive(Default)]
pub(crate) struct SmapsRollup {
    pub(crate) pss: Option<u64>,
    pub(crate) uss: Option<u64>,
    pub(crate) swap: Option<u64>,
}

pub(crate) fn parse_smaps_rollup(s: &str) -> Option<SmapsRollup> {
    let mut out = SmapsRollup::default();
    for line in s.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(kb) = parse_kb_line(rest) else {
            continue;
        };
        match key.trim() {
            "Pss" => out.pss = Some(kb),
            "Private_Clean" => out.uss = Some(out.uss.unwrap_or(0) + kb),
            "Private_Dirty" => out.uss = Some(out.uss.unwrap_or(0) + kb),
            "Swap" => out.swap = Some(kb),
            _ => {}
        }
    }
    out.pss?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("memtop-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, pid: u32, file: &str, content: &str) {
        let dir = root.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(file), content).unwrap();
    }

    #[test]
    fn parse_stat_normal_process() {
        let stat = "1234 (firefox) S 1200 1234 1200 34816 1234 4194560 500 0 0 0 100 50 0 0 20 0 8 0 98765 1234567890 9876 18446744073709551615";
        let s = parse_stat(stat).unwrap();
        assert_eq!(s.state, 'S');
        assert_eq!(s.ppid, 1200);
        assert_eq!(s.tty_nr, 34816);
        assert_eq!(s.utime, 100);
        assert_eq!(s.stime, 50);
        assert_eq!(s.num_threads, 8);
        assert_eq!(s.starttime, 98765);
    }

    #[test]
    fn parse_stat_comm_with_parens_and_spaces() {
        // comm itself contains ") (" — splitting at the LAST ')' must survive
        let stat = "42 (we) (are) (paren) S 1 42 42 0 -1 4194560 0 0 0 0 10 5 0 0 20 0 4 0 5000 1000000 3000 0";
        let s = parse_stat(stat).unwrap();
        assert_eq!(s.ppid, 1);
        assert_eq!(s.state, 'S');
        assert_eq!(s.utime, 10);
        assert_eq!(s.starttime, 5000);
    }

    #[test]
    fn parse_stat_garbage() {
        assert!(parse_stat("").is_none());
        assert!(parse_stat("not a stat").is_none());
        assert!(parse_stat("42 (short) S 1").is_none());
    }

    #[test]
    fn parse_status_fields() {
        let status = "Name:\tbash\nUid:\t1000\t1000\t1000\t5\nGid:\t1000\t1000\t1000\t1000\nVmRSS:\t  2048 kB\nVmSize:\t  4096 kB\nThreads:\t1\n";
        let s = parse_status(status).unwrap();
        assert_eq!(s.uid, 1000);
        assert_eq!(s.rss, Some(2048 * 1024));
        assert_eq!(s.virt, Some(4096 * 1024));
    }

    #[test]
    fn parse_status_missing_vm_lines() {
        let s = parse_status("Name:\tx\nUid:\t0\t0\t0\t0\n").unwrap();
        assert_eq!(s.uid, 0);
        assert_eq!(s.rss, None);
        assert_eq!(s.virt, None);
    }

    #[test]
    fn parse_status_no_uid_is_none() {
        assert!(parse_status("Name:\tx\n").is_none());
    }

    #[test]
    fn list_and_read_fixture_processes() {
        let root = fixture_root("collect");

        write(&root, 42, "stat", "42 (node) S 1 42 42 0 -1 4194560 0 0 0 0 10 5 0 0 20 0 4 0 5000 1000000 3000 0");
        write(&root, 42, "status", "Uid:\t1000\t1000\t1000\t1000\nVmRSS:\t 2048 kB\nVmSize:\t 8192 kB\n");
        write(&root, 42, "cmdline", "/usr/bin/node\0server.js\0");

        // kernel thread: empty cmdline
        write(&root, 7, "stat", "7 (kworker/0:1) S 2 0 0 0 -1 69238880 0 0 0 0 0 0 0 0 20 0 1 0 1 100 0 0");
        write(&root, 7, "status", "Uid:\t0\t0\t0\t0\n");
        write(&root, 7, "cmdline", "");

        // process that vanishes mid-read: dir exists, files missing
        fs::create_dir_all(root.join("99")).unwrap();

        assert_eq!(list_pids(&root), vec![7, 42, 99]);

        let argv = read_cmdline(&root, 42).unwrap();
        assert_eq!(argv, vec!["/usr/bin/node", "server.js"]);
        // kernel thread has empty argv
        assert!(read_cmdline(&root, 7).unwrap().is_empty());

        let stat = read_stat(&root, 42).unwrap();
        assert_eq!(stat.ppid, 1);
        assert_eq!(stat.starttime, 5000);
        // vanished process: read fails, caller skips
        assert!(read_stat(&root, 99).is_none());

        let status = read_status(&root, 42).unwrap();
        assert_eq!(status.rss, Some(2048 * 1024));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parse_uptime_value() {
        assert_eq!(parse_uptime("12345.67 45678.90\n"), 12345.67);
        assert_eq!(parse_uptime("junk"), 0.0);
    }

    #[test]
    fn parse_passwd_map() {
        let map = parse_passwd("root:x:0:0:root:/root:/bin/bash\ncosign:x:1000:1000:,,,:/home/cosign:/bin/bash\nbroken\n:x:5:\n");
        assert_eq!(map.get(&0).map(String::as_str), Some("root"));
        assert_eq!(map.get(&1000).map(String::as_str), Some("cosign"));
        assert_eq!(map.get(&5), None);
    }

    #[test]
    fn parse_smaps_rollup_fields() {
        let text = "Rss:              100 kB\nPss:               80 kB\nPrivate_Clean:     10 kB\nPrivate_Dirty:     20 kB\nSwap:              12 kB\nSwapPss:            2 kB\n";
        let r = parse_smaps_rollup(text).unwrap();
        assert_eq!(r.pss, Some(80 * 1024));
        assert_eq!(r.uss, Some(30 * 1024)); // clean + dirty
        assert_eq!(r.swap, Some(12 * 1024));
    }

    #[test]
    fn parse_smaps_rollup_dirty_only() {
        let r = parse_smaps_rollup("Pss: 5 kB\nPrivate_Dirty: 7 kB\n").unwrap();
        assert_eq!(r.uss, Some(7 * 1024));
    }

    #[test]
    fn parse_smaps_rollup_no_pss_is_none() {
        assert!(parse_smaps_rollup("Rss: 100 kB\n").is_none());
        assert!(parse_smaps_rollup("").is_none());
    }

    #[test]
    fn read_smaps_rollup_missing_file_is_none() {
        let root = fixture_root("smaps-missing");
        assert!(read_smaps_rollup(&root, 1).is_none());
        fs::remove_dir_all(&root).ok();
    }
}