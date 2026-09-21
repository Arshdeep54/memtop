use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::{exit, Command};

use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, Clear, ClearType},
};

use crate::cli::Args;
use crate::format::{format_bytes, truncate};
use crate::proc::{build_rows, collect_processes};
use crate::procfs;
use crate::types::Row;

pub(crate) fn interactive_kill(args: &Args, min_mem: Option<u64>) {
    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("error: failed to enable raw mode: {e}");
        exit(1);
    }

    let mut rows = refresh_rows(args, min_mem);
    let mut selected: usize = 0;
    let mut start: usize = 0;
    let mut status: Option<String> = None;
    let mut tree_scope = false;
    let mut stdout = io::stdout();

    loop {
        let (term_w, term_h) = terminal::size().unwrap_or((80, 24));
        let visible = (term_h as usize).saturating_sub(7).max(1);

        let view = KillView {
            rows: &rows,
            selected,
            start,
            tree_scope,
            term_w,
            visible,
        };
        render_kill_screen(&mut stdout, &view, status.as_deref());
        status = None;

        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if selected > 0 {
                    selected -= 1;
                    if selected < start {
                        start = selected;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if selected + 1 < rows.len() {
                    selected += 1;
                    if selected >= start + visible {
                        start = selected + 1 - visible;
                    }
                }
            }
            KeyCode::PageUp => {
                selected = selected.saturating_sub(visible);
                if selected < start {
                    start = selected;
                }
            }
            KeyCode::PageDown => {
                selected = (selected + visible).min(rows.len().saturating_sub(1));
                if selected >= start + visible {
                    start = selected + 1 - visible;
                }
            }
            KeyCode::Home => {
                selected = 0;
                start = 0;
            }
            KeyCode::End => {
                selected = rows.len().saturating_sub(1);
                start = selected.saturating_sub(visible.saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(row) = rows.get(selected) {
                    let targets = if tree_scope {
                        let t = tree_targets(&rows, selected);
                        let view = KillView {
                            rows: &rows,
                            selected,
                            start,
                            tree_scope,
                            term_w,
                            visible,
                        };
                        if !confirm_tree_kill(&mut stdout, &view, t.len()) {
                            status = Some("tree kill cancelled".to_string());
                            continue;
                        }
                        t
                    } else {
                        vec![(row.pid, row.start_time)]
                    };
                    let sig = "TERM";
                    match kill_pids(&targets, sig) {
                        Ok(msg) => status = Some(msg),
                        Err(e) => status = Some(e),
                    }
                    rows = refresh_rows(args, min_mem);
                    clamp_view(rows.len(), &mut selected, &mut start, visible);
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(row) = rows.get(selected) {
                    let targets = if tree_scope {
                        let t = tree_targets(&rows, selected);
                        let view = KillView {
                            rows: &rows,
                            selected,
                            start,
                            tree_scope,
                            term_w,
                            visible,
                        };
                        if !confirm_tree_kill(&mut stdout, &view, t.len()) {
                            status = Some("tree kill cancelled".to_string());
                            continue;
                        }
                        t
                    } else {
                        vec![(row.pid, row.start_time)]
                    };
                    match kill_pids(&targets, "KILL") {
                        Ok(msg) => status = Some(msg),
                        Err(e) => status = Some(e),
                    }
                    rows = refresh_rows(args, min_mem);
                    clamp_view(rows.len(), &mut selected, &mut start, visible);
                }
            }
            KeyCode::Char('t') => {
                tree_scope = !tree_scope;
            }
            KeyCode::Char('r') => {
                rows = refresh_rows(args, min_mem);
                clamp_view(rows.len(), &mut selected, &mut start, visible);
            }
            KeyCode::Char('q') | KeyCode::Esc => break,
            _ => {}
        }
    }

    let _ = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0));
    let _ = terminal::disable_raw_mode();
}

fn refresh_rows(args: &Args, min_mem: Option<u64>) -> Vec<Row> {
    build_rows(collect_processes(args), args, min_mem)
}

