// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

//! Configuration file parsing and precedence resolution.
//!
//! Precedence order: CLI flag > environment variable > config file > default.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use cram_vertex::Config as VertexConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct ConfigFile {
    pub port: Option<u16>,
    pub vertex: Option<VertexSection>,
    pub models: Option<HashMap<String, String>>,
    pub update: Option<UpdateSection>,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct UpdateSection {
    pub check: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct VertexSection {
    pub project: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct CredentialsFile {
    pub vertex: Option<VertexCredentials>,
    pub gateway: Option<GatewayCredentials>,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct VertexCredentials {
    pub key_file: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub struct GatewayCredentials {
    pub api_key: Option<String>,
}

pub struct ResolvedConfig {
    pub vertex: VertexConfig,
    pub port: u16,
}

pub fn is_update_check_disabled(config_file: Option<&ConfigFile>) -> bool {
    if let Ok(val) = std::env::var("CRAM_NO_UPDATE_CHECK") {
        let trimmed = val.trim();
        if trimmed == "1"
            || trimmed.eq_ignore_ascii_case("true")
            || trimmed.eq_ignore_ascii_case("yes")
        {
            return true;
        }
    }
    if let Some(cfg) = config_file {
        if let Some(update) = &cfg.update {
            if update.check == Some(false) {
                return true;
            }
        }
    }
    false
}

/// Directory where cram stores its configuration and credentials.
/// Defaults to `~/.cram/`, overridable with `CRAM_HOME`.
pub fn cram_home() -> PathBuf {
    if let Ok(val) = std::env::var("CRAM_HOME") {
        if !val.trim().is_empty() {
            return PathBuf::from(val);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".cram");
        }
    }
    PathBuf::from(".cram")
}

pub fn load_config_file(path: &Path) -> anyhow::Result<Option<ConfigFile>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {}", path.display(), e))?;
    let parsed: ConfigFile = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
    Ok(Some(parsed))
}

pub fn load_credentials_file(path: &Path) -> anyhow::Result<Option<CredentialsFile>> {
    if !path.exists() {
        return Ok(None);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                tracing::warn!(
                    "{} has permissions {:04o}, should be 0600 or stricter",
                    path.display(),
                    mode & 0o777
                );
            }
        }
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {}", path.display(), e))?;
    let parsed: CredentialsFile = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;
    Ok(Some(parsed))
}

pub fn save_credentials_file(path: &Path, creds: &CredentialsFile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(creds)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, content)?;
    }

    Ok(())
}

