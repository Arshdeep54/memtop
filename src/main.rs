mod cli;
mod format;
mod kill;
mod mem;
mod proc;
mod procfs;
mod render;
mod run;
mod track;
mod types;

use std::io::{self, Write};
use std::process::exit;
use std::thread;
use std::time::Duration;

use clap::Parser;
use cli::Args;
use format::{parse_duration, parse_size};
use kill::interactive_kill;
use mem::read_meminfo;
use proc::{aggregate, build_rows, collect_processes};
use render::{
    render_groups, render_groups_json, render_json, render_system_summary, render_table,
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
        None => {}
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
        track::track(&args, duration, args.json);
        return;
    }

    if args.watch && args.json {
        eprintln!("error: --watch and --json cannot be combined");
        exit(2);
    }
    if args.watch && args.group {
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
        print!("{}", render_system_summary(&mem, &args));
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

    if args.group {
        let groups = aggregate(&rows, &args, min_mem);
        if args.json {
            print!("{}", render_groups_json(&groups, &args, mem.total));
        } else {
            print!("{}", render_system_summary(&mem, &args));
            print!("{}", render_groups(&groups, &args, mem.total));
        }
    } else {
        let rows = build_rows(rows, &args, min_mem);
        if args.json {
            print!("{}", render_json(&rows, &args));
        } else {
            print!("{}", render_table(&rows, &args));
        }
    }
}

fn run_watch(args: &Args, min_mem: Option<u64>) {
    let mut stdout = io::stdout().lock();
    loop {
        let rows = build_rows(collect_processes(args), args, min_mem);
        let mut out = String::new();
        out.push_str("\x1b[2J\x1b[H");
        out.push_str(&render_table(&rows, args));
        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
        thread::sleep(Duration::from_secs_f64(args.interval));
    }
}
