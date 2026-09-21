use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::cli::Args;
use crate::format::{format_bytes, pct_of};
use crate::types::{Group, MemInfo, Row};

pub(crate) fn render_system_summary(mem: &MemInfo, args: &Args) -> String {
    let used = mem.total.saturating_sub(mem.available);

    let mut out = format!(
        "Total: {}   Used: {} ({:.1}%)   Available: {}\n",
        format_bytes(mem.total, args.bytes),
        format_bytes(used, args.bytes),
        pct_of(used, mem.total),
        format_bytes(mem.available, args.bytes),
    );

    if mem.swap_total > 0 {
        out.push_str(&format!(
            "Swap:  {} used / {} total\n",
            format_bytes(mem.swap_total - mem.swap_free, args.bytes),
            format_bytes(mem.swap_total, args.bytes),
        ));
    }

    out
}

/// A right- or left-aligned table column; cells must be one per row.
pub(crate) struct Col {
    pub(crate) header: &'static str,
    pub(crate) cells: Vec<String>,
    pub(crate) right: bool,
}

pub(crate) fn render_columns(cols: &[Col]) -> String {
    let widths: Vec<usize> = cols
        .iter()
        .map(|c| {
            c.cells
                .iter()
                .map(|s| s.len())
                .chain(std::iter::once(c.header.len()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let last = cols.len().saturating_sub(1);
    let mut out = String::with_capacity(cols.len() * 16 * (cols[0].cells.len() + 2));

    for (i, c) in cols.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        if c.right {
            out.push_str(&format!("{:>w$}", c.header, w = widths[i]));
        } else if i == last {
            out.push_str(c.header);
        } else {
            out.push_str(&format!("{:<w$}", c.header, w = widths[i]));
        }
    }
    out.push('\n');

    for row in 0..cols[0].cells.len() {
        for (i, c) in cols.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            if c.right {
                out.push_str(&format!("{:>w$}", c.cells[row], w = widths[i]));
            } else if i == last {
                out.push_str(&c.cells[row]);
            } else {
                out.push_str(&format!("{:<w$}", c.cells[row], w = widths[i]));
            }
        }
        out.push('\n');
    }

    out
}

fn mem_cell(value: Option<u64>, args: &Args) -> String {
    match value {
        Some(v) => format_bytes(v, args.bytes),
        None => "-".to_string(),
    }
}

fn ports_cell(ports: &[u16]) -> String {
    if ports.is_empty() {
        "-".to_string()
    } else {
        ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn show_ports(args: &Args) -> bool {
    args.ports || !args.port.is_empty()
}

pub(crate) fn render_json(rows: &[Row], args: &Args) -> String {
    let procs: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let mut v = serde_json::json!({
                "pid": r.pid,
                "cmd": r.cmd,
                "args": r.args,
                "user": r.user,
                "rss_bytes": r.rss,
                "vms_bytes": r.virt,
                "cpu_percent": r.cpu,
                "threads": r.threads,
            });
            if args.pss {
                v["pss_bytes"] = serde_json::json!(r.pss);
                v["uss_bytes"] = serde_json::json!(r.uss);
                v["swap_bytes"] = serde_json::json!(r.swap);
            }
            if show_ports(args) {
                v["ports"] = serde_json::json!(r.ports);
            }
            v
        })
        .collect();

    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "processes": procs,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
}

pub(crate) fn render_groups_json(groups: &[Group], args: &Args, total_mem: u64) -> String {
    let apps: Vec<serde_json::Value> = groups
        .iter()
        .map(|g| {
            let total = if args.pss { g.pss.unwrap_or(0) } else { g.rss };
            let mut v = serde_json::json!({
                "name": g.name,
                "processes": g.count,
                "rss_bytes": g.rss,
                "vms_bytes": g.virt,
                "mem_percent": pct_of(total, total_mem),
            });
            if args.pss {
                v["pss_bytes"] = serde_json::json!(g.pss);
                v["unreadable"] = serde_json::json!(g.unreadable);
            }
            if show_ports(args) {
                v["ports"] = serde_json::json!(g.ports);
            }
            v
        })
        .collect();

    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "applications": apps,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
}

pub(crate) fn render_groups(groups: &[Group], args: &Args, total_mem: u64) -> String {
    let mem_header = if args.pss { "PSS" } else { "RSS" };

    let mem_values: Vec<Option<u64>> = groups
        .iter()
        .map(|g| if args.pss { g.pss } else { Some(g.rss) })
        .collect();
    let mem_cells: Vec<String> = mem_values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let mut s = mem_cell(*v, args);
            if args.pss && groups[i].unreadable > 0 {
                s.push('~');
            }
            s
        })
        .collect();
    let pct_s: Vec<String> = mem_values
        .iter()
        .map(|v| format!("{:.1}%", pct_of(v.unwrap_or(0), total_mem)))
        .collect();
    let cnt_s: Vec<String> = groups.iter().map(|g| g.count.to_string()).collect();

    let mut cols = vec![
        Col {
            header: "PROCESS",
            cells: groups.iter().map(|g| g.name.clone()).collect(),
            right: false,
        },
        Col {
            header: mem_header,
            cells: mem_cells,
            right: true,
        },
        Col {
            header: "%MEM",
            cells: pct_s,
            right: true,
        },
        Col {
            header: "PROCS",
            cells: cnt_s,
            right: true,
        },
    ];

    if show_ports(args) {
        cols.push(Col {
            header: "PORTS",
            cells: groups.iter().map(|g| ports_cell(&g.ports)).collect(),
            right: true,
        });
    }

    render_columns(&cols)
}

