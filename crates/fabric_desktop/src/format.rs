//! Formatting helpers — mirrors `web_app/src/lib/format.ts` (subset).

use chrono::{DateTime, Local, TimeZone, Utc};

pub fn fmt_num(v: Option<f64>, digits: usize) -> String {
    match v {
        Some(n) if n.is_finite() => format!("{n:.prec$}", prec = digits),
        _ => "—".into(),
    }
}

pub fn fmt_epoch(current: Option<i64>, total: Option<i64>) -> String {
    match (current, total) {
        (Some(c), Some(t)) if t > 0 => format!("{c}/{t}"),
        (Some(c), _) => c.to_string(),
        _ => "—".into(),
    }
}

pub fn fmt_ago(ts: Option<f64>) -> String {
    let Some(ts) = ts else {
        return "—".into();
    };
    let secs = ts as i64;
    let dt: DateTime<Utc> = Utc
        .timestamp_opt(secs, 0)
        .single()
        .unwrap_or_else(Utc::now);
    let local: DateTime<Local> = dt.into();
    local.format("%b %d %H:%M").to_string()
}

pub fn fmt_eta(secs: Option<f64>) -> String {
    let Some(s) = secs else {
        return "—".into();
    };
    let s = s.round() as i64;
    if s < 60 {
        return format!("{s}s");
    }
    if s < 3600 {
        return format!("{}m", s / 60);
    }
    format!("{}h", s / 3600)
}

pub fn status_label(status: Option<&str>) -> &'static str {
    match status.unwrap_or("unknown") {
        "running" => "running",
        "starting" => "starting",
        "stopping" => "stopping",
        "idle" => "idle",
        other => {
            let _ = other;
            "unknown"
        }
    }
}
