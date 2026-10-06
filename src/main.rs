mod cli;
mod diff;
mod format;
mod guard;
mod kill;
mod mem;
mod net;
mod proc;
mod procfs;
mod render;
mod run;
mod track;
mod types;

use std::io::{self, Write};
use std::path::Path;
use std::process::exit;
use std::thread;
use std::time::Duration;

use clap::Parser;
use cli::{Args, GroupBy};
use format::{format_bytes, parse_duration, parse_size};
use kill::interactive_kill;
use mem::read_meminfo;
use proc::{aggregate, build_rows, collect_processes, effective_count};
use render::{
    render_columns, render_groups, render_groups_json, render_json, render_system_summary,
    render_table, render_tree, Col, SystemExtras,
};

fn main() {
    let mut args = Args::parse();

    // subcommands own the whole flow; flags never mix with them
    match args.command.take() {
        Some(cli::Command::Run {
            json,
            interval_ms,
            cmd,
        }) => {
            run::run(json, interval_ms, cmd);
            return;
        }
        Some(cli::Command::Diff { before, after }) => {
            diff::diff(before, after, &args);
            return;
        }
        Some(cli::Command::Guard {
            config,
            dry_run,
            once,
        }) => {
            guard::guard(&args, config, dry_run, once);
            return;
        }
        None => {}
    }

    if args.oom_log {
        match mem::oom_log() {
            Ok(events) if events.is_empty() => println!("no OOM kills found"),
            Ok(events) => {
                let cols = vec![
                    Col {
                        header: "TIME",
                        cells: events.iter().map(|e| e.time.clone()).collect(),
                        right: false,
                    },
                    Col {
                        header: "PID",
                        cells: events.iter().map(|e| e.pid.to_string()).collect(),
                        right: true,
                    },
                    Col {
                        header: "NAME",
                        cells: events.iter().map(|e| e.name.clone()).collect(),
                        right: false,
                    },
                    Col {
                        header: "RSS",
                        cells: events
                            .iter()
                            .map(|e| {
                                e.rss
                                    .map(|b| format_bytes(b, false))
                                    .unwrap_or_else(|| "-".to_string())
                            })
                            .collect(),
                        right: true,
                    },
                ];
                print!("{}", render_columns(&cols));
            }
            Err(e) => {
                eprintln!("error: {e}");
                exit(1);
            }
        }
        return;
    }

    if let Some(track_spec) = args.track.clone() {
        if args.watch || args.kill || args.summary {
            eprintln!("error: --track cannot be combined with --watch, --kill, or --summary");
            exit(2);
        }
        let duration = match parse_duration(&track_spec) {
            Ok(d) if d > 0.0 => d,
            Ok(_) => {
                eprintln!("error: --track duration must be positive");
                exit(2);
            }
            Err(e) => {
                eprintln!("error: invalid --track value '{track_spec}': {e}");
                exit(2);
            }
        };
        // growth ranking is where accurate numbers are the point
        args.pss = true;
        let cap = effective_count(&args, !args.json);
        track::track(&args, duration, args.json, cap);
        return;
    }

    if args.watch && args.json {
        eprintln!("error: --watch and --json cannot be combined");
        exit(2);
    }
    let grouped = args.group || !matches!(args.group_by, GroupBy::Cmd);

    if args.tree && (args.json || grouped || args.watch) {
        eprintln!("error: --tree cannot be combined with --json, --group, or --watch");
        exit(2);
    }

    if args.watch && grouped {
        eprintln!("error: --watch and --group cannot be combined");
        exit(2);
    }

    let min_mem = args.min_mem.as_deref().map(|s| {
        parse_size(s).unwrap_or_else(|e| {
            eprintln!("error: invalid --min-mem value '{s}': {e}");
            exit(2);
        })
    });

    if args.watch {
        run_watch(&args, min_mem);
        return;
    }

    if args.kill {
        interactive_kill(&args, min_mem);
        return;
    }

    let mem = read_meminfo();

    if args.summary {
        // swap rate costs a 1 s sample, so only when swap is actually in use
        let extras = SystemExtras {
            pressure: mem::read_pressure(Path::new("/proc")),
            swap_rate: if mem.swap_total > mem.swap_free {
                mem::swap_rates()
            } else {
                None
            },
        };
        print!("{}", render_system_summary(&mem, &args, &extras));
        return;
    }

    let rows = collect_processes(&args);

    if args.pss {
        let unreadable = rows.iter().filter(|r| r.pss.is_none()).count();
        if unreadable > 0 {
            eprintln!(
                "note: PSS unavailable for {unreadable} processes, run as root for full data"
            );
        }
    }

    if grouped {
        let cap = effective_count(&args, !args.json);
        let groups = aggregate(&rows, &args, min_mem, cap);
        if args.json {
            print!("{}", render_groups_json(&groups, &args, mem.total));
        } else {
            let extras = SystemExtras {
                pressure: mem::read_pressure(Path::new("/proc")),
                swap_rate: None,
            };
            print!("{}", render_system_summary(&mem, &args, &extras));
            print!("{}", render_groups(&groups, &args, mem.total));
        }
    } else {
        if args.tree {
            // --sort/--count semantics change: tree orders by subtree total,
            // the cap applies to top-level roots, so filter but don't sort
            let mut rows = rows;
            if let Some(min) = min_mem {
                rows.retain(|r| r.rss >= min);
            }
            let cap = effective_count(&args, true);
            print!("{}", render_tree(&rows, &args, cap));
            return;
        }
        let cap = effective_count(&args, !args.json);
        let rows = build_rows(rows, &args, min_mem, cap);
        if args.json {
            print!("{}", render_json(&rows, &args));
        } else {
            let extras = SystemExtras {
                pressure: mem::read_pressure(Path::new("/proc")),
                swap_rate: None,
            };
            print!("{}", render_system_summary(&mem, &args, &extras));
            print!("{}", render_table(&rows, &args));
        }
    }
}

fn run_watch(args: &Args, min_mem: Option<u64>) {
    let mut stdout = io::stdout().lock();
    let cap = effective_count(args, true);
    loop {
        let rows = build_rows(collect_processes(args), args, min_mem, cap);
        let mut out = String::new();
        out.push_str("\x1b[2J\x1b[H");
        out.push_str(&render_table(&rows, args));
        if let Err(e) = stdout.write_all(out.as_bytes()) {
            eprintln!("error: write failed, exiting watch: {e}");
            exit(1);
        }
        if let Err(e) = stdout.flush() {
            eprintln!("error: flush failed, exiting watch: {e}");
            exit(1);
        }
        thread::sleep(Duration::from_secs_f64(args.interval));
    }
}
