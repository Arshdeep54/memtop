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
}

pub(crate) struct Group {
    pub(crate) name: String,
    pub(crate) rss: u64,
    pub(crate) virt: u64,
    pub(crate) count: usize,
}

pub(crate) struct MemInfo {
    pub(crate) total: u64,
    pub(crate) available: u64,
    pub(crate) swap_total: u64,
    pub(crate) swap_free: u64,
}
