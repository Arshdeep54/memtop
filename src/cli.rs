use clap::{Parser, Subcommand, ValueEnum};

/// Show running processes ordered by memory usage.
#[derive(Parser)]
#[command(
    name = "memtop",
    version,
    about,
    long_about = None,
    args_conflicts_with_subcommands = true
)]
pub(crate) struct Args {
    /// Show the first N processes/groups (tables default to 10; JSON and
    /// kill mode stay complete unless this is set explicitly).
    #[arg(short, long, alias = "list", value_name = "N")]
    pub(crate) count: Option<usize>,

    /// Show every process/group instead of the default top 10.
    #[arg(long, conflicts_with = "count")]
    pub(crate) all: bool,

    /// Sort ascending (least memory first) instead of biggest-first.
    #[arg(long)]
    pub(crate) asc: bool,

    /// Column to sort by (flat mode only).
    #[arg(short, long, value_enum, default_value_t = SortKey::Mem)]
    pub(crate) sort: SortKey,

    /// Reverse the sort order.
    #[arg(short, long)]
    pub(crate) reverse: bool,

    /// Only show processes owned by this username.
    #[arg(short, long)]
    pub(crate) user: Option<String>,

    /// Only show these PIDs (comma-separated, repeatable).
    #[arg(short, long, value_delimiter = ',')]
    pub(crate) pid: Vec<u32>,

    /// Only show entries using at least this much memory
    /// (accepts sizes like "512M", "1G", or a raw byte count).
    #[arg(short = 'm', long)]
    pub(crate) min_mem: Option<String>,

    /// Show raw byte counts instead of human-readable units.
    #[arg(long)]
    pub(crate) bytes: bool,

    /// Include a thread-count column.
    #[arg(short = 't', long)]
    pub(crate) threads: bool,

    /// Group processes by application and show combined memory.
    #[arg(short, long)]
    pub(crate) group: bool,

    /// Grouping key for grouped view (implies --group).
    #[arg(long, value_enum, default_value_t = GroupBy::Cmd)]
    pub(crate) group_by: GroupBy,

    /// Print only the system memory summary.
    #[arg(short = 'S', long)]
    pub(crate) summary: bool,

    /// Output as JSON.
    #[arg(short, long)]
    pub(crate) json: bool,

    /// Live-updating mode (like top).
    #[arg(short, long)]
    pub(crate) watch: bool,

    /// Refresh interval in seconds for --watch.
    #[arg(short, long, default_value_t = 2.0)]
    pub(crate) interval: f64,

    /// Show PSS/USS/swap instead of RSS: reads /proc/<pid>/smaps_rollup,
    /// which the kernel walks page tables for (slower than status).
    #[arg(long)]
    pub(crate) pss: bool,

    /// Only show processes owning a listening TCP port (comma-separated,
    /// repeatable). Implies the PORTS column.
    #[arg(long, value_delimiter = ',')]
    pub(crate) port: Vec<u16>,

    /// Show the PORTS column without filtering.
    #[arg(long)]
    pub(crate) ports: bool,

    /// Render the flat list as an indented process tree with subtree
    /// memory totals (implies no --sort/--json).
    #[arg(long)]
    pub(crate) tree: bool,

    /// EXPERIMENTAL: only show likely stale dev processes (reparented,
    /// no tty, dev-tool command, older than --min-age). Same user only.
    #[arg(long)]
    pub(crate) orphans: bool,

    /// Minimum age for --orphans candidates (e.g. 10m, 2h; default 10m).
    #[arg(long, value_name = "DURATION", default_value = "10m")]
    pub(crate) min_age: String,

    /// Include oom_score/oom_score_adj columns (who dies first).
    #[arg(long)]
    pub(crate) oom: bool,

    /// Print kernel-log OOM kills (journalctl -k, fallback dmesg).
    #[arg(long)]
    pub(crate) oom_log: bool,

    /// Track memory growth for this duration (e.g. 30s, 5m) and rank by
    /// growth rate instead of current size.
    #[arg(long, value_name = "DURATION")]
    pub(crate) track: Option<String>,

    /// Interactively select and kill a process.
    #[arg(short = 'k', long)]
    pub(crate) kill: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Run a command and report the peak memory it and its descendants reached
    Run {
        /// Print the report as a single JSON object (to stderr)
        #[arg(long)]
        json: bool,

        /// Sampling interval in milliseconds
        #[arg(long, default_value_t = 100)]
        interval_ms: u64,

        /// The command to run (everything after `--`)
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },

    /// Diff two group snapshots (`memtop -g -j` output); without a second
    /// file, compares against a live scan
    Diff {
        /// Snapshot file taken before
        before: std::path::PathBuf,

        /// Snapshot file taken after (omit to scan now)
        after: Option<std::path::PathBuf>,
    },

    /// Watch memory and swap; when the system is about to choke, kill the
    /// configured apps (see ~/.config/memtop/guard.toml)
    Guard {
        /// Config file (default: ~/.config/memtop/guard.toml)
        #[arg(long)]
        config: Option<std::path::PathBuf>,

        /// Log what would be killed instead of killing it
        #[arg(long)]
        dry_run: bool,

        /// Check once and exit instead of looping
        #[arg(long)]
        once: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum GroupBy {
    Cmd,
    Project,
    Cgroup,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum SortKey {
    Mem,
    Virt,
    Cpu,
    Pid,
    Name,
}
