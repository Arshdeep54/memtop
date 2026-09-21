use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// inode → local port for every LISTEN socket (state `0A`) in
/// `/proc/net/tcp` and `/proc/net/tcp6`. UDP is out of scope.
pub(crate) fn listening_inodes(root: &Path) -> HashMap<u64, u16> {
    let mut map = HashMap::new();
    for file in ["net/tcp", "net/tcp6"] {
        if let Ok(text) = fs::read_to_string(root.join(file)) {
            for (inode, port) in parse_tcp(&text) {
                map.insert(inode, port);
            }
        }
    }
    map
}

/// Parse one `/proc/net/tcp[6]` body: keep state `0A` (LISTEN), extract the
/// local port (hex) and socket inode (column 10).
pub(crate) fn parse_tcp(text: &str) -> HashMap<u64, u16> {
    let mut map = HashMap::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[3] != "0A" {
            continue;
        }
        let Some((_, port_hex)) = fields[1].rsplit_once(':') else {
            continue;
        };
        let (Ok(port), Ok(inode)) = (
            u16::from_str_radix(port_hex, 16),
            fields[9].parse::<u64>(),
        ) else {
            continue;
        };
        map.insert(inode, port);
    }
    map
}

/// Socket inodes among `/proc/<pid>/fd/*` symlinks (`socket:[<inode>]`).
/// `None` when the fd directory is unreadable (another user's process).
pub(crate) fn fd_socket_inodes(root: &Path, pid: u32) -> Option<Vec<u64>> {
    let entries = fs::read_dir(root.join(pid.to_string()).join("fd")).ok()?;
    let mut inodes = Vec::new();
    for entry in entries.flatten() {
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        let Some(inner) = target
            .strip_prefix("socket:[")
            .and_then(|s| s.strip_suffix(']'))
        else {
            continue;
        };
        if let Ok(inode) = inner.parse::<u64>() {
            inodes.push(inode);
        }
    }
    Some(inodes)
}

/// Ports each pid listens on. Returns (pid → ports, count of unreadable fds).
pub(crate) fn ports_by_pid(
    root: &Path,
    pids: &[u32],
) -> (HashMap<u32, Vec<u16>>, usize) {
    let listeners = listening_inodes(root);
    let mut out: HashMap<u32, Vec<u16>> = HashMap::new();
    let mut unreadable = 0;

    for &pid in pids {
        let Some(inodes) = fd_socket_inodes(root, pid) else {
            unreadable += 1;
            continue;
        };
        let mut ports: Vec<u16> = inodes
            .into_iter()
            .filter_map(|i| listeners.get(&i).copied())
            .collect();
        ports.sort_unstable();
        ports.dedup();
        out.insert(pid, ports);
    }

    (out, unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn fixture_root(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("memtop-net-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn parse_tcp_keeps_only_listen() {
        // header + LISTEN + an established connection (must be ignored)
        let text = "  sl local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1 ffffabc000000000 100 0 0 10 0\n   1: 0100007F:9C40 0100007F:0BB8 01 00000000:00000000 00:00000000 00000000  1000        0 99999 1 ffffabc000000001 20 4 30 10 -1\n";
        let map = parse_tcp(text);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&12345), Some(&0x0BB8)); // 3000
        assert!(!map.contains_key(&99999));
    }

    #[test]
    fn parse_tcp_ipv6_and_garbage() {
        let text = "  sl local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000000000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 777 1\n   junk line\n";
        let map = parse_tcp(text);
        assert_eq!(map.get(&777), Some(&22));
    }

    #[test]
    fn fd_socket_inodes_from_fixture_symlinks() {
        let root = fixture_root("fd");
        let fd_dir = root.join("42/fd");
        fs::create_dir_all(&fd_dir).unwrap();
        let fake = root.join("nowhere");
        fs::write(&fake, "").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("socket:[12345]", fd_dir.join("3")).unwrap();
            std::os::unix::fs::symlink("socket:[999]", fd_dir.join("4")).unwrap();
            std::os::unix::fs::symlink("pipe:[555]", fd_dir.join("5")).unwrap();
            std::os::unix::fs::symlink(&fake, fd_dir.join("6")).unwrap();
        }

        let inodes = fd_socket_inodes(&root, 42).unwrap();
        let set: HashSet<u64> = inodes.into_iter().collect();
        assert!(set.contains(&12345));
        assert!(set.contains(&999));
        assert!(!set.contains(&555)); // pipe, not a socket

        // unreadable fd dir -> None
        assert_eq!(fd_socket_inodes(&root, 43), None);
        fs::remove_dir_all(&root).ok();
    }
}
