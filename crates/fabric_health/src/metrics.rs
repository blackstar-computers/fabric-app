//! Metric panel catalog (mirrors `web_app/src/lib/runs.ts` METRIC_PANELS).

use fabric_types::{RunScalars, RunSeries};
use crate::is_lm_run;

#[derive(Debug, Clone, Copy)]
pub struct MetricPanel {
    pub id: &'static str,
    pub title: &'static str,
    pub series_key: &'static str,
    pub lm: bool,
    pub pct: bool,
    pub unit: Option<&'static str>,
}

pub const METRIC_PANELS: &[MetricPanel] = &[
    MetricPanel {
        id: "ppl",
        title: "Perplexity",
        series_key: "ppl",
        lm: true,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "ce",
        title: "Cross-entropy",
        series_key: "ce_val",
        lm: true,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "lm_acc",
        title: "Next-token accuracy",
        series_key: "top1",
        lm: true,
        pct: true,
        unit: None,
    },
    MetricPanel {
        id: "score",
        title: "Validation score",
        series_key: "top1",
        lm: false,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "loss",
        title: "Train loss",
        series_key: "loss",
        lm: false,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "lr",
        title: "Learning rate",
        series_key: "lr",
        lm: false,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "dream",
        title: "Dream rollout PSNR",
        series_key: "dream",
        lm: false,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "gnorm",
        title: "Gradient norm",
        series_key: "gnorm",
        lm: false,
        pct: false,
        unit: None,
    },
    MetricPanel {
        id: "gpu_util",
        title: "GPU utilization",
        series_key: "gpu_util",
        lm: false,
        pct: false,
        unit: Some("%"),
    },
    MetricPanel {
        id: "gpu_mem",
        title: "GPU memory",
        series_key: "gpu_mem",
        lm: false,
        pct: true,
        unit: None,
    },
];

pub fn metric_panels_for_run(run: &RunScalars, series: Option<&RunSeries>) -> Vec<&'static MetricPanel> {
    let lm = is_lm_run(run);
    let Some(s) = series else {
        return Vec::new();
    };
    METRIC_PANELS
        .iter()
        .filter(|p| p.lm == lm && !s.nums(p.series_key).is_empty())
        .collect()
}
