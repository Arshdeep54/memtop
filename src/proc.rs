use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::cli::{Args, SortKey};
use crate::procfs;
use crate::types::{Group, Row};

const INTERPRETERS: &[&str] = &[
    "python", "python2", "python3", "node", "npm", "npx", "yarn", "pnpm", "deno",
    "bun", "ruby", "perl", "php", "java", "sh", "bash", "dash", "zsh", "fish",
];

// ponytail: hardcode CLK_TCK; sysconf(_SC_CLK_TCK) is the upgrade path.
// 100 is the value on every Linux kernel in practice.
const CLK_TCK: f64 = 100.0;

pub(crate) fn collect_processes(args: &Args) -> Vec<Row> {
    let root = Path::new("/proc");
    let uptime = procfs::read_uptime(root);
    let uid_map = procfs::read_uid_map();
    let pids = procfs::list_pids(root);

    let mut rows = Vec::with_capacity(pids.len());

    for pid in pids {
        // races: processes vanish between readdir and read — skip, don't fail
        let Some(stat) = procfs::read_stat(root, pid) else {
            continue;
        };
        let Some(status) = procfs::read_status(root, pid) else {
            continue;
        };
        let Some(argv) = procfs::read_cmdline(root, pid) else {
            continue;
        };
        // empty cmdline = kernel thread (or zombie), as before
        if argv.is_empty() {
            continue;
        }

        let args_str = argv.join(" ");
        let cpu_ticks = (stat.utime + stat.stime) as f64 / CLK_TCK;
        let elapsed = uptime - stat.starttime as f64 / CLK_TCK;
        let cpu = if elapsed > 0.0 {
            (cpu_ticks / elapsed * 100.0) as f32
        } else {
            0.0
        };

        rows.push(Row {
            pid,
            user: uid_map
                .get(&status.uid)
                .cloned()
                .unwrap_or_else(|| status.uid.to_string()),
            rss: status.rss.unwrap_or(0),
            virt: status.virt.unwrap_or(0),
            cpu,
            threads: stat.num_threads,
            cmd: command_key(&args_str),
            args: args_str,
            ppid: stat.ppid,
            start_time: stat.starttime,
            tty_nr: stat.tty_nr,
            pss: None,
            uss: None,
            swap: None,
        });

        // smaps_rollup walks page tables — only pay for it when asked
        if args.pss
            && let Some(rollup) = procfs::read_smaps_rollup(root, pid)
        {
            let row = rows.last_mut().unwrap();
            row.pss = rollup.pss;
            row.uss = rollup.uss;
            row.swap = rollup.swap;
        }
    }

    if let Some(ref user) = args.user {
        rows.retain(|r| &r.user == user);
    }
    if !args.pid.is_empty() {
        let set: HashSet<u32> = args.pid.iter().copied().collect();
        rows.retain(|r| set.contains(&r.pid));
    }

    rows
}

/// The memory value a view sorts and filters on: PSS in --pss mode, RSS otherwise.
pub(crate) fn mem_value(r: &Row, pss: bool) -> u64 {
    if pss {
        r.pss.unwrap_or(0)
    } else {
        r.rss
    }
}

pub(crate) fn build_rows(mut rows: Vec<Row>, args: &Args, min_mem: Option<u64>) -> Vec<Row> {
    if let Some(min) = min_mem {
        rows.retain(|r| mem_value(r, args.pss) >= min);
    }

    sort_rows(&mut rows, args.sort, args.pss);

    if args.reverse {
        rows.reverse();
    }

    if let Some(n) = args.count {
        rows.truncate(n);
    }

    rows
}

pub(crate) fn aggregate(rows: &[Row], args: &Args, min_mem: Option<u64>) -> Vec<Group> {
    let mut map: HashMap<String, Group> = HashMap::new();
    for r in rows {
        let entry = map.entry(r.cmd.clone()).or_insert_with(|| Group {
            name: r.cmd.clone(),
            rss: 0,
            virt: 0,
            count: 0,
            pss: None,
            unreadable: 0,
        });
        entry.rss += r.rss;
        entry.virt += r.virt;
        entry.count += 1;
        if let Some(pss) = r.pss {
            entry.pss = Some(entry.pss.unwrap_or(0) + pss);
        } else {
            entry.unreadable += 1;
        }
    }

    let mut groups: Vec<Group> = map.into_values().collect();

    if let Some(min) = min_mem {
        let total = |g: &Group| if args.pss { g.pss.unwrap_or(0) } else { g.rss };
        groups.retain(|g| total(g) >= min);
    }

    if args.pss {
        groups.sort_by_key(|g| Reverse(g.pss.unwrap_or(0)));
    } else {
        groups.sort_by_key(|g| Reverse(g.rss));
    }

    if args.reverse {
        groups.reverse();
    }

    if let Some(n) = args.count {
        groups.truncate(n);
    }

    groups
}