/// Resolve configuration from CLI, environment, and config files.
pub fn resolve(
    cli_port: Option<u16>,
    config_file: Option<ConfigFile>,
    credentials_file: Option<CredentialsFile>,
) -> anyhow::Result<ResolvedConfig> {
    let cfg_file = config_file.unwrap_or_default();
    let creds_file = credentials_file.unwrap_or_default();

    // 1. Port
    let port = if let Some(p) = cli_port {
        p
    } else if let Ok(bind_addr) = std::env::var("BIND_ADDR") {
        bind_addr
            .split(':')
            .next_back()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or_else(|| cfg_file.port.unwrap_or(8787))
    } else {
        cfg_file.port.unwrap_or(8787)
    };

    // 2. Project
    let project = if let Ok(p) = std::env::var("GCP_PROJECT_ID") {
        if !p.trim().is_empty() {
            Some(p)
        } else {
            None
        }
    } else {
        None
    }
    .or_else(|| cfg_file.vertex.as_ref().and_then(|v| v.project.clone()))
    .ok_or_else(|| {
        anyhow::anyhow!(
            "missing GCP_PROJECT_ID\nSet it in ~/.cram/config.toml ([vertex] project = \"...\") or via the GCP_PROJECT_ID environment variable."
        )
    })?;

    // 3. Location
    let location = if let Ok(l) = std::env::var("GCP_LOCATION") {
        if !l.trim().is_empty() {
            Some(l)
        } else {
            None
        }
    } else {
        None
    }
    .or_else(|| cfg_file.vertex.as_ref().and_then(|v| v.location.clone()))
    .unwrap_or_else(|| "global".to_string());

    // 4. Gateway Key
    let gateway_key = if let Ok(k) = std::env::var("GATEWAY_API_KEY") {
        k
    } else {
        creds_file
            .gateway
            .as_ref()
            .and_then(|g| g.api_key.clone())
            .unwrap_or_default()
    };

    // 5. Key File -> GOOGLE_APPLICATION_CREDENTIALS
    let env_creds = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok();
    if env_creds.as_deref().unwrap_or("").trim().is_empty() {
        if let Some(key_file) = creds_file.vertex.as_ref().and_then(|v| v.key_file.clone()) {
            if !key_file.trim().is_empty() {
                std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", key_file);
            }
        }
    }

    // 6. Model Aliases: default -> config.toml [models] -> MODEL_ALIASES env
    let mut aliases = cram_vertex::default_aliases();
    if let Some(models) = cfg_file.models {
        for (k, v) in models {
            aliases.insert(k, v);
        }
    }
    if let Ok(env_aliases) = std::env::var("MODEL_ALIASES") {
        if !env_aliases.trim().is_empty() {
            let parsed: HashMap<String, String> = serde_json::from_str(&env_aliases)
                .map_err(|e| anyhow::anyhow!("MODEL_ALIASES is not valid JSON: {e}"))?;
            for (k, v) in parsed {
                aliases.insert(k, v);
            }
        }
    }

    let vertex_config = VertexConfig::new(project)
        .with_location(location)
        .with_gateway_key(gateway_key)
        .with_aliases(aliases);

    Ok(ResolvedConfig {
        vertex: vertex_config,
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_env_overrides_config() {
        std::env::set_var("GCP_PROJECT_ID", "env-project");
        std::env::set_var("GCP_LOCATION", "asia-east1");
        std::env::set_var("GATEWAY_API_KEY", "env-key");

        let config_file = ConfigFile {
            port: Some(9999),
            vertex: Some(VertexSection {
                project: Some("config-project".into()),
                location: Some("us-central1".into()),
            }),
            models: None,
            update: None,
        };
        let credentials_file = CredentialsFile {
            vertex: None,
            gateway: Some(GatewayCredentials {
                api_key: Some("config-key".into()),
            }),
        };

        let res = resolve(None, Some(config_file), Some(credentials_file)).unwrap();
        assert_eq!(res.vertex.project(), "env-project");
        assert_eq!(res.vertex.location(), "asia-east1");
        assert_eq!(res.vertex.gateway_key(), "env-key");
        assert_eq!(res.port, 9999);

        std::env::remove_var("GCP_PROJECT_ID");
        std::env::remove_var("GCP_LOCATION");
        std::env::remove_var("GATEWAY_API_KEY");
    }

    #[test]
    fn update_check_disabled_via_env() {
        std::env::set_var("CRAM_NO_UPDATE_CHECK", "1");
        assert!(is_update_check_disabled(None));
        std::env::remove_var("CRAM_NO_UPDATE_CHECK");
    }

    #[test]
    fn update_check_disabled_via_config() {
        std::env::remove_var("CRAM_NO_UPDATE_CHECK");
        let cfg = ConfigFile {
            update: Some(UpdateSection { check: Some(false) }),
            ..Default::default()
        };
        assert!(is_update_check_disabled(Some(&cfg)));
    }

    #[test]
    fn precedence_flag_overrides_env_and_config() {
        std::env::set_var("GCP_PROJECT_ID", "p");
        std::env::set_var("BIND_ADDR", "127.0.0.1:8888");

        let config_file = ConfigFile {
            port: Some(7777),
            vertex: None,
            models: None,
            update: None,
        };
        let res = resolve(Some(9000), Some(config_file), None).unwrap();
        assert_eq!(res.port, 9000);

        std::env::remove_var("GCP_PROJECT_ID");
        std::env::remove_var("BIND_ADDR");
    }

    #[test]
    fn missing_config_file_falls_back_to_env() {
        std::env::set_var("GCP_PROJECT_ID", "fallback-project");
        std::env::remove_var("GCP_LOCATION");
        std::env::remove_var("GATEWAY_API_KEY");
        std::env::remove_var("BIND_ADDR");

        let res = resolve(None, None, None).unwrap();
        assert_eq!(res.vertex.project(), "fallback-project");
        assert_eq!(res.vertex.location(), "global");
        assert_eq!(res.vertex.gateway_key(), "");
        assert_eq!(res.port, 8787);

        std::env::remove_var("GCP_PROJECT_ID");
    }

    #[test]
    fn malformed_toml_produces_error_naming_path() {
        let dir = std::env::temp_dir().join(format!("cram-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config.toml");
        std::fs::write(&config_path, "invalid toml ::: = ").unwrap();

        let err = load_config_file(&config_path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(&config_path.display().to_string()),
            "error should contain path: {msg}"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    #[cfg(unix)]
    fn credentials_file_written_with_0600_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("cram-creds-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let creds_path = dir.join("credentials.toml");

        let creds = CredentialsFile {
            vertex: Some(VertexCredentials {
                key_file: Some("/path/to/sa.json".into()),
            }),
            gateway: Some(GatewayCredentials {
                api_key: Some("secret".into()),
            }),
        };

        save_credentials_file(&creds_path, &creds).unwrap();
        let meta = std::fs::metadata(&creds_path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);

        let _ = std::fs::remove_dir_all(dir);
    }
}
