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
struct Col {
    header: &'static str,
    cells: Vec<String>,
    right: bool,
}

fn render_columns(cols: &[Col]) -> String {
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

    let cols = vec![
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

    cols.push(Col {
        header: "NAME",
        cells: rows.iter().map(|r| r.cmd.clone()).collect(),
        right: false,
    });

    render_columns(&cols)
}

#[cfg(test)]
mod tests {
    use super::*;

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
