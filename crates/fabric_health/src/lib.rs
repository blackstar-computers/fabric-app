//! War Room derivations — torch-free port of `web_app/src/lib/health.ts` + `runs.ts` (subset).

mod metrics;
mod output;
mod runs;

use fabric_types::{RunScalars, RunSeries};

pub use metrics::{legacy_panels, metric_panels_for_run, MetricPanel};
pub use output::{headline_of, headline_series_key, panels_for, run_is_lm};
pub use runs::{
    color_for_key, filter_runs, group_key, member_key, matches_kind, matches_search,
    pick_sparkline_key, sparkline_values, KindFilter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Warn,
    Bad,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthSignal {
    pub id: &'static str,
    pub label: &'static str,
    pub value: String,
    pub tone: Tone,
    pub hint: String,
}

pub fn is_lm_run(run: &RunScalars) -> bool {
    if run.runspec.as_ref().is_some_and(|rs| rs.substrate_is_lm()) {
        return true;
    }
    if run.metric.as_deref() == Some("ppl") {
        return true;
    }
    let meta = [
        run.name.as_str(),
        run.label.as_deref().unwrap_or(""),
        run.group.as_str(),
        run.fleet.as_str(),
        run.grid.as_deref().unwrap_or(""),
        run.metric.as_deref().unwrap_or(""),
        run.dataset.as_deref().unwrap_or(""),
    ]
    .join(" ");
    meta.contains("fabric_lm")
        || meta.contains("transformer_lm")
        || meta.contains("lm_wikitext")
        || meta.contains("wikitext")
        || meta.contains("tinystories")
}

pub fn worst_tone(signals: &[HealthSignal]) -> Tone {
    if signals.iter().any(|s| s.tone == Tone::Bad) {
        return Tone::Bad;
    }
    if signals.iter().any(|s| s.tone == Tone::Warn) {
        return Tone::Warn;
    }
    if signals.iter().any(|s| s.tone == Tone::Good) {
        return Tone::Good;
    }
    Tone::Neutral
}

pub fn tone_label(tone: Tone) -> &'static str {
    match tone {
        Tone::Good => "HEALTHY",
        Tone::Warn => "WATCH",
        Tone::Bad => "AT RISK",
        Tone::Neutral => "UNKNOWN",
    }
}

pub fn derive_signals(run: &RunScalars, series: Option<&RunSeries>) -> Vec<HealthSignal> {
    let live = matches!(
        run.status.as_deref(),
        Some("running") | Some("starting")
    );
    let lm = is_lm_run(run);
    let mut out = Vec::new();

    if let Some(s) = series {
        if let Some(gnorm) = s.latest("gnorm") {
            let (tone, hint) = if !gnorm.is_finite() || gnorm > 1e3 {
                (
                    Tone::Bad,
                    "gradient is exploding / non-finite — expect divergence",
                )
            } else if gnorm < 1e-6 {
                (
                    Tone::Bad,
                    "gradient has vanished (~0) — the model has stopped learning",
                )
            } else if gnorm < 1e-4 {
                (Tone::Warn, "gradient is very small — learning may be stalling")
            } else {
                (Tone::Good, "gradient magnitude is in a healthy band")
            };
            out.push(HealthSignal {
                id: "gnorm",
                label: "Grad norm",
                value: fmt_g(gnorm),
                tone,
                hint: hint.into(),
            });
        }

        let loss_key = if lm {
            if !s.nums("ce_val").is_empty() {
                "ce_val"
            } else {
                "ppl"
            }
        } else {
            "loss"
        };
        if let Some(trend) = rel_trend(s.nums(loss_key), 8) {
            if live {
                let flat = trend.abs() < 0.002;
                out.push(HealthSignal {
                    id: "plateau",
                    label: "Objective trend",
                    value: format!("{}{:.1}%", if trend >= 0.0 { "+" } else { "" }, trend * 100.0),
                    tone: if flat { Tone::Warn } else { Tone::Good },
                    hint: if flat {
                        format!(
                            "{loss_key} has barely moved over the last 8 probes — possible plateau"
                        )
                    } else {
                        format!(
                            "{loss_key} is still moving ({:.1}% over 8 probes)",
                            trend * 100.0
                        )
                    },
                });
            }
        }

        if let Some(util) = s.latest("gpu_util") {
            let (tone, hint) = if live && util < 10.0 {
                (
                    Tone::Bad,
                    "GPU is near-idle while the run claims to be training (stalled / phantom?)",
                )
            } else if live && util < 40.0 {
                (
                    Tone::Warn,
                    "GPU is under-utilized — input pipeline or sync may be the bottleneck",
                )
            } else {
                (Tone::Good, "GPU is busy")
            };
            out.push(HealthSignal {
                id: "gpu",
                label: "GPU util",
                value: format!("{util:.0}%"),
                tone,
                hint: hint.into(),
            });
        }

        if !lm {
            if let (Some(top1), Some(dream)) = (s.latest("top1"), s.latest("dream")) {
                let gap = top1 - dream;
                out.push(HealthSignal {
                    id: "openloop",
                    label: "Dream gap",
                    value: format!("{}{:.1} dB", if gap >= 0.0 { "" } else { "+" }, -gap),
                    tone: if gap > 3.0 { Tone::Warn } else { Tone::Good },
                    hint: if gap > 3.0 {
                        "open-loop dream rollout lags teacher-forced PSNR by >3 dB — overfitting the rollout".into()
                    } else {
                        "open-loop rollout is tracking teacher-forced PSNR".into()
                    },
                });
            }
        }

        if lm {
            if let (Some(ce_tr), Some(ce_val)) = (s.latest("ce_tr"), s.latest("ce_val")) {
                let gap = ce_val - ce_tr;
                out.push(HealthSignal {
                    id: "overfit",
                    label: "Val−train CE",
                    value: format!("{}{:.3}", if gap >= 0.0 { "+" } else { "" }, gap),
                    tone: if gap > 0.3 { Tone::Warn } else { Tone::Good },
                    hint: if gap > 0.3 {
                        "val cross-entropy is pulling away from train — overfitting".into()
                    } else {
                        "train/val gap is healthy".into()
                    },
                });
            }
        }
    }

    out
}

fn rel_trend(values: &[f64], win: usize) -> Option<f64> {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < win.max(4) {
        return None;
    }
    let tail = &finite[finite.len() - win..];
    let first = tail[0];
    let last = tail[tail.len() - 1];
    let denom = first.abs().max(1e-9);
    Some((last - first) / denom)
}

fn fmt_g(v: f64) -> String {
    let a = v.abs();
    if a != 0.0 && !(1e-3..1e4).contains(&a) {
        format!("{v:.1e}")
    } else if a < 1.0 {
        format!("{v:.3}")
    } else {
        format!("{v:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::RunScalars;
    use std::collections::HashMap;

    fn mk_series(gnorm: f64) -> RunSeries {
        RunSeries {
            pod: "f:n1".into(),
            name: "r".into(),
            epochs: vec![1, 2],
            metrics: HashMap::from([("gnorm".into(), vec![gnorm, gnorm])]),
        }
    }

    #[test]
    fn flags_vanishing_grad() {
        let run = RunScalars {
            status: Some("running".into()),
            ..Default::default()
        };
        let signals = derive_signals(&run, Some(&mk_series(1e-8)));
        assert!(signals.iter().any(|s| s.id == "gnorm" && s.tone == Tone::Bad));
    }

    #[test]
    fn lm_detects_ppl_metric() {
        let run = RunScalars {
            metric: Some("ppl".into()),
            ..Default::default()
        };
        assert!(is_lm_run(&run));
    }
}
