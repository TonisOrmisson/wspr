//! Update check against GitHub Releases through github.com itself, not the GitHub API: the API
//! allows only 60 requests an hour per IP address without a login, shared by everything on the network.

pub const RELEASES: &str = "https://github.com/TonisOrmisson/wspr/releases";

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse().ok());
    let r = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(r)
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

/// The release asset this platform updates from: the installer on Windows, the binary on Linux.
pub fn update_asset(version: &str) -> String {
    if cfg!(windows) {
        format!("wspr-setup-{version}.exe")
    } else {
        "wspr-linux-x86_64".into()
    }
}

/// The version /releases/latest redirects to, e.g. ".../releases/tag/v1.2.3" gives "1.2.3".
pub fn version_from_location(location: &str) -> Option<String> {
    let tag = location.rsplit_once("/tag/")?.1;
    parse_version(tag)?;
    Some(tag.trim_start_matches('v').to_string())
}

/// The lowercase SHA-256 of `name` in a SHA256SUMS file (`sha256sum` output).
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hex, file) = l.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hex.len() == 64)
            .then(|| hex.to_ascii_lowercase())
    })
}

pub fn sha256_matches(bytes: &[u8], expected_hex: &str) -> bool {
    use sha2::{Digest, Sha256};
    crate::settings::to_hex(&Sha256::digest(bytes)) == expected_hex
}

/// Ok(None): already up to date. Ok(Some): a newer, checksum-verified installer.
#[cfg(any(windows, target_os = "linux"))]
pub fn fetch_newer(current: &str) -> Result<Option<(String, Vec<u8>)>, String> {
    use std::time::Duration;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let get = |url: &str| -> Result<Vec<u8>, String> {
        let mut resp = agent
            .get(url)
            .header("User-Agent", &format!("wspr/{current}"))
            .call()
            .map_err(|e| format!("Update check failed: {e}"))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("Update check failed: HTTP {status}"));
        }
        resp.body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| format!("Update download failed: {e}"))
    };
    let resp = agent
        .get(&format!("{RELEASES}/latest"))
        .header("User-Agent", &format!("wspr/{current}"))
        .config()
        .max_redirects(0)
        .build()
        .call()
        .map_err(|e| format!("Update check failed: {e}"))?;
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| format!("Update check failed: HTTP {}", resp.status().as_u16()))?;
    let version = version_from_location(location)
        .ok_or_else(|| format!("Update check failed: no release in {location}"))?;
    if !is_newer(&version, current) {
        return Ok(None);
    }
    let base = format!("{RELEASES}/download/v{version}");
    let name = update_asset(&version);
    let sums = String::from_utf8_lossy(&get(&format!("{base}/SHA256SUMS"))?).into_owned();
    // Without a checksum the download cannot be verified, so it is not installed.
    let sha256 = sum_for(&sums, &name).ok_or("Update has no checksum")?;
    let bytes = get(&format!("{base}/{name}"))?;
    if !sha256_matches(&bytes, &sha256) {
        return Err("Update download is corrupt (checksum mismatch)".into());
    }
    Ok(Some((version, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_sha256() {
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(sha256_matches(b"abc", abc));
        assert!(!sha256_matches(b"abd", abc));
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("garbage", "0.2.0"));
        assert!(!is_newer("1.0.0.1", "0.2.0"));
    }

    #[test]
    fn update_asset_per_platform() {
        let name = update_asset("1.0.0");
        assert_eq!(name == "wspr-linux-x86_64", cfg!(target_os = "linux"));
        assert_eq!(name == "wspr-setup-1.0.0.exe", cfg!(windows));
    }

    #[test]
    fn reads_version_from_redirect() {
        let loc = "https://github.com/TonisOrmisson/wspr/releases/tag/v0.12.2";
        assert_eq!(version_from_location(loc), Some("0.12.2".into()));
        assert_eq!(version_from_location("https://github.com/x/releases"), None);
        assert_eq!(version_from_location("https://x/tag/latest"), None);
    }

    #[test]
    fn finds_checksum() {
        let a = "a".repeat(64);
        let sums = format!(
            "{a}  wspr-linux-x86_64\n{}  *wspr-setup-1.0.0.exe\n",
            "B".repeat(64)
        );
        assert_eq!(sum_for(&sums, "wspr-linux-x86_64"), Some(a));
        assert_eq!(sum_for(&sums, "wspr-setup-1.0.0.exe"), Some("b".repeat(64)));
        assert_eq!(sum_for(&sums, "wspr.exe"), None);
        assert_eq!(sum_for("abc  wspr.exe", "wspr.exe"), None);
    }
}
