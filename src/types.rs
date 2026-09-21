pub(crate) struct Row {
    pub(crate) pid: u32,
    pub(crate) user: String,
    pub(crate) rss: u64,
    pub(crate) virt: u64,
    pub(crate) cpu: f32,
    pub(crate) threads: usize,
    pub(crate) cmd: String,
    pub(crate) args: String,
    /// parent pid from /proc/<pid>/stat
    #[allow(dead_code)] // consumed by --tree/--track/--orphans in later phases
    pub(crate) ppid: u32,
    /// clock ticks since boot (stat field 22), for PID-reuse safety
    #[allow(dead_code)] // consumed by --track/--orphans/kill in later phases
    pub(crate) start_time: u64,
    /// controlling terminal device number, 0 when none (stat field 7)
    #[allow(dead_code)] // consumed by --orphans in a later phase
    pub(crate) tty_nr: u64,
    /// proportional set size, only read when --pss is set
    pub(crate) pss: Option<u64>,
    /// unique set size (private clean + dirty), only read when --pss is set
    pub(crate) uss: Option<u64>,
    /// swap-backed memory, only read when --pss is set
    pub(crate) swap: Option<u64>,
    /// listening TCP ports, only scanned when --port/--ports is set
    pub(crate) ports: Vec<u16>,
}

pub(crate) struct Group {
    pub(crate) name: String,
    pub(crate) rss: u64,
    pub(crate) virt: u64,
    pub(crate) count: usize,
    /// sum of member PSS (members with unreadable PSS excluded), None = all unreadable
    pub(crate) pss: Option<u64>,
    /// members whose PSS could not be read
    pub(crate) unreadable: usize,
    /// union of member listening ports
    pub(crate) ports: Vec<u16>,
}

pub(crate) struct MemInfo {
    pub(crate) total: u64,
    pub(crate) available: u64,
    pub(crate) swap_total: u64,
    pub(crate) swap_free: u64,
}
