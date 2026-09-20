use clap::{Parser, ValueEnum};

/// Show running processes ordered by memory usage (wraps `ps` and /proc/meminfo).
#[derive(Parser)]
#[command(name = "memtop", version, about, long_about = None)]
pub(crate) struct Args {
    /// Number of processes (or groups with --group) to display.
    #[arg(short, long)]
    pub(crate) count: Option<usize>,

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

    /// Interactively select and kill a process.
    #[arg(short = 'k', long)]
    pub(crate) kill: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum SortKey {
    Mem,
    Virt,
    Cpu,
    Pid,
    Name,
}
