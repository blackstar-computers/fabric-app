//! Checkpoint eligibility gates — port of `viz.ts` + `output.ts` viz eligibility.

use fabric_types::{CheckpointFile, CheckpointRun, RunScalars};

const SUPPORTED_CKPT_KINDS: &[&str] = &["canvas"];

fn pod_tag(pod: &str) -> &str {
    pod.rsplit(':').next().unwrap_or(pod)
}

fn flat_ckpt_match(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    for ext in [".pt", ".pth", ".ckpt"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.is_empty()
                || stem.ends_with(".best")
                || stem.ends_with(".last")
                || !stem.contains('.');
        }
        for suffix in [".best", ".last"] {
            if let Some(stem) = lower.strip_suffix(&format!("{ext}{suffix}")) {
                return !stem.is_empty();
            }
        }
    }
    false
}

fn gossip_checkpoint(filename: &str) -> bool {
    filename.starts_with("w_")
        || filename.starts_with("t_")
        || filename.starts_with("s_")
        || filename.starts_with("w_consensus")
        || filename.starts_with("t_consensus")
}

fn is_lm_run(run: &RunScalars) -> bool {
    if run
        .runspec
        .as_ref()
        .is_some_and(|rs| rs.substrate_kind.as_deref() == Some("lm"))
    {
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
        || meta.contains("_lm_")
        || meta.contains("_lm.")
        || meta.ends_with("_lm")
}

fn viz_eligible_by_output(run: &RunScalars) -> bool {
    let ck = run
        .runspec
        .as_ref()
        .and_then(|rs| rs.output.as_ref())
        .and_then(|o| o.checkpoint_kind.as_deref());
    if let Some(ck) = ck {
        return SUPPORTED_CKPT_KINDS.contains(&ck);
    }
    let sk = run
        .runspec
        .as_ref()
        .and_then(|rs| rs.substrate_kind.as_deref());
    if matches!(sk, Some("lm") | Some("polar")) {
        return false;
    }
    if sk == Some("canvas") {
        return true;
    }
    if run.runspec.is_some() {
        return false;
    }
    !is_lm_run(run)
}

fn strip_ckpt_ext(filename: &str) -> String {
    let lower = filename.to_ascii_lowercase();
    for ext in [".pt.best", ".pth.best", ".ckpt.best", ".pt.last", ".pth.last", ".ckpt.last"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    for ext in [".pt", ".pth", ".ckpt"] {
        if let Some(stem) = lower.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    lower
}

fn is_orphan_visualizer_checkpoint(run: &str, filename: &str, pod: &str) -> bool {
    let tag = pod_tag(pod);
    let stem = strip_ckpt_ext(filename);
    if stem.ends_with(&format!("_{tag}")) {
        return true;
    }
    if run.ends_with(&format!("_{tag}")) {
        return true;
    }
    if stem == format!("{run}_{tag}") {
        return true;
    }
    filename == "best.pt" || epoch_ckpt_match(filename)
}

fn epoch_ckpt_match(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    if !lower.starts_with("epoch_") {
        return false;
    }
    flat_ckpt_match(filename)
}

/// Filename/kind the infer-box viewer can load (flat saves — not gossip or topology).
pub fn is_visualizer_checkpoint_file(filename: &str, kind: Option<&str>) -> bool {
    if matches!(kind, Some("topology") | Some("meta")) {
        return false;
    }
    if filename == "topology.pt" || filename.ends_with(".json") {
        return false;
    }
    if gossip_checkpoint(filename) || filename.contains("consensus") {
        return false;
    }
    if filename == "best.pt" {
        return true;
    }
    if epoch_ckpt_match(filename) {
        return true;
    }
    flat_ckpt_match(filename)
}

/// File + run contract gate for the Visualizer list (canvas recon/classify only).
pub fn viz_eligible_checkpoint(
    ck: &CheckpointRun,
    file: &CheckpointFile,
    run_row: Option<&RunScalars>,
) -> bool {
    if !is_visualizer_checkpoint_file(&file.filename, file.kind.as_deref()) {
        return false;
    }
    if let Some(run) = run_row {
        return viz_eligible_by_output(run);
    }
    is_orphan_visualizer_checkpoint(&ck.run, &file.filename, &ck.pod)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::{RunOutput, RunSpecEnvelope};

    fn mk_ck() -> CheckpointRun {
        CheckpointRun {
            fleet: "f".into(),
            pod: "n1".into(),
            run: "mnist_recon_n1".into(),
            n_files: 1,
            bytes: 1,
            files: vec![],
            ..Default::default()
        }
    }

    fn canvas_run() -> RunScalars {
        RunScalars {
            pod: "f:n1".into(),
            name: "mnist_recon_n1".into(),
            group: "mnist_recon".into(),
            fleet: "f".into(),
            runspec: Some(RunSpecEnvelope {
                substrate_kind: Some("canvas".into()),
                output: Some(RunOutput {
                    checkpoint_kind: Some("canvas".into()),
                    kind: Some("canvas.recon".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn accepts_agent_flat_saves() {
        assert!(is_visualizer_checkpoint_file("mnist_recon_n1.pt", None));
        assert!(is_visualizer_checkpoint_file("mnist_recon_n1.pt.best", None));
        assert!(is_visualizer_checkpoint_file("best.pt", None));
        assert!(is_visualizer_checkpoint_file("epoch_3.pt", None));
    }

    #[test]
    fn rejects_gossip_and_topology() {
        assert!(!is_visualizer_checkpoint_file("w_n1.pt", None));
        assert!(!is_visualizer_checkpoint_file("topology.pt", Some("topology")));
        assert!(!is_visualizer_checkpoint_file("meta.json", Some("meta")));
    }

    #[test]
    fn orphan_ckpt_before_metrics_ingest() {
        let ck = mk_ck();
        assert!(viz_eligible_checkpoint(
            &ck,
            &CheckpointFile {
                filename: "mnist_recon_n1.pt".into(),
                ..Default::default()
            },
            None,
        ));
        assert!(!viz_eligible_checkpoint(
            &ck,
            &CheckpointFile {
                filename: "w_n1.pt".into(),
                ..Default::default()
            },
            None,
        ));
    }

    #[test]
    fn requires_canvas_runspec_when_run_present() {
        let ck = mk_ck();
        let file = CheckpointFile {
            filename: "mnist_recon_n1.pt".into(),
            ..Default::default()
        };
        assert!(viz_eligible_checkpoint(&ck, &file, Some(&canvas_run())));

        let polar = RunScalars {
            runspec: Some(RunSpecEnvelope {
                substrate_kind: Some("polar".into()),
                output: Some(RunOutput {
                    checkpoint_kind: Some("polar".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!viz_eligible_checkpoint(&ck, &file, Some(&polar)));
    }
}
