//! Build POST /api/step drive bodies — port of `buildDrive` in `viz.ts`.

use fabric_types::{DriveSpec, VizLoadMeta};

pub struct BuildDriveOpts<'a> {
    pub source: &'a str,
    pub idx: i64,
    pub run_dataset: Option<&'a str>,
    pub has_dataset: bool,
    pub pixel_b64: Option<&'a str>,
}

/// Build the POST /api/step drive body. Run dataset uses type `"image"`, not `"dataset"`.
pub fn build_drive(opts: BuildDriveOpts<'_>) -> DriveSpec {
    if let Some(data) = opts.pixel_b64.filter(|s| !s.is_empty()) {
        return DriveSpec::Pixels {
            data: data.to_string(),
        };
    }
    let ds = opts.run_dataset.unwrap_or("");
    if opts.has_dataset && !ds.is_empty() && opts.source == ds {
        return DriveSpec::Image { idx: opts.idx };
    }
    DriveSpec::Builtin {
        dataset: opts.source.to_string(),
        idx: opts.idx,
    }
}

/// Default input source after load — port of `defaultVizSource` in `viz.ts`.
/// Prefers the run's own `.npy` dataset on the box, otherwise the first built-in.
pub fn default_viz_source(meta: &VizLoadMeta) -> String {
    if meta.has_dataset.unwrap_or(false) {
        if let Some(ds) = meta.dataset.as_ref().filter(|s| !s.is_empty()) {
            return ds.clone();
        }
    }
    meta.builtins
        .first()
        .cloned()
        .unwrap_or_else(|| "mnist".to_string())
}

/// Selectable drive sources for the source picker — port of `vizSources` in `viz.ts`.
/// Run dataset (when present) first, then the built-ins, de-duplicated.
pub fn viz_sources(meta: &VizLoadMeta) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if meta.has_dataset.unwrap_or(false) {
        if let Some(ds) = meta.dataset.as_ref().filter(|s| !s.is_empty()) {
            out.push(ds.clone());
        }
    }
    for b in &meta.builtins {
        if !out.contains(b) {
            out.push(b.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_image_for_run_dataset() {
        let drive = build_drive(BuildDriveOpts {
            source: "clip.npy",
            idx: 3,
            run_dataset: Some("clip.npy"),
            has_dataset: true,
            pixel_b64: None,
        });
        assert!(matches!(drive, DriveSpec::Image { idx: 3 }));
    }

    #[test]
    fn uses_builtin_for_mnist() {
        let drive = build_drive(BuildDriveOpts {
            source: "mnist",
            idx: 7,
            run_dataset: Some("clip.npy"),
            has_dataset: true,
            pixel_b64: None,
        });
        assert!(matches!(
            drive,
            DriveSpec::Builtin {
                ref dataset,
                idx: 7
            } if dataset == "mnist"
        ));
    }

    #[test]
    fn default_source_prefers_run_dataset() {
        let meta = VizLoadMeta {
            has_dataset: Some(true),
            dataset: Some("clip.npy".into()),
            builtins: vec!["mnist".into(), "cifar".into()],
            ..Default::default()
        };
        assert_eq!(default_viz_source(&meta), "clip.npy");
        assert_eq!(viz_sources(&meta), vec!["clip.npy", "mnist", "cifar"]);
    }

    #[test]
    fn default_source_falls_back_to_first_builtin() {
        let meta = VizLoadMeta {
            has_dataset: Some(false),
            dataset: Some("clip.npy".into()),
            builtins: vec!["fashion".into()],
            ..Default::default()
        };
        assert_eq!(default_viz_source(&meta), "fashion");
        assert_eq!(viz_sources(&meta), vec!["fashion"]);
    }

    #[test]
    fn default_source_empty_meta_is_mnist() {
        let meta = VizLoadMeta::default();
        assert_eq!(default_viz_source(&meta), "mnist");
        assert!(viz_sources(&meta).is_empty());
    }

    #[test]
    fn prefers_pixels_when_upload_present() {
        let drive = build_drive(BuildDriveOpts {
            source: "mnist",
            idx: 0,
            run_dataset: None,
            has_dataset: false,
            pixel_b64: Some("abc123"),
        });
        assert!(matches!(
            drive,
            DriveSpec::Pixels { ref data } if data == "abc123"
        ));
    }
}
