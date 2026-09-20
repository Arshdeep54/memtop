use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::process::{exit, Command};

use crate::cli::{Args, SortKey};
use crate::types::{Group, Row};

const INTERPRETERS: &[&str] = &[
    "python", "python2", "python3", "node", "npm", "npx", "yarn", "pnpm", "deno",
    "bun", "ruby", "perl", "php", "java", "sh", "bash", "dash", "zsh", "fish",
];

pub(crate) fn collect_processes(args: &Args) -> Vec<Row> {
    let output = Command::new("ps")
        .args(["-eo", "pid=,rss=,vsz=,pcpu=,nlwp=,user=,args="])
        .output();

    let stdout = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            eprintln!("error: failed to run `ps`");
            exit(1);
        }
    };

    let text = String::from_utf8_lossy(&stdout);
    let mut rows = Vec::with_capacity(text.lines().count());

    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 7 {
            continue;
        }

        let (Ok(pid), Ok(rss_kib), Ok(vsz_kib), Ok(cpu), Ok(threads)) = (
            tokens[0].parse::<u32>(),
            tokens[1].parse::<u64>(),
            tokens[2].parse::<u64>(),
            tokens[3].parse::<f32>(),
            tokens[4].parse::<usize>(),
        ) else {
            continue;
        };

        let args_str = tokens[6..].join(" ");
        if args_str.starts_with('[') {
            continue;
        }

        rows.push(Row {
            pid,
            user: tokens[5].to_string(),
            rss: rss_kib * 1024,
            virt: vsz_kib * 1024,
            cpu,
            threads,
            cmd: command_key(&args_str),
            args: args_str,
        });
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

pub(crate) fn build_rows(mut rows: Vec<Row>, args: &Args, min_mem: Option<u64>) -> Vec<Row> {
    if let Some(min) = min_mem {
        rows.retain(|r| r.rss >= min);
    }

    sort_rows(&mut rows, args.sort);

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
        });
        entry.rss += r.rss;
        entry.virt += r.virt;
        entry.count += 1;
    }

    let mut groups: Vec<Group> = map.into_values().collect();

    if let Some(min) = min_mem {
        groups.retain(|g| g.rss >= min);
    }

    groups.sort_by_key(|g| Reverse(g.rss));

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

fn sort_rows(rows: &mut [Row], key: SortKey) {
    match key {
        SortKey::Mem => rows.sort_by_key(|a| Reverse(a.rss)),
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
