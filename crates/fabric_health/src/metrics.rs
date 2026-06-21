//! Metric panel catalog (mirrors `web_app/src/lib/runs.ts` METRIC_PANELS).

pub const SYSTEM_PANEL_IDS: &[&str] = &["gpu_util", "gpu_mem"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricPanel {
    pub id: String,
    pub title: String,
    pub series_key: String,
    pub lm: bool,
    pub pct: bool,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StaticMetricPanel {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) series_key: &'static str,
    pub(crate) lm: bool,
    pub(crate) pct: bool,
    pub(crate) unit: Option<&'static str>,
}

impl StaticMetricPanel {
    pub(crate) fn into_panel(self) -> MetricPanel {
        MetricPanel {
            id: self.id.into(),
            title: self.title.into(),
            series_key: self.series_key.into(),
            lm: self.lm,
            pct: self.pct,
            unit: self.unit.map(str::to_string),
        }
    }
}

const LEGACY_PANELS: &[StaticMetricPanel] = &[
    StaticMetricPanel {
        id: "ppl",
        title: "Perplexity",
        series_key: "ppl",
        lm: true,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "ce",
        title: "Cross-entropy",
        series_key: "ce_val",
        lm: true,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "lm_acc",
        title: "Next-token accuracy",
        series_key: "top1",
        lm: true,
        pct: true,
        unit: None,
    },
    StaticMetricPanel {
        id: "score",
        title: "Validation score",
        series_key: "top1",
        lm: false,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "loss",
        title: "Train loss",
        series_key: "loss",
        lm: false,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "lr",
        title: "Learning rate",
        series_key: "lr",
        lm: false,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "dream",
        title: "Dream rollout PSNR",
        series_key: "dream",
        lm: false,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "gnorm",
        title: "Gradient norm",
        series_key: "gnorm",
        lm: false,
        pct: false,
        unit: None,
    },
    StaticMetricPanel {
        id: "gpu_util",
        title: "GPU utilization",
        series_key: "gpu_util",
        lm: false,
        pct: false,
        unit: Some("%"),
    },
    StaticMetricPanel {
        id: "gpu_mem",
        title: "GPU memory",
        series_key: "gpu_mem",
        lm: false,
        pct: true,
        unit: None,
    },
];

pub fn legacy_panels() -> Vec<MetricPanel> {
    LEGACY_PANELS.iter().copied().map(StaticMetricPanel::into_panel).collect()
}

pub fn legacy_panel_catalog() -> &'static [StaticMetricPanel] {
    LEGACY_PANELS
}

pub use crate::output::metric_panels_for_run;
