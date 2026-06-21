//! Overview table helpers — search, grouping, sparklines.

use fabric_types::RunScalars;

use crate::output::headline_series_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Recon,
    Lm,
}

impl KindFilter {
    pub fn label(self) -> &'static str {
        match self {
            KindFilter::All => "ALL",
            KindFilter::Recon => "RECON",
            KindFilter::Lm => "LM",
        }
    }
}

const GSEP: char = '\u{0001}';

pub fn group_key(run: &RunScalars) -> String {
    format!(
        "{}{GSEP}{}",
        run.fleet,
        if run.group.is_empty() {
            run.name.as_str()
        } else {
            run.group.as_str()
        }
    )
}

pub fn member_key(run: &RunScalars) -> String {
    format!("{}{GSEP}{}", run.pod, run.name)
}

pub fn matches_kind(run: &RunScalars, kind: KindFilter) -> bool {
    match kind {
        KindFilter::All => true,
        KindFilter::Lm => super::run_is_lm(run),
        KindFilter::Recon => !super::run_is_lm(run),
    }
}

pub fn matches_search(run: &RunScalars, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    [run.name.as_str(), run.pod.as_str(), run.group.as_str(), run.fleet.as_str()]
        .iter()
        .any(|s| s.to_lowercase().contains(&q))
}

pub fn filter_runs<'a>(
    runs: &'a [RunScalars],
    kind: KindFilter,
    query: &str,
) -> Vec<&'a RunScalars> {
    runs.iter()
        .filter(|r| matches_kind(r, kind))
        .filter(|r| matches_search(r, query))
        .collect()
}

/// Sparkline uses the headline metric only (matches web `Leaderboard.sparklineValues`).
pub fn pick_sparkline_key(run: &RunScalars) -> String {
    headline_series_key(run)
}

/// Last N finite samples for a tiny trend line (port of `Leaderboard.sparklineValues`).
pub fn sparkline_values(series: &fabric_types::RunSeries, key: &str, window: usize) -> Vec<f64> {
    series
        .nums(key)
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(window)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

const PALETTE: [u32; 16] = [
    0x4f8cff, 0xff7a45, 0x36c98d, 0xc879ff, 0xffd23f, 0xff5c8a, 0x28c0c8, 0x9ae24b, 0xff6b6b,
    0x845ef7, 0x20c997, 0xff922b, 0x51cf66, 0x339af0, 0xf06595, 0x12b886,
];

/// Stable accent from a group key (port of `runs.colorForKey`).
pub fn color_for_key(key: &str) -> u32 {
    let mut h: i32 = 0;
    for b in key.bytes() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(b));
    }
    PALETTE[(h.unsigned_abs() as usize) % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::RunScalars;

    #[test]
    fn group_key_uses_group_when_set() {
        let run = RunScalars {
            fleet: "copytest".into(),
            group: "sweep_a".into(),
            name: "sweep_a_n1".into(),
            ..Default::default()
        };
        assert_eq!(group_key(&run), "copytest\u{0001}sweep_a");
    }

    #[test]
    fn kind_filter_lm() {
        let lm = RunScalars {
            metric: Some("ppl".into()),
            ..Default::default()
        };
        let recon = RunScalars::default();
        assert!(matches_kind(&lm, KindFilter::Lm));
        assert!(!matches_kind(&recon, KindFilter::Lm));
        assert!(matches_kind(&recon, KindFilter::Recon));
    }

    #[test]
    fn search_matches_pod() {
        let run = RunScalars {
            name: "foo".into(),
            pod: "f:n7".into(),
            ..Default::default()
        };
        assert!(matches_search(&run, "n7"));
        assert!(!matches_search(&run, "n8"));
    }

    #[test]
    fn pick_sparkline_uses_headline_only() {
        use fabric_types::{RunOutput, RunSpecEnvelope};
        let run = RunScalars {
            runspec: Some(RunSpecEnvelope {
                output: Some(RunOutput {
                    headline: Some(fabric_types::Headline {
                        key: "diff".into(),
                        goal: Some("max".into()),
                        label: None,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(pick_sparkline_key(&run), "diff");
        assert_eq!(pick_sparkline_key(&RunScalars::default()), "top1");
    }
}
