use crate::cli::Args;
use crate::format::{format_bytes, pct_of};
use crate::types::{Group, MemInfo, Row};

pub(crate) fn render_system_summary(mem: &MemInfo, args: &Args) -> String {
    let used = mem.total.saturating_sub(mem.available);

    let mut out = format!(
        "Total: {}   Used: {} ({:.1}%)   Available: {}\n",
        format_bytes(mem.total, args.bytes),
        format_bytes(used, args.bytes),
        pct_of(used, mem.total),
        format_bytes(mem.available, args.bytes),
    );

    if mem.swap_total > 0 {
        out.push_str(&format!(
            "Swap:  {} used / {} total\n",
            format_bytes(mem.swap_total - mem.swap_free, args.bytes),
            format_bytes(mem.swap_total, args.bytes),
        ));
    }

    out
}

pub(crate) fn render_json(rows: &[Row]) -> String {
    let procs: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "pid": r.pid,
                "cmd": r.cmd,
                "args": r.args,
                "user": r.user,
                "rss_bytes": r.rss,
                "vms_bytes": r.virt,
                "cpu_percent": r.cpu,
                "threads": r.threads,
            })
        })
        .collect();

    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "processes": procs,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
}

pub(crate) fn render_groups_json(groups: &[Group], total_mem: u64) -> String {
    let apps: Vec<serde_json::Value> = groups
        .iter()
        .map(|g| {
            serde_json::json!({
                "name": g.name,
                "processes": g.count,
                "rss_bytes": g.rss,
                "vms_bytes": g.virt,
                "mem_percent": pct_of(g.rss, total_mem),
            })
        })
        .collect();

    let mut out = serde_json::to_string_pretty(&serde_json::json!({
        "applications": apps,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
}

pub(crate) fn render_groups(groups: &[Group], args: &Args, total_mem: u64) -> String {
    const NAME_H: &str = "PROCESS";
    const RSS_H: &str = "RSS";
    const PCT_H: &str = "%MEM";
    const CNT_H: &str = "PROCS";

    let rss_s: Vec<String> = groups
        .iter()
        .map(|g| format_bytes(g.rss, args.bytes))
        .collect();
    let pct_s: Vec<String> = groups
        .iter()
        .map(|g| format!("{:.1}%", pct_of(g.rss, total_mem)))
        .collect();
    let cnt_s: Vec<String> = groups.iter().map(|g| g.count.to_string()).collect();

    let mut name_w = NAME_H.len();
    let mut rss_w = RSS_H.len();
    let mut pct_w = PCT_H.len();
    let mut cnt_w = CNT_H.len();

    for (i, g) in groups.iter().enumerate() {
        name_w = name_w.max(g.name.len());
        rss_w = rss_w.max(rss_s[i].len());
        pct_w = pct_w.max(pct_s[i].len());
        cnt_w = cnt_w.max(cnt_s[i].len());
    }

    let mut out = String::with_capacity(groups.len() * (name_w + 32));

    out.push_str(&format!(
        "{:<name_w$}  {:>rss_w$}  {:>pct_w$}  {:>cnt_w$}\n",
        NAME_H, RSS_H, PCT_H, CNT_H,
        name_w = name_w,
        rss_w = rss_w,
        pct_w = pct_w,
        cnt_w = cnt_w,
    ));

    for (i, g) in groups.iter().enumerate() {
        out.push_str(&format!(
            "{:<name_w$}  {:>rss_w$}  {:>pct_w$}  {:>cnt_w$}\n",
            g.name, rss_s[i], pct_s[i], cnt_s[i],
            name_w = name_w,
            rss_w = rss_w,
            pct_w = pct_w,
            cnt_w = cnt_w,
        ));
    }

    out
}

pub(crate) fn render_table(rows: &[Row], args: &Args) -> String {
    const PID_H: &str = "PID";
    const USER_H: &str = "USER";
    const MEM_H: &str = "MEM";
    const VIRT_H: &str = "VIRT";
    const CPU_H: &str = "CPU%";
    const THR_H: &str = "THR";
    const NAME_H: &str = "NAME";

    let pid_s: Vec<String> = rows.iter().map(|r| r.pid.to_string()).collect();
    let rss_s: Vec<String> = rows
        .iter()
        .map(|r| format_bytes(r.rss, args.bytes))
        .collect();
    let virt_s: Vec<String> = rows
        .iter()
        .map(|r| format_bytes(r.virt, args.bytes))
        .collect();
    let cpu_s: Vec<String> = rows.iter().map(|r| format!("{:.1}", r.cpu)).collect();
    let thr_s: Vec<String> = rows.iter().map(|r| r.threads.to_string()).collect();

    let mut pid_w = PID_H.len();
    let mut user_w = USER_H.len();
    let mut mem_w = MEM_H.len();
    let mut virt_w = VIRT_H.len();
    let mut cpu_w = CPU_H.len();
    let mut thr_w = THR_H.len();

    for (i, r) in rows.iter().enumerate() {
        pid_w = pid_w.max(pid_s[i].len());
        user_w = user_w.max(r.user.len());
        mem_w = mem_w.max(rss_s[i].len());
        virt_w = virt_w.max(virt_s[i].len());
        cpu_w = cpu_w.max(cpu_s[i].len());
        thr_w = thr_w.max(thr_s[i].len());
    }

    let mut out = String::with_capacity(rows.len() * 80);

    if args.threads {
        out.push_str(&format!(
            "{:>pid_w$}  {:<user_w$}  {:>mem_w$}  {:>virt_w$}  {:>cpu_w$}  {:>thr_w$}  {}\n",
            PID_H, USER_H, MEM_H, VIRT_H, CPU_H, THR_H, NAME_H,
            pid_w = pid_w,
            user_w = user_w,
            mem_w = mem_w,
            virt_w = virt_w,
            cpu_w = cpu_w,
            thr_w = thr_w,
        ));
    } else {
        out.push_str(&format!(
            "{:>pid_w$}  {:<user_w$}  {:>mem_w$}  {:>virt_w$}  {:>cpu_w$}  {}\n",
            PID_H, USER_H, MEM_H, VIRT_H, CPU_H, NAME_H,
            pid_w = pid_w,
            user_w = user_w,
            mem_w = mem_w,
            virt_w = virt_w,
            cpu_w = cpu_w,
        ));
    }

    for (i, r) in rows.iter().enumerate() {
        if args.threads {
            out.push_str(&format!(
                "{:>pid_w$}  {:<user_w$}  {:>mem_w$}  {:>virt_w$}  {:>cpu_w$}  {:>thr_w$}  {}\n",
                pid_s[i], r.user, rss_s[i], virt_s[i], cpu_s[i], thr_s[i], r.cmd,
                pid_w = pid_w,
                user_w = user_w,
                mem_w = mem_w,
                virt_w = virt_w,
                cpu_w = cpu_w,
                thr_w = thr_w,
            ));
        } else {
            out.push_str(&format!(
                "{:>pid_w$}  {:<user_w$}  {:>mem_w$}  {:>virt_w$}  {:>cpu_w$}  {}\n",
                pid_s[i], r.user, rss_s[i], virt_s[i], cpu_s[i], r.cmd,
                pid_w = pid_w,
                user_w = user_w,
                mem_w = mem_w,
                virt_w = virt_w,
                cpu_w = cpu_w,
            ));
        }
    }

    out
}
