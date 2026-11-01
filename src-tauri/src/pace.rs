//! Pace projection: compares usage with the share of the window that has passed
//! and extrapolates the current rate to the reset time.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PaceStatus {
    /// Too early in the window to extrapolate.
    TooEarly,
    /// Projected to stay under the limit until reset.
    OnTrack,
    /// Projected to hit the limit before reset.
    RunsOut,
    /// Limit already reached.
    Exhausted,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pace {
    /// Usage % you would be at if you spread the limit evenly across the window.
    pub expected_percent: f64,
    /// used - expected. Positive = ahead of pace (burning faster than even).
    pub delta_percent: f64,
    /// Usage % at reset if the current average rate continues.
    pub projected_percent: Option<f64>,
    /// When the limit is hit at the current average rate (only if before reset).
    pub runs_out_at: Option<DateTime<Utc>>,
    pub status: PaceStatus,
}

/// Minimum share of the window that must have passed before extrapolating.
const MIN_ELAPSED_FRACTION: f64 = 0.03;

pub fn compute(used: f64, resets_at: DateTime<Utc>, window: Duration, now: DateTime<Utc>) -> Pace {
    let window_s = window.num_seconds().max(1) as f64;
    let remaining_s = (resets_at - now).num_seconds().clamp(0, window.num_seconds()) as f64;
    let elapsed_s = window_s - remaining_s;
    let expected = (elapsed_s / window_s * 100.0).clamp(0.0, 100.0);
    let delta = used - expected;

    if used >= 100.0 {
        return Pace {
            expected_percent: expected,
            delta_percent: delta,
            projected_percent: Some(used),
            runs_out_at: None,
            status: PaceStatus::Exhausted,
        };
    }

    if elapsed_s / window_s < MIN_ELAPSED_FRACTION || used <= 0.0 {
        return Pace {
            expected_percent: expected,
            delta_percent: delta,
            projected_percent: if used <= 0.0 { Some(0.0) } else { None },
            runs_out_at: None,
            status: if used <= 0.0 { PaceStatus::OnTrack } else { PaceStatus::TooEarly },
        };
    }

    let rate_per_s = used / elapsed_s;
    let projected = used + rate_per_s * remaining_s;
    if projected > 100.0 {
        let secs_to_full = (100.0 - used) / rate_per_s;
        Pace {
            expected_percent: expected,
            delta_percent: delta,
            projected_percent: Some(projected),
            runs_out_at: Some(now + Duration::seconds(secs_to_full.round() as i64)),
            status: PaceStatus::RunsOut,
        }
    } else {
        Pace {
            expected_percent: expected,
            delta_percent: delta,
            projected_percent: Some(projected),
            runs_out_at: None,
            status: PaceStatus::OnTrack,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(0, 0).unwrap() + Duration::hours(h)
    }

    #[test]
    fn halfway_through_week_at_half_usage_is_on_pace() {
        let p = compute(50.0, at(84), Duration::days(7), at(0));
        assert!((p.expected_percent - 50.0).abs() < 1e-9);
        assert!(p.delta_percent.abs() < 1e-9);
        assert_eq!(p.status, PaceStatus::OnTrack);
        assert!((p.projected_percent.unwrap() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn fast_burn_runs_out_before_reset() {
        // One day into the week, 30% used -> projected 210%.
        let p = compute(30.0, at(144), Duration::days(7), at(0));
        assert_eq!(p.status, PaceStatus::RunsOut);
        // 70% left at 30%/day = 56h.
        assert_eq!(p.runs_out_at.unwrap(), at(56));
        assert!(p.delta_percent > 0.0);
    }

    #[test]
    fn slow_burn_lasts() {
        let p = compute(10.0, at(84), Duration::days(7), at(0));
        assert_eq!(p.status, PaceStatus::OnTrack);
        assert!((p.projected_percent.unwrap() - 20.0).abs() < 1e-9);
        assert!(p.runs_out_at.is_none());
    }

    #[test]
    fn too_early_to_tell() {
        let p = compute(5.0, at(167), Duration::days(7), at(0));
        assert_eq!(p.status, PaceStatus::TooEarly);
    }

    #[test]
    fn exhausted() {
        let p = compute(100.0, at(2), Duration::hours(5), at(0));
        assert_eq!(p.status, PaceStatus::Exhausted);
    }

    #[test]
    fn reset_time_in_the_past_is_clamped() {
        let p = compute(40.0, at(-1), Duration::hours(5), at(0));
        assert!((p.expected_percent - 100.0).abs() < 1e-9);
    }
}
