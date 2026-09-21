# memtop

A tiny Linux CLI that shows which processes are using the most memory — ordered
biggest-first, grouped by application, or as a flat per-process table. It also
has an interactive arrow-key mode to kill a selected process.

It reads `/proc` directly, so the numbers it shows are exactly what the kernel
reports — no `ps` parsing in between.

## Features

- **Grouped view** — collapse a multi-process app (browsers, databases, workers)
  into one row with combined memory and `%MEM`.
- **Flat view** — every process sorted by memory, CPU, PID, or name.
- **System summary** — total / used / available RAM and swap at a glance.
- **Interactive kill** — navigate with arrow keys and kill the selected process.
- **JSON output** — for scripts and automation.
- **Live watch mode** — auto-refreshing `top`-style view.

## Requirements

- Linux
- `kill` (for the interactive kill mode; present on essentially every distro)
- Rust (to build) — edition 2024, so Rust 1.85+

## Install

```bash
git clone https://github.com/Arshdeep54/memtop
cd memtop
cargo build --release
install -m 755 target/release/memtop ~/.local/bin/memtop
```

Make sure `~/.local/bin` is on your `PATH` (add it to your shell rc if not).

Or install directly from the repository:

```bash
cargo install --git https://github.com/Arshdeep54/memtop
```

## Usage

```bash
memtop                 # flat list, most memory first
memtop -g              # grouped by application, biggest hog first
memtop -g -c 10        # top 10 apps
memtop -S              # system RAM summary only
memtop -k              # interactive kill mode (arrow keys + Enter)
memtop run -- cargo build --release   # peak memory of a command
```

### `memtop run -- <cmd>`

Reports the peak memory a command and **all its descendants** reached — for
sizing CI machines, Docker limits, or test suites. The child inherits your
terminal, so its output is untouched; the report goes to stderr, and memtop
exits with the child's exit code (signals → `128 + signo`).

```bash
memtop run -- python3 -c "b=bytearray(300*1024*1024)"
# memtop run: peak 301.5 MiB across 1 processes (at 0.3s)
# wall: 1.0s   exit: 0
```

Sampled every 100 ms (`--interval-ms`). Known limits: spikes shorter than the
sampling interval are missed; double-forking daemons reparent to PID 1 and
leave the tracked tree; the peak is the sum of per-sample PSS, not a kernel
high-water mark; on Ctrl-C both memtop and the child die without a report.

### Options

| Flag | Description |
|------|-------------|
| `-c, --count <N>` | Number of processes/groups to show |
| `-s, --sort <KEY>` | Sort by `mem`, `virt`, `cpu`, `pid`, or `name` |
| `-r, --reverse` | Reverse the sort order |
| `-u, --user <NAME>` | Only show processes owned by this user |
| `-p, --pid <PID>` | Only show these PIDs (comma-separated, repeatable) |
| `-m, --min-mem <SIZE>` | Only show entries using at least this much (`512M`, `1G`) |
| `-g, --group` | Group processes by application |
| `--pss` | Show PSS/USS/swap (shared pages counted once) instead of RSS; slower, reads `smaps_rollup` |
| `-S, --summary` | Print only the memory summary |
| `-j, --json` | Output JSON |
| `-w, --watch` | Live-updating mode |
| `-i, --interval <SECS>` | Refresh interval for `--watch` (default 2) |
| `-t, --threads` | Add a thread-count column |
| `--bytes` | Show raw byte counts instead of human-readable units |

### Interactive kill mode (`-k`)

```text
      PID  USER          MEM  NAME
-----------------------------------
>  12345  cosign  800.5 MiB  zen
   67890  cosign  519.5 MiB  node …/expo

  Selected: 12345  zen  (800.5 MiB)
  ↑/↓ move   Enter kill (TERM)   x force (KILL)   r refresh   q quit
```

| Key | Action |
|-----|--------|
| `↑` / `↓` (or `j` / `k`) | Move selection |
| `Enter` / `Space` | Kill selected process (SIGTERM) |
| `x` / `Delete` | Force-kill selected process (SIGKILL) |
| `PgUp` / `PgDn` / `Home` / `End` | Page / jump |
| `r` | Refresh the list |
| `q` / `Esc` / `Ctrl-C` | Quit |

## Examples

```bash
memtop -S                 # how much RAM is free right now
memtop -g -c 10           # what's eating my RAM
memtop -g --pss           # grouped totals with shared pages counted once
memtop -m 500M            # only processes using at least 500 MiB
memtop -u alice -j        # alice's processes as JSON
memtop -k                 # find the hog and kill it
```

### Notes on `--pss`

RSS double-counts memory shared between processes, so grouped RSS totals are
inflated. PSS (proportional set size) counts each shared page once, spread
across the processes sharing it — `memtop -g --pss` gives an honest total.

`--pss` reads `/proc/<pid>/smaps_rollup`, which the kernel walks page tables
for: a full scan takes a few seconds on a busy desktop versus ~0.3 s for the
default scan. Processes owned by other users hide their `smaps_rollup`
without root; their PSS shows as `-` and they are excluded from PSS sums
(marked with `~` in grouped view), never silently counted as 0.

## License

[MIT](LICENSE)
