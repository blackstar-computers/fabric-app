//! Output-driven panel selection — port of `web_app/src/lib/output.ts`.

use fabric_types::{RunMetricDef, RunScalars, RunSeries};
use std::collections::HashMap;

use crate::is_lm_run;
use crate::metrics::{legacy_panel_catalog, MetricPanel, SYSTEM_PANEL_IDS};

pub fn run_is_lm(run: &RunScalars) -> bool {
    if run.runspec.as_ref().is_some_and(|rs| rs.substrate_is_lm()) {
        return true;
    }
    is_lm_run(run)
}

pub fn headline_of(run: &RunScalars) -> Option<&fabric_types::Headline> {
    run.runspec
        .as_ref()
        .and_then(|rs| rs.output.as_ref())
        .and_then(|o| o.headline.as_ref())
        .filter(|h| !h.key.is_empty())
}

/// Headline metric key for sparklines / sorting (runspec first, legacy fallback).
pub fn headline_series_key(run: &RunScalars) -> String {
    headline_of(run)
        .map(|h| h.key.clone())
        .unwrap_or_else(|| {
            if run_is_lm(run) {
                "ppl".into()
            } else {
                "top1".into()
            }
        })
}

fn synth_panel(m: &RunMetricDef, lm: bool) -> MetricPanel {
    MetricPanel {
        id: m.key.clone(),
        title: m.label.clone().unwrap_or_else(|| m.key.clone()),
        series_key: m.key.clone(),
        lm,
        pct: m.pct,
        unit: None,
    }
}

fn legacy_by_series() -> HashMap<(bool, String), MetricPanel> {
    legacy_panel_catalog()
        .iter()
        .map(|p| ((p.lm, p.series_key.into()), p.into_panel()))
        .collect()
}

fn insert_system_panels(mut out: Vec<MetricPanel>) -> Vec<MetricPanel> {
    let seen: std::collections::HashSet<_> = out.iter().map(|p| p.id.clone()).collect();
    let sys: Vec<_> = legacy_panel_catalog()
        .iter()
        .filter(|p| SYSTEM_PANEL_IDS.contains(&p.id) && !seen.contains(p.id))
        .map(|p| p.into_panel())
        .collect();
    if sys.is_empty() {
        return out;
    }
    let at = out
        .iter()
        .position(|p| p.id == "gnorm")
        .map(|i| i + 1)
        .unwrap_or(out.len());
    out.splice(at..at, sys);
    out
}

/// Panels for one run in output order (headline first). Mirrors `panelsFor`.
pub fn panels_for(run: &RunScalars) -> Vec<MetricPanel> {
    let lm = run_is_lm(run);
    let legacy = legacy_by_series();
    let metrics = run
        .runspec
        .as_ref()
        .and_then(|rs| rs.output.as_ref())
        .map(|o| o.metrics.as_slice());

    let Some(metrics) = metrics.filter(|m| !m.is_empty()) else {
        let base: Vec<_> = legacy_panel_catalog()
            .iter()
            .filter(|p| p.lm == lm)
            .map(|p| p.into_panel())
            .collect();
        return insert_system_panels(base);
    };

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for m in metrics {
        let panel = legacy
            .get(&(lm, m.key.clone()))
            .cloned()
            .unwrap_or_else(|| synth_panel(m, lm));
        if !seen.insert(panel.id.clone()) {
            continue;
        }
        out.push(panel);
    }
    insert_system_panels(out)
}

fn has_finite(series: &RunSeries, key: &str) -> bool {
    series.nums(key).iter().any(|v| v.is_finite())
}

/// War Room panels: output contract + system GPU, gated on real series data.
pub fn metric_panels_for_run(run: &RunScalars, series: Option<&RunSeries>) -> Vec<MetricPanel> {
    let Some(s) = series else {
        return Vec::new();
    };
    panels_for(run)
        .into_iter()
        .filter(|p| has_finite(s, &p.series_key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::{RunOutput, RunSpecEnvelope};

    fn mk_run(p: RunScalars) -> RunScalars {
        p
    }

    fn diff_run() -> RunScalars {
        mk_run(RunScalars {
            pod: "f:n1".into(),
            name: "r_n1".into(),
            group: "r".into(),
            fleet: "f".into(),
            runspec: Some(RunSpecEnvelope {
                substrate_kind: Some("canvas".into()),
                output: Some(RunOutput {
                    headline: Some(fabric_types::Headline {
                        key: "diff".into(),
                        goal: Some("max".into()),
                        label: Some("DIFF PSNR (dB)".into()),
                    }),
                    metrics: vec![
                        RunMetricDef {
                            key: "diff".into(),
                            label: Some("DIFF PSNR (dB)".into()),
                            ..Default::default()
                        },
                        RunMetricDef {
                            key: "loss".into(),
                            lower_better: true,
                            ..Default::default()
                        },
                        RunMetricDef {
                            key: "gnorm".into(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    #[test]
    fn panels_include_system_gpu() {
        let ids: Vec<_> = panels_for(&diff_run()).into_iter().map(|p| p.id).collect();
        assert!(ids.contains(&"gpu_util".to_string()));
        assert!(ids.contains(&"gpu_mem".to_string()));
    }

    #[test]
    fn legacy_lm_includes_gpu_panels() {
        let run = mk_run(RunScalars {
            metric: Some("ppl".into()),
            ..Default::default()
        });
        let ids: Vec<_> = panels_for(&run).into_iter().map(|p| p.id).collect();
        assert!(ids.contains(&"gpu_util".to_string()));
        assert!(ids.contains(&"gpu_mem".to_string()));
        assert!(ids.iter().any(|id| id == "ppl"));
    }

    #[test]
    fn headline_from_runspec() {
        let run = diff_run();
        assert_eq!(headline_series_key(&run), "diff");
    }

    #[test]
    fn synth_panel_for_novel_key() {
        let run = mk_run(RunScalars {
            runspec: Some(RunSpecEnvelope {
                output: Some(RunOutput {
                    metrics: vec![RunMetricDef {
                        key: "fid".into(),
                        label: Some("FID".into()),
                        lower_better: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        });
        let p = panels_for(&run).into_iter().find(|p| p.id == "fid").expect("fid");
        assert_eq!(p.series_key, "fid");
        assert_eq!(p.title, "FID");
    }
}
