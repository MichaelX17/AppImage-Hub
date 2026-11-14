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
}