pub(crate) fn render_table(rows: &[Row], args: &Args) -> String {
    let pid_s: Vec<String> = rows.iter().map(|r| r.pid.to_string()).collect();
    let mem_values: Vec<Option<u64>> = rows
        .iter()
        .map(|r| if args.pss { r.pss } else { Some(r.rss) })
        .collect();
    let mem_s: Vec<String> = mem_values.iter().map(|v| mem_cell(*v, args)).collect();
    let virt_s: Vec<String> = rows
        .iter()
        .map(|r| format_bytes(r.virt, args.bytes))
        .collect();
    let cpu_s: Vec<String> = rows.iter().map(|r| format!("{:.1}", r.cpu)).collect();

    let mut cols = vec![
        Col {
            header: "PID",
            cells: pid_s,
            right: true,
        },
        Col {
            header: "USER",
            cells: rows.iter().map(|r| r.user.clone()).collect(),
            right: false,
        },
        Col {
            header: "MEM",
            cells: mem_s,
            right: true,
        },
        Col {
            header: "VIRT",
            cells: virt_s,
            right: true,
        },
    ];

    if args.pss {
        cols.push(Col {
            header: "USS",
            cells: rows.iter().map(|r| mem_cell(r.uss, args)).collect(),
            right: true,
        });
        cols.push(Col {
            header: "SWAP",
            cells: rows.iter().map(|r| mem_cell(r.swap, args)).collect(),
            right: true,
        });
    }

    cols.push(Col {
        header: "CPU%",
        cells: cpu_s,
        right: true,
    });

    if args.threads {
        cols.push(Col {
            header: "THR",
            cells: rows.iter().map(|r| r.threads.to_string()).collect(),
            right: true,
        });
    }

    if show_ports(args) {
        cols.push(Col {
            header: "PORTS",
            cells: rows.iter().map(|r| ports_cell(&r.ports)).collect(),
            right: true,
        });
    }

    cols.push(Col {
        header: "NAME",
        cells: rows.iter().map(|r| r.cmd.clone()).collect(),
        right: false,
    });

    render_columns(&cols)
}

/// Flat view rendered as an indented tree over `ppid`: each row shows own
/// memory and the subtree total, siblings sorted by subtree total. A parent
/// missing from the (filtered) row set makes its child a root; cycles
/// cannot occur in a real ppid graph but are guarded anyway.
pub(crate) fn render_tree(rows: &[Row], args: &Args) -> String {
    let (mut children, mut roots) = tree_structure(rows);
    let subtree = subtree_totals(rows, &children);

    for kids in children.values_mut() {
        kids.sort_by_key(|&i| Reverse(subtree[i]));
    }
    roots.sort_by_key(|&i| Reverse(subtree[i]));
    if let Some(n) = args.count {
        roots.truncate(n);
    }

    let mut ordered: Vec<(usize, usize)> = Vec::new(); // (row index, depth)
    let mut seen: HashSet<usize> = HashSet::new();
    for &root in &roots {
        push_subtree(root, 0, rows, &children, &mut seen, &mut ordered);
    }
    let ordered: Vec<(usize, usize)> = ordered
        .into_iter()
        .filter(|(i, _)| seen.contains(i))
        .collect();

    let pid_s: Vec<String> = ordered.iter().map(|&(i, _)| rows[i].pid.to_string()).collect();
    let mem_s: Vec<String> = ordered
        .iter()
        .map(|&(i, _)| format_bytes(rows[i].rss, args.bytes))
        .collect();
    let sub_s: Vec<String> = ordered
        .iter()
        .map(|&(i, _)| format_bytes(subtree[i], args.bytes))
        .collect();
    let name_s: Vec<String> = ordered
        .iter()
        .map(|&(i, depth)| {
            let indent = if depth > 0 {
                format!("{}└ ", "  ".repeat(depth - 1))
            } else {
                String::new()
            };
            format!("{indent}{}", rows[i].cmd)
        })
        .collect();

    let cols = vec![
        Col {
            header: "PID",
            cells: pid_s,
            right: true,
        },
        Col {
            header: "USER",
            cells: ordered.iter().map(|&(i, _)| rows[i].user.clone()).collect(),
            right: false,
        },
        Col {
            header: "MEM",
            cells: mem_s,
            right: true,
        },
        Col {
            header: "SUBTREE",
            cells: sub_s,
            right: true,
        },
        Col {
            header: "NAME",
            cells: name_s,
            right: false,
        },
    ];

    render_columns(&cols)
}

