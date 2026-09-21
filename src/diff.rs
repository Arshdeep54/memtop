use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use crate::cli::Args;
use crate::format::format_bytes;
use crate::proc::{aggregate, collect_processes};
use crate::render::{Col, render_columns};

/// `memtop diff <before> [after]`: what changed in memory between two
/// points in time. The snapshot format is the `memtop -g -j` output —
/// no separate snapshot subcommand needed.
pub(crate) fn diff(before: PathBuf, after: Option<PathBuf>, args: &Args) {
    let before = load_snapshot(&before);
    let after = match after {
        Some(path) => load_snapshot(&path),
        None => live_groups(args),
    };

    let mut rows = diff_rows(before, after);
    rows.sort_by(|a, b| {
        b.delta()
            .abs()
            .total_cmp(&a.delta().abs())
            .then_with(|| a.name.cmp(&b.name))
    });

    let cols = vec![
        Col {
            header: "NAME",
            cells: rows.iter().map(|r| r.name.clone()).collect(),
            right: false,
        },
        Col {
            header: "BEFORE",
            cells: rows
                .iter()
                .map(|r| r.before.map(|b| format_bytes(b, false)).unwrap_or_else(|| "-".into()))
                .collect(),
            right: true,
        },
        Col {
            header: "AFTER",
            cells: rows
                .iter()
                .map(|r| r.after.map(|a| format_bytes(a, false)).unwrap_or_else(|| "-".into()))
                .collect(),
            right: true,
        },
        Col {
            header: "DELTA",
            cells: rows.iter().map(delta_cell).collect(),
            right: true,
        },
        Col {
            header: "DELTA%",
            cells: rows.iter().map(pct_cell).collect(),
            right: true,
        },
        Col {
            header: "NOTE",
            cells: rows
                .iter()
                .map(|r| match (r.before, r.after) {
                    (None, Some(_)) => "new".to_string(),
                    (Some(_), None) => "gone".to_string(),
                    _ => String::new(),
                })
                .collect(),
            right: false,
        },
    ];

    print!("{}", render_columns(&cols));
}

struct DiffRow {
    name: String,
    before: Option<u64>,
    after: Option<u64>,
}

impl DiffRow {
    fn delta(&self) -> f64 {
        self.after.unwrap_or(0) as f64 - self.before.unwrap_or(0) as f64
    }
}

fn delta_cell(r: &DiffRow) -> String {
    let d = r.delta();
    let sign = if d > 0.0 { "+" } else { "" };
    format!("{sign}{:.1} MiB", d / 1024.0 / 1024.0)
}

fn pct_cell(r: &DiffRow) -> String {
    match r.before {
        Some(0) | None => "-".to_string(),
        Some(b) => {
            let pct = r.delta() / b as f64 * 100.0;
            let sign = if pct > 0.0 { "+" } else { "" };
            format!("{sign}{:.0}%", pct)
        }
    }
}

/// Compare two name → bytes snapshots; names present in only one side
/// become new/gone rows.
fn diff_rows(
    before: Vec<(String, u64)>,
    after: Vec<(String, u64)>,
) -> Vec<DiffRow> {
    let before_map: HashMap<String, u64> = before.into_iter().collect();
    let after_map: HashMap<String, u64> = after.into_iter().collect();

    let mut names: Vec<String> = before_map
        .keys()
        .chain(after_map.keys())
        .cloned()
        .collect();
    names.sort_unstable();
    names.dedup();

    names
        .into_iter()
        .map(|name| DiffRow {
            before: before_map.get(&name).copied(),
            after: after_map.get(&name).copied(),
            name,
        })
        .collect()
}

/// A snapshot is the `applications` array of `memtop -g -j` output.
fn load_snapshot(path: &Path) -> Vec<(String, u64)> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        exit(2);
    });
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("error: {} is not valid JSON: {e}", path.display());
        exit(2);
    });
    parse_snapshot(&value).unwrap_or_else(|| {
        eprintln!(
            "error: {} is not a memtop -g -j snapshot (expected an \"applications\" array with name/rss_bytes)",
            path.display()
        );
        exit(2);
    })
}

fn parse_snapshot(value: &serde_json::Value) -> Option<Vec<(String, u64)>> {
    let apps = value.get("applications")?.as_array()?;
    let mut out = Vec::with_capacity(apps.len());
    for app in apps {
        let name = app.get("name")?.as_str()?.to_string();
        let rss = app.get("rss_bytes")?.as_u64()?;
        out.push((name, rss));
    }
    Some(out)
}

/// No `after` file: take the group snapshot live.
fn live_groups(args: &Args) -> Vec<(String, u64)> {
    let rows = collect_processes(args);
    // snapshots must be complete; the top-10 default is a table-UX thing
    aggregate(&rows, args, None, crate::proc::effective_count(args, false))
        .into_iter()
        .map(|g| (g.name, g.rss))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_json() -> serde_json::Value {
        serde_json::json!({
            "applications": [
                {"name": "zen", "processes": 2, "rss_bytes": 1000, "vms_bytes": 2000, "mem_percent": 1.0},
                {"name": "node", "processes": 1, "rss_bytes": 500, "vms_bytes": 900, "mem_percent": 0.5}
            ]
        })
    }

    #[test]
    fn parse_snapshot_valid() {
        let s = parse_snapshot(&snapshot_json()).unwrap();
        assert_eq!(s, vec![("zen".to_string(), 1000), ("node".to_string(), 500)]);
    }

    #[test]
    fn parse_snapshot_wrong_shapes() {
        assert!(parse_snapshot(&serde_json::json!({})).is_none());
        assert!(parse_snapshot(&serde_json::json!([])).is_none());
        assert!(parse_snapshot(&serde_json::json!({"applications": [{}]})).is_none());
        assert!(parse_snapshot(&serde_json::json!({"applications": [{"name": 5}]})).is_none());
    }

    #[test]
    fn diff_rows_new_gone_and_changed() {
        let before = vec![("zen".to_string(), 1000), ("gone-app".to_string(), 400)];
        let after = vec![("zen".to_string(), 1500), ("new-app".to_string(), 200)];
        let mut rows = diff_rows(before, after);
        rows.sort_by(|a, b| a.name.cmp(&b.name));

        let zen = &rows[2];
        assert_eq!(zen.name, "zen");
        assert_eq!(zen.delta(), 500.0);

        let new_app = &rows[1];
        assert_eq!(new_app.name, "new-app");
        assert_eq!(new_app.before, None);
        assert_eq!(new_app.delta(), 200.0);

        let gone = &rows[0];
        assert_eq!(gone.name, "gone-app");
        assert_eq!(gone.after, None);
        assert_eq!(gone.delta(), -400.0);
    }

    #[test]
    fn delta_pct_guard_for_zero_before() {
        let r = DiffRow {
            name: "x".into(),
            before: Some(0),
            after: Some(100),
        };
        assert_eq!(pct_cell(&r), "-"); // division by zero avoided
    }
}
