# memtop

A tiny Linux CLI that shows which processes are using the most memory. Orders by biggest-first, groups by application, or shows a flat per-process table. Interactive kill mode with arrow keys.

Reads `/proc` directly, so numbers are exactly what the kernel reports — no `ps` parsing.

## Quick Start

```bash
memtop              # top 10 processes by memory
memtop -g           # top 10 apps (grouped)
memtop -k           # interactive kill mode
memtop -w           # live watch (like top)
memtop run -- cargo build  # peak memory of a command
```

## Requirements

- **Linux** only (reads `/proc`)
- `kill` command (standard on all distros)

## Install

```bash
curl https://memtop.hiesenbug.dev/install.sh | bash
```

Or `cargo install --git https://github.com/Arshdeep54/memtop`

Or build: `git clone ... && cd memtop && cargo build --release`

### Update

```bash
memtop update --check   # is a newer release available?
memtop update           # replace this binary with the latest release (restarts a running guard)
```

`update` needs `curl` and downloads the pre-built release binary for your CPU (x86_64 or aarch64). If you installed an older version that has no `update` command, re-run the install one-liner once.

## Modes

### Default views

**Flat** (process-by-process, sorted by memory)
```bash
memtop              # top 10 biggest
memtop --all        # everything
memtop --asc        # least memory first
memtop -s cpu       # sort by CPU instead
```

**Grouped** (app-by-app, summed memory)
```bash
memtop -g           # top 10 apps
memtop -g --pss     # honest totals (shared pages counted once)
memtop -g --group-by project  # group by git project
memtop -g --group-by cgroup   # group by container/systemd unit
```

**Tree** (parent-child hierarchy)
```bash
memtop --tree       # who spawned whom
```

### Interactive kill mode

```bash
memtop -k
```

Navigate with `↑/↓` (or `j`/`k`). Press `Enter` to kill (SIGTERM), `x` to force-kill (SIGKILL), `r` to refresh, `q` to quit.

### Live watch

```bash
memtop -w           # auto-refresh every 2s
memtop -w -i 5      # auto-refresh every 5s
```

### Memory profiling

```bash
memtop run -- cargo build --release
```

Reports peak memory of a command and all its descendants. Useful for sizing CI machines, Docker limits, test suites.

### Snapshots & diff

```bash
memtop -g -j > before.json
# ... do something ...
memtop diff before.json          # vs. live scan
memtop diff before.json after.json  # two snapshots
```

### Orphan finder

```bash
memtop --orphans    # stale dev processes from dead sessions
```

Finds background processes (node, vite, jest, etc.) that lost their controlling terminal and are older than 10 minutes. Experimental heuristic.

### Guard (kill apps before the system chokes)

`memtop guard` polls `/proc/meminfo` and `/proc/pressure/memory` and, when memory is about to run out, kills the apps you list, one app per trigger, in order (SIGTERM, then SIGKILL after `grace_secs`). All processes of an app are killed, e.g. every `zen-bin` content process.

After a kill it sends a desktop notification (needs `notify-send`; skipped silently if missing). At startup it logs a note for each configured app that isn't running, which catches wrong process names. The service runs with a high CPU weight and `MemoryLow=64M` so it keeps running while the system is thrashing.

Trigger: available RAM below `mem_available_below_pct` **and** (swap used above `swap_used_above_pct` **or** PSI `full avg10` above `psi_full_avg10_above`). Low RAM alone is normal cache behaviour, so it never fires on its own.

`~/.config/memtop/guard.toml`:
```toml
mem_available_below_pct = 8
swap_used_above_pct = 85
psi_full_avg10_above = 5
interval_secs = 1
cooldown_secs = 5   # pause after a kill so memory can be freed
grace_secs = 3      # TERM -> KILL delay
apps = ["zen", "chrome", "slack"]   # matches the executable name: zen, zen-bin, zen_x, not frozen
```

```bash
memtop guard --dry-run   # log what would be killed
memtop guard --once      # check once and exit
```

Run it in the background (systemd user service, starts at every login):
```bash
memtop guard start    # first run creates the config template, edit `apps`, run it again
memtop guard status   # running? shows recent log lines
memtop guard stop     # stop now and disable at login
journalctl --user -u memtop-guard   # what it killed and when
```

## Options

| Flag | Description |
|------|-------------|
| `-c, --count <N>` | Show first N (default 10; `--all` for everything) |
| `--asc` | Ascending — least memory first |
| `-s, --sort <KEY>` | Sort by: `mem`, `virt`, `cpu`, `pid`, `name` |
| `-u, --user <NAME>` | Filter by user |
| `-p, --pid <PID>` | Filter by PID (comma-separated, repeatable) |
| `-m, --min-mem <SIZE>` | Filter by minimum (`512M`, `1G`) |
| `-g, --group` | Group by application |
| `--group-by <KEY>` | Group by: `cmd`, `project` (`.git`), or `cgroup` |
| `--port <PORT>` | Show processes on port (implies `--ports`) |
| `--ports` | Show PORTS column |
| `--tree` | Show process tree (parent-child) |
| `--orphans` | Show stale dev processes (experimental) |
| `--min-age <DURATION>` | Min age for `--orphans` (default `10m`) |
| `--pss` | Use PSS instead of RSS (slower, more accurate) |
| `-S, --summary` | Show only RAM/swap summary |
| `-j, --json` | Output JSON |
| `-w, --watch` | Live-updating mode |
| `-i, --interval <SECS>` | Watch refresh (default 2s) |
| `-t, --threads` | Add thread-count column |
| `--bytes` | Raw bytes instead of human-readable |
| `--oom` | Show OOM score columns |
| `--oom-log` | Show kernel OOM kill log |

## Advanced

### PSS vs RSS

RSS double-counts shared memory, so grouped totals are inflated. PSS counts each shared page once:

```bash
memtop -g --pss         # accurate grouped totals
```

Trade-off: PSS is slower (reads `/proc/<pid>/smaps_rollup`), ~1s vs 0.3s. Requires read access to other users' smaps (usually needs root).

### Port filtering

```bash
memtop --port 3000      # what's listening on :3000?
memtop -k --port 3000   # and kill it interactively
```

Scans `/proc/net/tcp*` and each process's FDs. Processes of other users show as `-` without root.

### Memory profiling

```bash
memtop run -- cargo build --release
# peak 512.3 MiB across 5 processes (at 2.4s)
```

Samples every 100ms. Known limits: misses spikes <100ms, reports sum of per-sample PSS (not kernel high-water), double-forking daemons reparent to PID 1 and leave the tree.

### Snapshots and diff

Compare memory before/after:

```bash
memtop -g -j > before.json
# ... do something ...
memtop diff before.json          # vs live scan
memtop diff before.json after.json  # two files
```

Rows sorted by delta, marked `new`/`gone`. JSON always contains **complete** list unless `-c N` specified.

## License

[MIT](LICENSE)