/// children map (ppid → row indices) and roots. A parent missing from the
/// (filtered) row set makes its child a root.
fn tree_structure(rows: &[Row]) -> (HashMap<u32, Vec<usize>>, Vec<usize>) {
    let pids: HashSet<u32> = rows.iter().map(|r| r.pid).collect();
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if r.ppid != 0 && pids.contains(&r.ppid) {
            children.entry(r.ppid).or_default().push(i);
        } else {
            roots.push(i);
        }
    }
    (children, roots)
}

/// Subtree memory totals = own rss + all descendants (cycles guarded).
fn subtree_totals(rows: &[Row], children: &HashMap<u32, Vec<usize>>) -> Vec<u64> {
    rows.iter()
        .enumerate()
        .map(|(i, _)| {
            let mut visited: HashSet<usize> = HashSet::new();
            subtree_of(i, rows, children, &mut visited)
        })
        .collect()
}

fn subtree_of(
    i: usize,
    rows: &[Row],
    children: &HashMap<u32, Vec<usize>>,
    visited: &mut HashSet<usize>,
) -> u64 {
    if !visited.insert(i) {
        return 0;
    }
    let mut total = rows[i].rss;
    if let Some(kids) = children.get(&rows[i].pid) {
        for &kid in kids {
            total += subtree_of(kid, rows, children, visited);
        }
    }
    total
}

fn push_subtree(
    i: usize,
    depth: usize,
    rows: &[Row],
    children: &HashMap<u32, Vec<usize>>,
    seen: &mut HashSet<usize>,
    out: &mut Vec<(usize, usize)>,
) {
    if !seen.insert(i) {
        return;
    }
    out.push((i, depth));
    if let Some(kids) = children.get(&rows[i].pid) {
        for &kid in kids {
            push_subtree(kid, depth + 1, rows, children, seen, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, ppid: u32, rss: u64) -> Row {
        Row {
            pid,
            user: "u".to_string(),
            rss,
            virt: 0,
            cpu: 0.0,
            threads: 1,
            cmd: format!("p{pid}"),
            args: format!("p{pid}"),
            ppid,
            start_time: 100,
            tty_nr: 0,
            pss: None,
            uss: None,
            swap: None,
            ports: Vec::new(),
        }
    }

    fn tree_args() -> Args {
        Args {
            count: None,
            sort: crate::cli::SortKey::Mem,
            reverse: false,
            user: None,
            pid: Vec::new(),
            min_mem: None,
            bytes: false,
            threads: false,
            group: false,
            group_by: crate::cli::GroupBy::Cmd,
            port: Vec::new(),
            ports: false,
            tree: true,
            pss: false,
            track: None,
            summary: false,
            json: false,
            watch: false,
            interval: 2.0,
            kill: false,
            command: None,
        }
    }

    #[test]
    fn subtree_totals_equal_own_plus_descendants() {
        // p1 (100) -> p2 (200) -> p3 (400); p9 (800) is a separate root
        let rows = vec![row(1, 999, 100), row(2, 1, 200), row(3, 2, 400), row(9, 999, 800)];
        let (children, _roots) = tree_structure(&rows);
        let sub = subtree_totals(&rows, &children);
        assert_eq!(sub[0], 700); // 100 + 200 + 400
        assert_eq!(sub[1], 600); // 200 + 400
        assert_eq!(sub[2], 400);
        assert_eq!(sub[3], 800);
    }

    #[test]
    fn subtree_cycle_guard_terminates() {
        // impossible in reality; the guard must terminate with finite totals
        let rows = vec![row(1, 2, 100), row(2, 1, 200)];
        let (children, _) = tree_structure(&rows);
        let sub = subtree_totals(&rows, &children);
        // each side counts itself plus the other once (no infinite loop)
        assert_eq!(sub[0], 300);
        assert_eq!(sub[1], 300);
    }

    #[test]
    fn render_tree_orders_siblings_by_subtree_total() {
        // two roots: p9 (subtree 800) before p1 (subtree 700)
        let rows = vec![row(1, 999, 100), row(2, 1, 200), row(3, 2, 400), row(9, 999, 800)];
        let out = render_tree(&rows, &tree_args());
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[1].ends_with("p9"));
        assert!(lines[2].ends_with("p1"));
        assert!(lines[3].ends_with("└ p2"));
        assert!(lines[4].ends_with("  └ p3"));
    }

    #[test]
    fn render_columns_alignment_and_widths() {
        let cols = vec![
            Col {
                header: "PID",
                cells: vec!["1".to_string(), "65535".to_string()],
                right: true,
            },
            Col {
                header: "NAME",
                cells: vec!["a".to_string(), "bb".to_string()],
                right: false,
            },
        ];
        let out = render_columns(&cols);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "  PID  NAME");
        assert_eq!(lines[1], "    1  a");
        assert_eq!(lines[2], "65535  bb");
    }

    #[test]
    fn render_columns_missing_value() {
        let cols = vec![Col {
            header: "MEM",
            cells: vec!["-".to_string(), "1.0 KiB".to_string()],
            right: true,
        }];
        let out = render_columns(&cols);
        assert_eq!(out.lines().next().unwrap(), "    MEM");
        assert_eq!(out.lines().nth(1).unwrap(), "      -");
    }
}
