// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config;

const DEFAULT_RELEASE_URL: &str = "https://github.com/revises-org/cram/releases";

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCache {
    pub last_check: u64,
    pub latest_version: String,
    pub html_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Option<String>,
}

fn cache_path() -> PathBuf {
    config::cram_home().join("update-check.json")
}

pub fn get_cached_update_notice(disabled: bool) -> Option<(String, String)> {
    if disabled {
        return None;
    }

    let path = cache_path();
    if !path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&path).ok()?;
    let cache: UpdateCache = serde_json::from_str(&content).ok()?;

    let clean_latest = cache.latest_version.trim_start_matches('v');
    let latest_ver = Version::parse(clean_latest).ok()?;
    let current_ver = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;

    if latest_ver > current_ver {
        // Ignore prereleases unless the running version is a prerelease
        if !latest_ver.pre.is_empty() && current_ver.pre.is_empty() {
            return None;
        }

        let release_url = DEFAULT_RELEASE_URL.to_string();
        return Some((clean_latest.to_string(), release_url));
    }

    None
}

pub fn spawn_update_check(disabled: bool) {
    if disabled {
        return;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let path = cache_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(cache) = serde_json::from_str::<UpdateCache>(&content) {
                // Re-check at most every 24 hours (86400 seconds)
                if now.saturating_sub(cache.last_check) < 86400 {
                    return;
                }
            }
        }
    }

    tokio::spawn(async move {
        let current_version = env!("CARGO_PKG_VERSION");
        let user_agent = format!("cram/{current_version}");

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .user_agent(user_agent)
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let response = match client
            .get("https://api.github.com/repos/revises-org/cram/releases/latest")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp,
            _ => return,
        };

        let release: GitHubRelease = match response.json().await {
            Ok(r) => r,
            Err(_) => return,
        };

        let clean_tag = release.tag_name.trim_start_matches('v').to_string();
        if Version::parse(&clean_tag).is_err() {
            return;
        }

        let cache = UpdateCache {
            last_check: now,
            latest_version: clean_tag,
            html_url: release.html_url,
        };

        let home = config::cram_home();
        let _ = std::fs::create_dir_all(&home);
        if let Ok(json_str) = serde_json::to_string_pretty(&cache) {
            let _ = std::fs::write(home.join("update-check.json"), json_str);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_returns_none() {
        assert_eq!(get_cached_update_notice(true), None);
    }

    #[test]
    fn test_cached_notice_with_mock_cache() {
        let dir = std::env::temp_dir().join(format!("cram-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("CRAM_HOME", &dir);

        let cache_file = dir.join("update-check.json");

        // Older version -> None
        let cache_older = UpdateCache {
            last_check: 100,
            latest_version: "0.0.1".into(),
            html_url: None,
        };
        std::fs::write(&cache_file, serde_json::to_string(&cache_older).unwrap()).unwrap();
        assert_eq!(get_cached_update_notice(false), None);

        // Newer version -> Some
        let cache_newer = UpdateCache {
            last_check: 100,
            latest_version: "v999.0.0".into(),
            html_url: Some("https://example.com/release".into()),
        };
        std::fs::write(&cache_file, serde_json::to_string(&cache_newer).unwrap()).unwrap();
        let notice = get_cached_update_notice(false);
        assert!(notice.is_some());
        let (ver, url) = notice.unwrap();
        assert_eq!(ver, "999.0.0");
        assert_eq!(url, "https://github.com/revises-org/cram/releases");

        // Newer prerelease -> None when current is stable
        let cache_pre = UpdateCache {
            last_check: 100,
            latest_version: "v999.0.0-rc.1".into(),
            html_url: None,
        };
        std::fs::write(&cache_file, serde_json::to_string(&cache_pre).unwrap()).unwrap();
        assert_eq!(get_cached_update_notice(false), None);

        std::env::remove_var("CRAM_HOME");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_semver_comparison_logic() {
        let current = Version::parse("0.9.0").unwrap();
        let newer = Version::parse("0.10.0").unwrap();
        assert!(newer > current);

        let prerelease = Version::parse("0.10.0-rc.1").unwrap();
        assert!(prerelease > current);
        // Ignore prereleases if current is stable
        assert!(!prerelease.pre.is_empty() && current.pre.is_empty());

        let current_pre = Version::parse("0.10.0-rc.1").unwrap();
        let newer_pre = Version::parse("0.10.0-rc.2").unwrap();
        assert!(newer_pre > current_pre);
        // Allowed because current is also a prerelease
        assert!(!(!newer_pre.pre.is_empty() && current_pre.pre.is_empty()));
    }
}
