use std::collections::HashMap;
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::Args;
use crate::format::format_bytes;
use crate::proc::collect_processes;
use crate::render::{Col, render_columns};

/// A memory series over time: (seconds since start, bytes).
type Sample = (f64, u64);

pub(crate) fn track(args: &Args, duration: f64, json: bool) {
    let interval = args.interval.max(0.1);
    let start = Instant::now();
    let expected = ((duration / interval).ceil() as usize).max(1);

    // per-process series key on (pid, start_time) so a reused pid never
    // merges into one series; group series key on command_key
    let mut series: HashMap<String, (String, Vec<Sample>)> = HashMap::new();

    let mut stderr = std::io::stderr();
    let mut planned = 0;
    loop {
        let elapsed = start.elapsed().as_secs_f64();
        if elapsed >= duration {
            break;
        }

        let rows = collect_processes(args);
        let t = start.elapsed().as_secs_f64();
        if args.group {
            let mut sums: HashMap<String, u64> = HashMap::new();
            for r in &rows {
                *sums.entry(r.cmd.clone()).or_default() += r.pss.unwrap_or(r.rss);
            }
            for (name, mem) in sums {
                series
                    .entry(name.clone())
                    .or_insert_with(|| (name.clone(), Vec::new()))
                    .1
                    .push((t, mem));
            }
        } else {
            for r in &rows {
                let key = format!("{}:{}", r.pid, r.start_time);
                series
                    .entry(key)
                    .or_insert_with(|| (r.cmd.clone(), Vec::new()))
                    .1
                    .push((t, r.pss.unwrap_or(r.rss)));
            }
        }

        planned += 1;
        let _ = write!(
            stderr,
            "\r  sampling {planned}/{expected} samples ({:.0}s left)",
            (duration - t).max(0.0)
        );
        let _ = stderr.flush();

        let spent = start.elapsed().as_secs_f64() - elapsed;
        let sleep_for = (interval - spent).max(0.05);
        thread::sleep(Duration::from_secs_f64(sleep_for.min(interval)));
    }
    let _ = write!(stderr, "\r\x1b[K");
    let _ = stderr.flush();

    let mut results: Vec<TrackRow> = series
        .into_values()
        .filter_map(|(name, samples)| {
            if !eligible(samples.len(), expected) {
                return None;
            }
            let first = samples[0].1;
            let last = samples[samples.len() - 1].1;
            Some(TrackRow {
                name,
                first,
                last,
                growth_mib_per_min: growth_mib_per_min(&samples),
                monotonic: monotonic_fraction(&samples),
            })
        })
        .collect();
    results.sort_by(|a, b| b.growth_mib_per_min.total_cmp(&a.growth_mib_per_min));

    if json {
        let rows: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "first_bytes": r.first,
                    "last_bytes": r.last,
                    "delta_bytes": r.last as i64 - r.first as i64,
                    "mib_per_min": r.growth_mib_per_min,
                    "monotonic": r.monotonic,
                })
            })
            .collect();
        let mut out = serde_json::to_string_pretty(&serde_json::json!({
            "duration_s": duration,
            "interval_s": interval,
            "series": rows,
        }))
        .unwrap_or_else(|_| "{}".to_string());
        out.push('\n');
        print!("{out}");
    } else {
        let raw = args.bytes;
        let cols = vec![
            Col {
                header: "NAME",
                cells: results.iter().map(|r| r.name.clone()).collect(),
                right: false,
            },
            Col {
                header: "START",
                cells: results.iter().map(|r| format_bytes(r.first, raw)).collect(),
                right: true,
            },
            Col {
                header: "NOW",
                cells: results.iter().map(|r| format_bytes(r.last, raw)).collect(),
                right: true,
            },
            Col {
                header: "DELTA",
                cells: results
                    .iter()
                    .map(|r| {
                        let d = r.last as i64 - r.first as i64;
                        let sign = if d > 0 { "+" } else { "" };
                        format!("{sign}{:.1} MiB", d as f64 / 1024.0 / 1024.0)
                    })
                    .collect(),
                right: true,
            },
            Col {
                header: "MiB/min",
                cells: results
                    .iter()
                    .map(|r| format!("{:+.2}", r.growth_mib_per_min))
                    .collect(),
                right: true,
            },
            Col {
                header: "MONO",
                cells: results.iter().map(|r| format!("{:.2}", r.monotonic)).collect(),
                right: true,
            },
        ];
        print!("{}", render_columns(&cols));
    }
}

struct TrackRow {
    name: String,
    first: u64,
    last: u64,
    growth_mib_per_min: f64,
    monotonic: f64,
}

/// A series must have at least 3 samples and cover at least half the window.
fn eligible(len: usize, expected: usize) -> bool {
    len >= 3 && len * 2 >= expected.max(1)
}

/// Least-squares slope in MiB per minute.
fn growth_mib_per_min(samples: &[Sample]) -> f64 {
    let n = samples.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let sum_t: f64 = samples.iter().map(|s| s.0).sum();
    let sum_m: f64 = samples.iter().map(|s| s.1 as f64).sum();
    let mean_t = sum_t / n;
    let mean_m = sum_m / n;
    let cov: f64 = samples.iter().map(|s| (s.0 - mean_t) * (s.1 as f64 - mean_m)).sum();
    let var: f64 = samples.iter().map(|s| (s.0 - mean_t) * (s.0 - mean_t)).sum();
    if var == 0.0 {
        return 0.0;
    }
    let slope = cov / var; // bytes per second
    slope * 60.0 / (1024.0 * 1024.0)
}

/// Fraction of consecutive samples that did not decrease; a steady climb
/// scores near 1.0, a spike-and-drop scores low.
fn monotonic_fraction(samples: &[Sample]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let non_decreasing = samples
        .windows(2)
        .filter(|w| w[1].1 >= w[0].1)
        .count();
    non_decreasing as f64 / (samples.len() - 1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse_duration;

    #[test]
    fn growth_linear_climb() {
        // 10 MiB per second
        let samples: Vec<Sample> = (0..6)
            .map(|i| (i as f64, (10 * i * 1024 * 1024) as u64))
            .collect();
        let g = growth_mib_per_min(&samples);
        assert!((g - 600.0).abs() < 1.0, "got {g}");
    }

    #[test]
    fn growth_flat_is_zero() {
        let samples: Vec<Sample> = (0..5).map(|i| (i as f64, 1024_u64)).collect();
        assert!(growth_mib_per_min(&samples).abs() < 1e-9);
    }

    #[test]
    fn monotonic_steady_climb_scores_one() {
        let samples: Vec<Sample> = (0..5).map(|i| (i as f64, (i * 100) as u64)).collect();
        assert!((monotonic_fraction(&samples) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn monotonic_spike_and_drop_scores_low() {
        let samples: Vec<Sample> = vec![(0.0, 100), (1.0, 200), (2.0, 300), (3.0, 50), (4.0, 40)];
        let m = monotonic_fraction(&samples);
        // half the steps dropped — well below a steady climb's 1.0
        assert!(m <= 0.5, "got {m}");
    }

    #[test]
    fn eligibility_rules() {
        assert!(!eligible(2, 10));
        assert!(eligible(3, 4)); // >= half the window
        assert!(!eligible(3, 10)); // present for less than half
        assert!(eligible(5, 10));
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s"), Ok(30.0));
        assert_eq!(parse_duration("5m"), Ok(300.0));
        assert_eq!(parse_duration("2h"), Ok(7200.0));
        assert_eq!(parse_duration("90"), Ok(90.0));
        assert!(parse_duration("5x").is_err());
        assert!(parse_duration("").is_err());
    }
}