/// The selected process and all its visible descendants, ordered leaves
/// first and parent last, so a parent cannot respawn a child mid-way.
fn tree_targets(rows: &[Row], root_idx: usize) -> Vec<(u32, u64)> {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        if i != root_idx {
            children.entry(r.ppid).or_default().push(i);
        }
    }

    let mut order = Vec::new();
    let mut stack = vec![root_idx];
    while let Some(i) = stack.pop() {
        order.push(i);
        if let Some(kids) = children.get(&rows[i].pid) {
            for &kid in kids {
                stack.push(kid);
            }
        }
    }
    // reverse of a preorder is parents-last
    order.reverse();
    order
        .into_iter()
        .map(|i| (rows[i].pid, rows[i].start_time))
        .collect()
}

/// Everything the kill screen needs to redraw itself.
struct KillView<'a> {
    rows: &'a [Row],
    selected: usize,
    start: usize,
    tree_scope: bool,
    term_w: u16,
    visible: usize,
}

/// Extra confirm step for tree kills only — the blast radius is larger.
fn confirm_tree_kill(stdout: &mut io::Stdout, view: &KillView, count: usize) -> bool {
    render_kill_screen(
        stdout,
        view,
        Some(&format!(
            "tree kill of {count} processes (SIGTERM/SIGKILL): confirm? y/n"
        )),
    );
    matches!(
        event::read(),
        Ok(Event::Key(k)) if k.code == KeyCode::Char('y')
    )
}

fn clamp_view(rows_len: usize, selected: &mut usize, start: &mut usize, visible: usize) {
    *selected = (*selected).min(rows_len.saturating_sub(1));
    *start = (*start).min(rows_len.saturating_sub(1));
    if *selected < *start {
        *start = *selected;
    }
    if *selected >= start.saturating_add(visible) {
        *start = selected.saturating_sub(visible.saturating_sub(1));
    }
}

fn render_kill_screen(stdout: &mut io::Stdout, view: &KillView, status: Option<&str>) {
    // KillView is all-Copy, so destructure into plain values
    let KillView {
        rows,
        selected,
        start,
        tree_scope,
        term_w,
        visible,
    } = *view;
    let _ = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0));

    let w = term_w as usize;

    let pid_s: Vec<String> = rows.iter().map(|r| r.pid.to_string()).collect();
    let mem_s: Vec<String> = rows.iter().map(|r| format_bytes(r.rss, false)).collect();

    let mut pid_w = "PID".len();
    let mut user_w = "USER".len();
    let mut mem_w = "MEM".len();
    for (i, r) in rows.iter().enumerate() {
        pid_w = pid_w.max(pid_s[i].len());
        user_w = user_w.max(r.user.len());
        mem_w = mem_w.max(mem_s[i].len());
    }

    let name_w = w.saturating_sub(pid_w + user_w + mem_w + 9).max(6);

    let mut out = String::with_capacity(w * (visible + 6));

    let scope = if tree_scope { "  [tree]" } else { "" };
    out.push_str(&format!(
        "\x1b[1mmemtop — kill mode\x1b[0m{scope}   {} processes\n",
        rows.len(),
    ));
    out.push_str(&format!(
        "  {:>pid_w$}  {:<user_w$}  {:>mem_w$}  {}\n",
        "PID", "USER", "MEM", "NAME",
        pid_w = pid_w,
        user_w = user_w,
        mem_w = mem_w,
    ));
    out.push_str(&"-".repeat(w.saturating_sub(2).min(120)));
    out.push('\n');

    let end = (start + visible).min(rows.len());
    for i in start..end {
        let r = &rows[i];
        let name = truncate(&r.cmd, name_w);
        let line = format!(
            "{} {:>pid_w$}  {:<user_w$}  {:>mem_w$}  {}",
            if i == selected { ">" } else { " " },
            pid_s[i], r.user, mem_s[i], name,
            pid_w = pid_w,
            user_w = user_w,
            mem_w = mem_w,
        );
        if i == selected {
            out.push_str(&format!("\x1b[7m{}\x1b[0m\n", line));
        } else {
            out.push_str(&line);
            out.push('\n');
        }
    }

    if let Some(r) = rows.get(selected) {
        out.push_str(&format!(
            "\n  \x1b[1mSelected:\x1b[0m {}  {}  ({})\n",
            r.pid,
            r.cmd,
            format_bytes(r.rss, false),
        ));
    } else {
        out.push_str("\n  (no processes)\n");
    }
    out.push_str(
        "  ↑/↓ move   Enter kill (TERM)   x force (KILL)   t tree scope   r refresh   q quit",
    );

    if let Some(s) = status {
        out.push_str(&format!("\n  {s}"));
    }

    let _ = stdout.write_all(out.replace('\n', "\r\n").as_bytes());
    let _ = stdout.flush();
}

