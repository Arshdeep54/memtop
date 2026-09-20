use std::io::{self, Write};
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
    let mut stdout = io::stdout();

    loop {
        let (term_w, term_h) = terminal::size().unwrap_or((80, 24));
        let visible = (term_h as usize).saturating_sub(7).max(1);

        render_kill_screen(&mut stdout, &rows, selected, start, status.as_deref(), term_w, visible);
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
                    let pid = row.pid;
                    let name = row.cmd.clone();
                    match kill_process(pid, "TERM") {
                        Ok(()) => status = Some(format!("killed {pid} ({name})")),
                        Err(e) => status = Some(e),
                    }
                    rows = refresh_rows(args, min_mem);
                    clamp_view(rows.len(), &mut selected, &mut start, visible);
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(row) = rows.get(selected) {
                    let pid = row.pid;
                    let name = row.cmd.clone();
                    match kill_process(pid, "KILL") {
                        Ok(()) => status = Some(format!("killed {pid} ({name}) with SIGKILL")),
                        Err(e) => status = Some(e),
                    }
                    rows = refresh_rows(args, min_mem);
                    clamp_view(rows.len(), &mut selected, &mut start, visible);
                }
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

fn render_kill_screen(
    stdout: &mut io::Stdout,
    rows: &[Row],
    selected: usize,
    start: usize,
    status: Option<&str>,
    term_w: u16,
    visible: usize,
) {
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

    out.push_str(&format!(
        "\x1b[1mmemtop — kill mode\x1b[0m   {} processes\n",
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
    out.push_str("  ↑/↓ move   Enter kill (TERM)   x force (KILL)   r refresh   q quit");

    if let Some(s) = status {
        out.push_str(&format!("\n  {s}"));
    }

    let _ = stdout.write_all(out.replace('\n', "\r\n").as_bytes());
    let _ = stdout.flush();
}

fn kill_process(pid: u32, signal: &str) -> Result<(), String> {
    let sig = signal.trim().to_ascii_uppercase();
    let sig = sig.strip_prefix("SIG").unwrap_or(&sig);

    let output = Command::new("kill")
        .args([format!("-{sig}"), pid.to_string()])
        .output();

    match output {
        Ok(o) if o.status.success() => Ok(()),
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
