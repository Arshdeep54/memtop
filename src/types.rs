pub(crate) struct Row {
    pub(crate) pid: u32,
    pub(crate) user: String,
    pub(crate) rss: u64,
    pub(crate) virt: u64,
    pub(crate) cpu: f32,
    pub(crate) threads: usize,
    pub(crate) cmd: String,
    pub(crate) args: String,
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
