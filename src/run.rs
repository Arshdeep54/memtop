use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, ExitStatus, exit};
use std::thread;
use std::time::{Duration, Instant};

use crate::format::format_bytes;
use crate::procfs;

/// Peak-memory report for `memtop run`. Sampling misses spikes shorter than
/// the interval; the peak is the sum of per-sample PSS, not a kernel
/// high-water mark.
pub(crate) fn run(json: bool, interval_ms: u64, cmd: Vec<String>) {
    if cmd.is_empty() {
        eprintln!("error: no command given (usage: memtop run -- <cmd>)");
        exit(2);
    }

    let mut child = match Command::new(&cmd[0]).args(&cmd[1..]).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to spawn '{}': {e}", cmd[0]);
            exit(127);
        }
    };
    let child_pid = child.id();

    let root = Path::new("/proc");
    let start = Instant::now();
    let mut peak: u64 = 0;
    let mut peak_at: f64 = 0.0;
    let mut peak_procs: usize = 0;
    let mut rss_fallback = false;
    let mut status: Option<ExitStatus> = None;

    while status.is_none() {
        let (total, procs, fallback) = sample_tree(root, child_pid);
        rss_fallback |= fallback;
        if total > peak {
            peak = total;
            peak_at = start.elapsed().as_secs_f64();
            peak_procs = procs;
        }

        match child.try_wait() {
            Ok(Some(s)) => status = Some(s),
            Ok(None) => thread::sleep(Duration::from_millis(interval_ms.max(1))),
            Err(e) => {
                eprintln!("error: waiting for child: {e}");
                exit(1);
            }
        }
    }

    let wall = start.elapsed().as_secs_f64();
    let status = status.unwrap();
    let code = exit_code(&status);

    if rss_fallback {
        eprintln!("note: PSS unavailable for some processes, RSS used as fallback");
    }

    if json {
        let out = serde_json::json!({
            "peak_bytes": peak,
            "peak_at_s": peak_at,
            "peak_processes": peak_procs,
            "wall_s": wall,
            "exit_code": code,
            "rss_fallback": rss_fallback,
        });
        eprintln!("{out}");
    } else {
        eprintln!(
            "\nmemtop run: peak {} across {peak_procs} processes (at {peak_at:.1}s)",
            format_bytes(peak, false),
        );
        eprintln!("wall: {wall:.1}s   exit: {code}");
    }

    // propagate the child's fate, matching shell conventions
    exit(code);
}

/// 128 + signo for signal deaths, the numeric code otherwise.
fn exit_code(status: &ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

/// Sum PSS (RSS fallback) over the child and all its current descendants,
/// found by walking the ppid graph of a fresh /proc scan.
fn sample_tree(root: &Path, root_pid: u32) -> (u64, usize, bool) {
    let pids = procfs::list_pids(root);
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for pid in pids {
        if let Some(stat) = procfs::read_stat(root, pid) {
            children.entry(stat.ppid).or_default().push(pid);
        }
    }

    let mut tree: Vec<u32> = vec![root_pid];
    let mut seen: HashSet<u32> = HashSet::new();
    let mut i = 0;
    while i < tree.len() {
        if let Some(kids) = children.get(&tree[i]) {
            for &kid in kids {
                if seen.insert(kid) {
                    tree.push(kid);
                }
            }
        }
        i += 1;
    }

    let mut total: u64 = 0;
    let mut rss_fallback = false;
    for &pid in &tree {
        let mem = match read_pss(root, pid) {
            Some(pss) => pss,
            None => match procfs::read_status(root, pid) {
                Some(status) => {
                    rss_fallback = true;
                    status.rss.unwrap_or(0)
                }
                None => 0,
            },
        };
        total += mem;
    }

    (total, tree.len(), rss_fallback)
}

/// PSS of one pid. The very first sample can race with the child's execve —
/// `smaps_rollup` returns ENOENT while the mm is being replaced — so retry
/// once before giving up (genuine EACCES still falls through to the caller).
fn read_pss(root: &Path, pid: u32) -> Option<u64> {
    for _ in 0..2 {
        if let Some(rollup) = procfs::read_smaps_rollup(root, pid) {
            return rollup.pss;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_tree_counts_child_and_descendants() {
        // sleep in the background: sh stays alive (wait), the sleep is its child
        let mut child = Command::new("sh")
            .args(["-c", "sleep 1 & sleep 1 & wait"])
            .spawn()
            .unwrap();
        let root = Path::new("/proc");
        thread::sleep(Duration::from_millis(150));

        let (total, procs, fallback) = sample_tree(root, child.id());
        // sh + two sleeps
        assert_eq!(procs, 3);
        assert!(total > 0);
        // all same-user processes on this box, PSS is readable
        assert!(!fallback);

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn sample_tree_single_process() {
        // `exec` guarantees sh replaces itself, so the tree is exactly one process
        let mut child = Command::new("sh")
            .args(["-c", "exec sleep 0.5"])
            .spawn()
            .unwrap();
        let root = Path::new("/proc");
        thread::sleep(Duration::from_millis(100));
        let (total, procs, _) = sample_tree(root, child.id());
        assert_eq!(procs, 1);
        assert!(total > 0);
        let _ = child.wait();
    }
}