fn command_key(args: &str) -> String {
    let argv: Vec<&str> = args.split_whitespace().collect();
    if argv.is_empty() {
        return "(unknown)".to_string();
    }

    let base = argv[0].rsplit('/').next().unwrap_or(argv[0]);
    let mut key = base.to_string();

    if INTERPRETERS.contains(&base) {
        let mut added = 0;
        for a in argv.iter().skip(1) {
            if added >= 2 {
                break;
            }
            if a.starts_with('-') {
                continue;
            }
            key.push(' ');
            key.push_str(a);
            added += 1;
        }
    }

    key
}

fn sort_rows(rows: &mut [Row], key: SortKey, pss: bool) {
    match key {
        SortKey::Mem => rows.sort_by_key(|a| Reverse(mem_value(a, pss))),
        SortKey::Virt => rows.sort_by_key(|a| Reverse(a.virt)),
        SortKey::Cpu => rows.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortKey::Pid => rows.sort_by_key(|a| a.pid),
        SortKey::Name => rows.sort_by(|a, b| a.cmd.cmp(&b.cmd)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, cmd: &str, rss: u64, pss: Option<u64>) -> Row {
        Row {
            pid,
            user: "u".to_string(),
            rss,
            virt: rss * 2,
            cpu: 0.0,
            threads: 1,
            cmd: cmd.to_string(),
            args: cmd.to_string(),
            ppid: 1,
            start_time: 100,
            tty_nr: 0,
            pss,
            uss: None,
            swap: None,
        }
    }

    fn test_args(pss: bool) -> Args {
        Args {
            count: None,
            sort: SortKey::Mem,
            reverse: false,
            user: None,
            pid: Vec::new(),
            min_mem: None,
            bytes: false,
            threads: false,
            group: false,
            pss,
            summary: false,
            json: false,
            watch: false,
            interval: 2.0,
            kill: false,
        }
    }

    #[test]
    fn aggregate_sums_rss_and_counts() {
        let rows = vec![
            row(1, "app", 100, None),
            row(2, "app", 200, None),
            row(3, "other", 50, None),
        ];
        let groups = aggregate(&rows, &test_args(false), None);
        let app = groups.iter().find(|g| g.name == "app").unwrap();
        assert_eq!(app.rss, 300);
        assert_eq!(app.virt, 600);
        assert_eq!(app.count, 2);
        assert_eq!(groups[0].name, "app");
    }

    #[test]
    fn aggregate_pss_skips_unreadable_members() {
        // one member has PSS 80 (<= rss 100), one is unreadable
        let rows = vec![row(1, "app", 100, Some(80)), row(2, "app", 200, None)];
        let groups = aggregate(&rows, &test_args(true), None);
        let app = &groups[0];
        assert_eq!(app.pss, Some(80)); // RSS of the unreadable member must NOT be folded in
        assert_eq!(app.unreadable, 1);
        assert_eq!(app.count, 2);
    }

    #[test]
    fn aggregate_group_pss_never_exceeds_group_rss() {
        let rows = vec![
            row(1, "app", 100, Some(80)),
            row(2, "app", 200, Some(150)),
            row(3, "app", 300, None),
        ];
        let groups = aggregate(&rows, &test_args(true), None);
        let g = &groups[0];
        assert!(g.pss.unwrap() <= g.rss);
    }

    #[test]
    fn aggregate_all_unreadable_pss_is_none() {
        let rows = vec![row(1, "app", 100, None)];
        let groups = aggregate(&rows, &test_args(true), None);
        assert_eq!(groups[0].pss, None);
        assert_eq!(groups[0].unreadable, 1);
    }

    #[test]
    fn min_mem_filters_on_pss_when_pss_mode() {
        let args = test_args(true);
        let rows = vec![row(1, "a", 1000, Some(10)), row(2, "b", 100, Some(900))];
        let out = build_rows(rows, &args, Some(500));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].cmd, "b");
    }

    #[test]
    fn sort_mem_uses_pss_when_pss_mode() {
        let args = test_args(true);
        let rows = vec![row(1, "a", 1000, Some(10)), row(2, "b", 100, Some(900))];
        let out = build_rows(rows, &args, None);
        assert_eq!(out[0].cmd, "b");
    }

    #[test]
    fn command_key_simple() {
        assert_eq!(command_key("firefox"), "firefox");
        assert_eq!(command_key("/usr/bin/firefox"), "firefox");
    }

    #[test]
    fn command_key_interpreter_keeps_args() {
        // interpreters keep up to 2 non-flag args
        assert_eq!(command_key("node server.js --port 3000"), "node server.js 3000");
        assert_eq!(
            command_key("/usr/bin/python3 -u script.py extra"),
            "python3 script.py extra"
        );
    }

    #[test]
    fn command_key_interpreter_skips_flags() {
        assert_eq!(command_key("node --max-old-space-size=4096 app.js"), "node app.js");
        assert_eq!(command_key("python3 -u -B main.py"), "python3 main.py");
    }

    #[test]
    fn command_key_non_interpreter_ignores_args() {
        assert_eq!(command_key("chrome --flag value"), "chrome");
    }

    #[test]
    fn command_key_empty() {
        assert_eq!(command_key(""), "(unknown)");
        assert_eq!(command_key("   "), "(unknown)");
    }
}
