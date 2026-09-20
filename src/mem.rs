use crate::types::MemInfo;

pub(crate) fn read_meminfo() -> MemInfo {
    let mut info = MemInfo {
        total: 0,
        available: 0,
        swap_total: 0,
        swap_free: 0,
    };

    let Ok(content) = std::fs::read_to_string("/proc/meminfo") else {
        return info;
    };

    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let (Some(key), Some(val_kib)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(val) = val_kib.parse::<u64>().ok() else {
            continue;
        };
        let bytes = val * 1024;
        match key {
            "MemTotal:" => info.total = bytes,
            "MemAvailable:" => info.available = bytes,
            "SwapTotal:" => info.swap_total = bytes,
            "SwapFree:" => info.swap_free = bytes,
            _ => {}
        }
    }

    info
}
