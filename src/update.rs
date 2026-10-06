use std::os::unix::fs::PermissionsExt;
use std::process::{Command, exit};

const REPO: &str = "https://github.com/Arshdeep54/memtop";

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

/// `v1.2.3` / `1.2.3` -> (1, 2, 3)
pub(crate) fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.');
    let mut next = || it.next()?.split(['-', '+']).next()?.parse().ok();
    Some((next()?, next()?, next()?))
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "60"])
        .args(args)
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// `/releases/latest` redirects to `/releases/tag/<tag>`: no API, no rate limit.
fn latest_tag() -> Result<String, String> {
    let url = curl(&[
        "-I",
        "-o",
        "/dev/null",
        "-w",
        "%{url_effective}",
        &format!("{REPO}/releases/latest"),
    ])?;
    let url = String::from_utf8_lossy(&url);
    url.rsplit_once("/tag/")
        .map(|(_, t)| t.trim().to_string())
        .ok_or_else(|| format!("no release found at {REPO}/releases"))
}

pub(crate) fn update(check_only: bool) {
    let current = env!("CARGO_PKG_VERSION");
    let tag = latest_tag().unwrap_or_else(|e| fail(&e));
    let (Some(cur), Some(new)) = (parse_version(current), parse_version(&tag)) else {
        fail(&format!("cannot compare versions {current} and {tag}"));
    };
    if new <= cur {
        println!("memtop {current} is up to date");
        return;
    }
    println!("memtop {current} -> {tag}");
    if check_only {
        println!("run `memtop update` to install it");
        return;
    }

    let target = match std::env::consts::ARCH {
        a @ ("x86_64" | "aarch64") => format!("{a}-unknown-linux-gnu"),
        a => fail(&format!("no pre-built binary for {a}, use cargo install")),
    };
    let exe = std::env::current_exe().unwrap_or_else(|e| fail(&format!("current exe: {e}")));
    // same directory as the target so the final rename is atomic
    let tmp = exe.with_extension("update");
    let url = format!("{REPO}/releases/download/{tag}/memtop-{target}");
    let bytes = curl(&[&url]).unwrap_or_else(|e| fail(&format!("download {url}: {e}")));

    let installed = std::fs::write(&tmp, &bytes)
        .and_then(|_| std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)))
        .map_err(|e| format!("cannot write next to {}: {e}", exe.display()))
        .and_then(|_| {
            // never replace a working binary with one that does not run
            match Command::new(&tmp).arg("--version").output() {
                Ok(o) if o.status.success() => Ok(()),
                _ => Err("downloaded binary does not run".to_string()),
            }
        })
        // replacing a running binary by rename is fine on Linux
        .and_then(|_| std::fs::rename(&tmp, &exe).map_err(|e| e.to_string()));
    if let Err(e) = installed {
        let _ = std::fs::remove_file(&tmp);
        fail(&e);
    }
    println!("updated {} to {tag}", exe.display());

    // a running guard keeps the old binary in memory until restarted
    let restarted = Command::new("systemctl")
        .args(["--user", "try-restart", crate::guard::SERVICE])
        .status();
    if restarted.is_ok_and(|s| s.success()) {
        println!("restarted {} if it was running", crate::guard::SERVICE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_and_order() {
        assert_eq!(parse_version("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_version("nope"), None);
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
    }
}
