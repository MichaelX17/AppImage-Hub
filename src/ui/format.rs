//! Shared ETA / progress formatting. Single source of truth.

use std::time::Instant;

/// Format remaining download time as a short human-readable ETA.
///
/// Returns `"--"` when ETA cannot be determined yet.
pub fn format_eta(started_at: Instant, downloaded: u64, total: Option<u64>) -> String {
    let Some(total) = total else {
        return "--".into();
    };
    if downloaded == 0 {
        return "--".into();
    }
    let elapsed = started_at.elapsed().as_secs_f64();
    if elapsed <= 0.5 {
        return "--".into();
    }
    let rate = downloaded as f64 / elapsed;
    if rate <= 0.0 {
        return "--".into();
    }
    let remaining = total.saturating_sub(downloaded) as f64 / rate;
    if !remaining.is_finite() || remaining < 0.0 {
        return "--".into();
    }
    let secs = remaining.round() as u64;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Format download progress as `"<downloaded MB> / <total MB> MB"`.
///
/// `total` formats as `"?"` when unknown.
pub fn format_progress(downloaded: u64, total: Option<u64>) -> String {
    let downloaded_mb = downloaded as f64 / (1024.0 * 1024.0);
    let total_mb = total
        .map(|t| format!("{:.1}", t as f64 / (1024.0 * 1024.0)))
        .unwrap_or_else(|| "?".into());
    format!("{:.1} / {} MB", downloaded_mb, total_mb)
}

/// Format how long ago `then` was, for the footer's "Last checked" label.
pub fn format_relative_time(then: Instant) -> String {
    let secs = then.elapsed().as_secs();
    match secs {
        0..=9 => "just now".to_string(),
        10..=59 => format!("{secs} seconds ago"),
        60..=119 => "1 minute ago".to_string(),
        120..=3599 => format!("{} minutes ago", secs / 60),
        3600..=7199 => "1 hour ago".to_string(),
        7200..=86_399 => format!("{} hours ago", secs / 3600),
        86_400..=172_799 => "1 day ago".to_string(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn eta_matches_known_values() {
        let now = Instant::now();
        assert_eq!(format_eta(now, 0, Some(100)), "--");
        assert_eq!(format_eta(now, 50, None), "--");
        assert_eq!(
            format_eta(now - Duration::from_millis(100), 50, Some(100)),
            "--"
        );
        assert_eq!(
            format_eta(now - Duration::from_secs(10), 50, Some(100)),
            "10s"
        );
        assert_eq!(
            format_eta(now - Duration::from_secs(10), 10, Some(130)),
            "2m 0s"
        );
        assert_eq!(
            format_eta(now - Duration::from_secs(10), 1, Some(361)),
            "1h 0m"
        );
    }

    #[test]
    fn progress_matches_known_values() {
        assert_eq!(format_progress(0, None), "0.0 / ? MB");
        assert_eq!(
            format_progress(1_572_864, Some(10_485_760)),
            "1.5 / 10.0 MB"
        );
        assert_eq!(format_progress(1_048_576, None), "1.0 / ? MB");
    }

    #[test]
    fn relative_time_matches_known_values() {
        let now = Instant::now();
        assert_eq!(format_relative_time(now), "just now");
        assert_eq!(
            format_relative_time(now - Duration::from_secs(30)),
            "30 seconds ago"
        );
        assert_eq!(
            format_relative_time(now - Duration::from_secs(90)),
            "1 minute ago"
        );
        assert_eq!(
            format_relative_time(now - Duration::from_secs(120)),
            "2 minutes ago"
        );
        assert_eq!(
            format_relative_time(now - Duration::from_secs(7200)),
            "2 hours ago"
        );
        assert_eq!(
            format_relative_time(now - Duration::from_secs(86_400 * 3)),
            "3 days ago"
        );
    }
}
