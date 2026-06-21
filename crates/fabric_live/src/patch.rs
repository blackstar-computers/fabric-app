//! SSE v2 cache patching — port of `web_app/src/lib/ssePatch.ts`.

use fabric_types::{RunScalars, RunsSummary, SseRunEvent};
use std::collections::HashMap;

const DEFAULT_MAX_POINTS: usize = 600;

/// Patch the matching run row in place. Returns `true` when the run was found and updated.
pub fn patch_summary(summary: &mut RunsSummary, ev: &SseRunEvent) -> bool {
    let Some(idx) = summary
        .runs
        .iter()
        .position(|r| r.pod == ev.pod && r.name == ev.run)
    else {
        return false;
    };

    let row = &mut summary.runs[idx];
    apply_scalars(row, &ev.scalars);
    if let Some(headline) = &ev.headline {
        if headline.best {
            row.best = Some(headline.value);
        }
    }
    true
}

fn apply_scalars(row: &mut RunScalars, scalars: &HashMap<String, serde_json::Value>) {
    for (key, val) in scalars {
        if val.is_null() {
            continue;
        }
        match key.as_str() {
            "status" => row.status = val.as_str().map(str::to_string),
            "metric" => row.metric = val.as_str().map(str::to_string),
            "label" => row.label = val.as_str().map(str::to_string),
            "grid" => row.grid = val.as_str().map(str::to_string),
            "dataset" => row.dataset = val.as_str().map(str::to_string),
            "last_epoch" => row.last_epoch = val.as_i64(),
            "n" => row.n = val.as_i64(),
            "eta_sec" => row.eta_sec = val.as_f64(),
            "sec_per_epoch" => row.sec_per_epoch = val.as_f64(),
            "last_top1" => row.last_top1 = val.as_f64(),
            "last_tgt" => row.last_tgt = val.as_f64(),
            "last_lr" => row.last_lr = val.as_f64(),
            "best" => row.best = val.as_f64(),
            "total_epochs" => row.total_epochs = val.as_i64(),
            "created" => row.created = val.as_f64().or_else(|| val.as_i64().map(|n| n as f64)),
            "sweep" => row.sweep = val.as_str().map(str::to_string),
            _ => {
                row.extra.insert(key.clone(), val.clone());
            }
        }
    }
}

/// Append one SSE point onto a metric series map (`epoch` + metric keys).
pub fn append_point(
    epochs: &mut Vec<i64>,
    metrics: &mut HashMap<String, Vec<f64>>,
    point: &HashMap<String, serde_json::Value>,
    cap: usize,
) {
    let Some(epoch) = point.get("epoch").and_then(|v| v.as_i64()) else {
        return;
    };
    epochs.push(epoch);
    trim_vec(epochs, cap);

    for (key, val) in point {
        if key == "epoch" {
            continue;
        }
        if let Some(entry) = metrics.get_mut(key) {
            if let Some(n) = val.as_f64().or_else(|| val.as_i64().map(|v| v as f64)) {
                entry.push(n);
                trim_vec(entry, cap);
            }
        }
    }
}

fn trim_vec<T>(arr: &mut Vec<T>, cap: usize) {
    if cap > 0 && arr.len() > cap {
        let drop = arr.len() - cap;
        arr.drain(0..drop);
    }
}

pub fn default_max_points() -> usize {
    DEFAULT_MAX_POINTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::{GpuSummary, SseHeadline};

    fn mk_run(best: f64, epoch: i64) -> RunScalars {
        RunScalars {
            pod: "f:n1".into(),
            name: "r_n1".into(),
            group: "r".into(),
            fleet: "f".into(),
            best: Some(best),
            last_epoch: Some(epoch),
            status: Some("running".into()),
            ..Default::default()
        }
    }

    fn mk_summary(runs: Vec<RunScalars>) -> RunsSummary {
        RunsSummary {
            runs,
            gpus: GpuSummary::default(),
            ..Default::default()
        }
    }

    fn run_event(scalars: HashMap<String, serde_json::Value>, headline: Option<SseHeadline>) -> SseRunEvent {
        SseRunEvent {
            kind: "run".into(),
            pod: "f:n1".into(),
            run: "r_n1".into(),
            v: Some(2),
            ts: None,
            headline,
            scalars,
            point: HashMap::new(),
        }
    }

    #[test]
    fn patch_updates_best_and_scalars() {
        let mut summary = mk_summary(vec![mk_run(60.0, 10)]);
        let mut scalars = HashMap::new();
        scalars.insert("last_epoch".into(), serde_json::json!(12));
        scalars.insert("eta_sec".into(), serde_json::json!(3600));
        scalars.insert("status".into(), serde_json::json!("running"));
        let headline = SseHeadline {
            key: "top1".into(),
            value: 64.47,
            best: true,
        };
        assert!(patch_summary(
            &mut summary,
            &run_event(scalars, Some(headline))
        ));
        let row = &summary.runs[0];
        assert_eq!(row.best, Some(64.47));
        assert_eq!(row.last_epoch, Some(12));
        assert_eq!(row.eta_sec, Some(3600.0));
    }

    #[test]
    fn patch_updates_total_epochs_and_created() {
        let mut summary = mk_summary(vec![mk_run(60.0, 10)]);
        let mut scalars = HashMap::new();
        scalars.insert("total_epochs".into(), serde_json::json!(500));
        scalars.insert("created".into(), serde_json::json!(1718880000));
        scalars.insert("sweep".into(), serde_json::json!("lr_sweep"));
        assert!(patch_summary(&mut summary, &run_event(scalars, None)));
        let row = &summary.runs[0];
        assert_eq!(row.total_epochs, Some(500));
        assert_eq!(row.created, Some(1718880000.0));
        assert_eq!(row.sweep.as_deref(), Some("lr_sweep"));
    }

    #[test]
    fn patch_returns_false_for_unknown_run() {
        let mut summary = mk_summary(vec![mk_run(1.0, 1)]);
        let ev = SseRunEvent {
            kind: "run".into(),
            pod: "f:n2".into(),
            run: "other".into(),
            v: Some(2),
            ts: None,
            headline: None,
            scalars: HashMap::new(),
            point: HashMap::new(),
        };
        assert!(!patch_summary(&mut summary, &ev));
    }
}
