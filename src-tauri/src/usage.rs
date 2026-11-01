//! Client for the Claude OAuth usage endpoint (the data behind `/usage` in Claude Code).

use crate::credentials::Credentials;
use crate::pace::{self, Pace};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

// ---------- API response ----------

#[derive(Debug, Deserialize)]
pub struct UsageResponse {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub seven_day_opus: Option<Window>,
    pub seven_day_sonnet: Option<Window>,
    #[serde(default)]
    pub limits: Option<Vec<ApiLimit>>,
}

#[derive(Debug, Deserialize)]
pub struct Window {
    pub utilization: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct ApiLimit {
    pub kind: String,
    pub group: Option<String>,
    pub percent: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub scope: Option<serde_json::Value>,
}

// ---------- What the UI gets ----------

#[derive(Debug, Clone, Serialize)]
pub struct Limit {
    pub kind: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub window_hours: i64,
    pub pace: Option<Pace>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub plan: Option<String>,
    pub limits: Vec<Limit>,
    pub fetched_at: DateTime<Utc>,
}

impl Snapshot {
    pub fn session(&self) -> Option<&Limit> {
        self.limits.iter().find(|l| l.kind == "session")
    }
    pub fn weekly(&self) -> Option<&Limit> {
        self.limits.iter().find(|l| l.kind == "weekly_all")
    }
}

pub async fn fetch(client: &reqwest::Client, creds: &Credentials) -> Result<Snapshot, String> {
    if let Some(exp) = creds.expires_at {
        if exp < Utc::now().timestamp_millis() {
            return Err("Claude login token expired. Run any `claude` command to refresh it.".into());
        }
    }

    let resp = client
        .get(USAGE_URL)
        .bearer_auth(&creds.access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", concat!("claude-limits/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Claude rejected the login token. Run `claude` to log in again.".into());
    }
    if !status.is_success() {
        return Err(format!("Usage API returned HTTP {status}"));
    }
    let body: UsageResponse = resp.json().await.map_err(|e| format!("Bad usage response: {e}"))?;
    Ok(build_snapshot(body, plan_name(creds), Utc::now()))
}

fn plan_name(creds: &Credentials) -> Option<String> {
    let tier = creds.rate_limit_tier.as_deref().unwrap_or("");
    let multiplier = ["20x", "5x"].into_iter().find(|m| tier.ends_with(m));
    let base = match creds.subscription_type.as_deref()? {
        "max" => "Max".to_string(),
        "pro" => "Pro".to_string(),
        "team" => "Team".to_string(),
        "enterprise" => "Enterprise".to_string(),
        other => other.to_string(),
    };
    Some(match multiplier {
        Some(m) if tier.contains("max") && base != "Max" => format!("{base} · Max {m}"),
        Some(m) => format!("{base} {m}"),
        None => base,
    })
}

pub fn build_snapshot(body: UsageResponse, plan: Option<String>, now: DateTime<Utc>) -> Snapshot {
    let mut limits: Vec<Limit> = match body.limits {
        Some(list) if !list.is_empty() => list
            .into_iter()
            .filter_map(|l| {
                let used = l.percent?;
                let window_hours = window_hours(&l.kind, l.group.as_deref());
                Some(make_limit(l.kind.clone(), label_for(&l), used, l.resets_at, window_hours, now))
            })
            .collect(),
        _ => legacy_limits(&body, now),
    };
    // Session first, then the overall weekly limit, then per-model weekly limits.
    limits.sort_by_key(|l| match l.kind.as_str() {
        "session" => 0,
        "weekly_all" => 1,
        _ => 2,
    });
    Snapshot { plan, limits, fetched_at: now }
}

fn legacy_limits(body: &UsageResponse, now: DateTime<Utc>) -> Vec<Limit> {
    let pairs = [
        ("session", "Session (5h)", &body.five_hour, 5),
        ("weekly_all", "Weekly", &body.seven_day, 168),
        ("weekly_opus", "Weekly · Opus", &body.seven_day_opus, 168),
        ("weekly_sonnet", "Weekly · Sonnet", &body.seven_day_sonnet, 168),
    ];
    pairs
        .into_iter()
        .filter_map(|(kind, label, w, hours)| {
            let w = w.as_ref()?;
            Some(make_limit(kind.into(), label.into(), w.utilization?, w.resets_at, hours, now))
        })
        .collect()
}

fn make_limit(
    kind: String,
    label: String,
    used: f64,
    resets_at: Option<DateTime<Utc>>,
    window_hours: i64,
    now: DateTime<Utc>,
) -> Limit {
    let pace = resets_at.map(|r| pace::compute(used, r, Duration::hours(window_hours), now));
    Limit { kind, label, used_percent: used, resets_at, window_hours, pace }
}

fn window_hours(kind: &str, group: Option<&str>) -> i64 {
    if kind == "session" || group == Some("session") {
        5
    } else {
        168
    }
}

fn label_for(l: &ApiLimit) -> String {
    match l.kind.as_str() {
        "session" => "Session (5h)".into(),
        "weekly_all" => "Weekly · all models".into(),
        _ => {
            let model = l
                .scope
                .as_ref()
                .and_then(|s| s.pointer("/model/display_name"))
                .and_then(|v| v.as_str());
            match model {
                Some(m) => format!("Weekly · {m}"),
                None => "Weekly".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "five_hour": {"utilization": 5.0, "resets_at": "2026-10-31T23:39:59.589300+00:00"},
      "seven_day": {"utilization": 21.0, "resets_at": "2026-11-04T12:59:59.589326+00:00"},
      "seven_day_opus": null,
      "limits": [
        {"kind": "weekly_all", "group": "weekly", "percent": 21, "resets_at": "2026-11-04T12:59:59.589326+00:00", "scope": null},
        {"kind": "session", "group": "session", "percent": 5, "resets_at": "2026-10-31T23:39:59.589300+00:00", "scope": null},
        {"kind": "weekly_scoped", "group": "weekly", "percent": 0, "resets_at": "2026-11-04T13:00:00+00:00",
         "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}}
      ]
    }"#;

    fn now() -> DateTime<Utc> {
        "2026-10-31T22:00:00Z".parse().unwrap()
    }

    #[test]
    fn uses_limits_array_and_orders_it() {
        let body: UsageResponse = serde_json::from_str(SAMPLE).unwrap();
        let s = build_snapshot(body, None, now());
        let kinds: Vec<_> = s.limits.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, ["session", "weekly_all", "weekly_scoped"]);
        assert_eq!(s.limits[2].label, "Weekly · Fable");
        assert_eq!(s.session().unwrap().window_hours, 5);
        assert_eq!(s.weekly().unwrap().used_percent, 21.0);
        assert!(s.weekly().unwrap().pace.is_some());
    }

    #[test]
    fn falls_back_to_legacy_fields() {
        let body: UsageResponse = serde_json::from_str(
            r#"{"five_hour":{"utilization":40,"resets_at":"2026-11-01T00:00:00Z"},
                "seven_day":{"utilization":70,"resets_at":"2026-11-02T00:00:00Z"}}"#,
        )
        .unwrap();
        let s = build_snapshot(body, None, now());
        assert_eq!(s.limits.len(), 2);
        assert_eq!(s.session().unwrap().used_percent, 40.0);
        assert_eq!(s.weekly().unwrap().used_percent, 70.0);
    }

    #[test]
    fn plan_names() {
        let c = |sub: &str, tier: &str| Credentials {
            access_token: String::new(),
            expires_at: None,
            subscription_type: Some(sub.into()),
            rate_limit_tier: Some(tier.into()),
        };
        assert_eq!(plan_name(&c("max", "default_claude_max_20x")).unwrap(), "Max 20x");
        assert_eq!(plan_name(&c("pro", "default_claude_pro")).unwrap(), "Pro");
        assert_eq!(plan_name(&c("team", "default_claude_max_5x")).unwrap(), "Team · Max 5x");
    }
}
