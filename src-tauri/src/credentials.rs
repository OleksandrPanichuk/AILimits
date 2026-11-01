//! Reads the OAuth token that Claude Code stores after `claude` login.
//!
//! Windows/Linux: `%USERPROFILE%\.claude\.credentials.json` (or `$CLAUDE_CONFIG_DIR`).
//! macOS: the "Claude Code-credentials" Keychain item, with the file as fallback.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_token: String,
    /// Unix time in milliseconds.
    pub expires_at: Option<i64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OAuthBlock>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthBlock {
    access_token: String,
    expires_at: Option<i64>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

pub fn credentials_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return Some(PathBuf::from(dir).join(".credentials.json"));
        }
    }
    dirs::home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

pub fn parse(json: &str) -> Result<Credentials, String> {
    let file: CredentialsFile =
        serde_json::from_str(json).map_err(|e| format!("Cannot parse Claude credentials: {e}"))?;
    let block = file
        .claude_ai_oauth
        .ok_or("Claude credentials have no claudeAiOauth entry. Log in with `claude` first.")?;
    Ok(Credentials {
        access_token: block.access_token,
        expires_at: block.expires_at,
        subscription_type: block.subscription_type,
        rate_limit_tier: block.rate_limit_tier,
    })
}

pub fn load() -> Result<Credentials, String> {
    #[cfg(target_os = "macos")]
    if let Some(json) = read_macos_keychain() {
        return parse(&json);
    }

    let path = credentials_path().ok_or("Cannot find the home directory")?;
    let json = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Claude Code credentials not found at {}. Install Claude Code and run `claude` to log in.",
            path.display()
        )
    })?;
    parse(&json)
}

#[cfg(target_os = "macos")]
fn read_macos_keychain() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok().map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_credentials_file() {
        let json = r#"{"claudeAiOauth":{"accessToken":"tok","refreshToken":"r","expiresAt":1793484907860,"scopes":[],"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#;
        let c = parse(json).unwrap();
        assert_eq!(c.access_token, "tok");
        assert_eq!(c.expires_at, Some(1793484907860));
        assert_eq!(c.subscription_type.as_deref(), Some("max"));
    }

    #[test]
    fn missing_block_is_an_error() {
        assert!(parse("{}").is_err());
    }
}