/// Signal a set of (pid, start_time) targets with one `kill` call. Each
/// target's start_time is re-read immediately before signalling; a changed
/// start_time means the pid was reused and is skipped.
fn kill_pids(targets: &[(u32, u64)], signal: &str) -> Result<String, String> {
    let sig = signal.trim().to_ascii_uppercase();
    let sig = sig.strip_prefix("SIG").unwrap_or(&sig).to_string();

    let root = Path::new("/proc");
    let mut live: Vec<String> = Vec::new();
    let mut reused = 0;
    for (pid, start_time) in targets {
        match procfs::read_start_time(root, *pid) {
            Some(t) if t == *start_time => live.push(pid.to_string()),
            _ => reused += 1,
        }
    }

    if live.is_empty() {
        return Err("no targets left: pids exited or were reused".to_string());
    }

    let output = Command::new("kill")
        .arg(format!("-{sig}"))
        .args(&live)
        .output();

    let base = format!("signalled {} pid(s) with SIG{sig}", live.len());
    match output {
        Ok(o) if o.status.success() => Ok(if reused > 0 {
            format!("{base} ({reused} skipped: pid reused or gone)")
        } else {
            base
        }),
        Ok(o) => {
            let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
            if msg.is_empty() {
                Err(format!("kill failed (exit status {})", o.status))
            } else {
                Err(msg)
            }
        }
        Err(e) => Err(format!("failed to run `kill`: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, ppid: u32) -> Row {
        Row {
            pid,
            user: "u".to_string(),
            rss: 0,
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

    #[test]
    fn tree_targets_parent_last() {
        // rows: p1 root (idx 0) with children p2, p3; p2 has child p4
        let rows = vec![row(1, 0), row(2, 1), row(3, 1), row(4, 2)];
        let targets = tree_targets(&rows, 0);
        let pids: Vec<u32> = targets.iter().map(|(p, _)| *p).collect();
        assert_eq!(pids.len(), 4);
        // parent must come last so it cannot respawn children mid-way
        assert_eq!(*pids.last().unwrap(), 1);
        assert!(pids.contains(&2) && pids.contains(&3) && pids.contains(&4));
    }

    #[test]
    fn tree_targets_single() {
        let rows = vec![row(1, 0)];
        let targets = tree_targets(&rows, 0);
        assert_eq!(targets, vec![(1, 100)]);
    }

    #[test]
    fn kill_pids_skips_stale_start_times() {
        // a target whose start_time no longer matches /proc is treated as
        // reused and skipped; with no live targets left, this is an error
        let own_pid = std::process::id();
        let stale = vec![(own_pid, 12345_u64)];
        let err = kill_pids(&stale, "TERM").unwrap_err();
        assert!(err.contains("reused"), "got: {err}");
    }

    #[test]
    fn kill_pids_reports_already_exited() {
        // pid 4 (safely out of use almost everywhere) with any start_time:
        // the guard finds nothing live and reports instead of crashing
        let stale = vec![(4_u32, 1_u64)];
        let err = kill_pids(&stale, "KILL").unwrap_err();
        assert!(err.contains("no targets"), "got: {err}");
    }
}
