//! Update check against GitHub Releases: version comparison and installer asset lookup.

pub const LATEST_URL: &str = "https://api.github.com/repos/TonisOrmisson/wspr/releases/latest";

#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// Lowercase hex SHA-256 of the installer, as published by GitHub.
    pub sha256: String,
}

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

/// Picks the installer asset from a GitHub "latest release" JSON. None if it has no usable installer.
pub fn parse_latest(json: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Bad update info: {e}"))?;
    let Some(tag) = v["tag_name"].as_str() else {
        return Err(format!(
            "Bad update info: {}",
            v["message"].as_str().unwrap_or("no tag")
        ));
    };
    let assets = v["assets"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let installer = assets.iter().find(|a| {
        let name = a["name"].as_str().unwrap_or("");
        name.starts_with("wspr-setup-") && name.ends_with(".exe")
    });
    Ok(installer.and_then(|a| {
        Some(Release {
            version: tag.trim_start_matches('v').to_string(),
            url: a["browser_download_url"].as_str()?.to_string(),
            // Without a checksum the download cannot be verified, so it is not offered.
            sha256: a["digest"]
                .as_str()?
                .strip_prefix("sha256:")?
                .to_ascii_lowercase(),
        })
    }))
}

pub fn sha256_matches(bytes: &[u8], expected_hex: &str) -> bool {
    use sha2::{Digest, Sha256};
    crate::settings::to_hex(&Sha256::digest(bytes)) == expected_hex
}

/// Ok(None): already up to date. Ok(Some): a newer, checksum-verified installer.
#[cfg(windows)]
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
            .header("Accept", "application/vnd.github+json")
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
    let json = String::from_utf8_lossy(&get(LATEST_URL)?).into_owned();
    let Some(rel) = parse_latest(&json)? else {
        return Ok(None);
    };
    if !is_newer(&rel.version, current) {
        return Ok(None);
    }
    let bytes = get(&rel.url)?;
    if !sha256_matches(&bytes, &rel.sha256) {
        return Err("Update download is corrupt (checksum mismatch)".into());
    }
    Ok(Some((rel.version, bytes)))
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
    fn finds_installer_with_digest() {
        let json = r#"{"tag_name":"v0.3.0","assets":[
            {"name":"wspr.exe","browser_download_url":"https://x/wspr.exe","digest":"sha256:aa"},
            {"name":"wspr-setup-0.3.0.exe","browser_download_url":"https://x/setup.exe","digest":"sha256:ABCD"}]}"#;
        assert_eq!(
            parse_latest(json),
            Ok(Some(Release {
                version: "0.3.0".into(),
                url: "https://x/setup.exe".into(),
                sha256: "abcd".into()
            }))
        );
    }

    #[test]
    fn rejects_missing_digest_and_errors() {
        let no_digest = r#"{"tag_name":"v0.3.0","assets":[{"name":"wspr-setup-0.3.0.exe","browser_download_url":"https://x"}]}"#;
        assert_eq!(parse_latest(no_digest), Ok(None));
        assert_eq!(
            parse_latest(r#"{"tag_name":"v0.3.0","assets":[]}"#),
            Ok(None)
        );
        assert!(
            parse_latest(r#"{"message":"Not Found"}"#)
                .unwrap_err()
                .contains("Not Found")
        );
    }
}
