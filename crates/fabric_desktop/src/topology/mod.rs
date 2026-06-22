//! Topology explorer — run picker, substrate canvas, controls, timeline.
//!
//! `TopologyView` owns deck + viz state and the network channel; rendering is split
//! across [`picker`](picker), [`gallery`](gallery), [`explorer`](explorer),
//! [`controls`](controls), [`timeline`](timeline), and [`previews`](previews).

mod controls;
mod explorer;
mod gallery;
mod images;
mod picker;
mod previews;
mod timeline;

use crate::network::{viz_status_error, viz_status_ready, NetworkCommand, TopologyMsg};
use crate::search_input::SearchInput;
use crate::theme::Theme;
use crate::topology::controls::controls_rail;
use crate::topology::explorer::explorer_canvas;
use crate::topology::gallery::gallery_pane;
use crate::topology::picker::{picker_rail, PickerRowData};
use crate::topology::timeline::timeline_bar;
use fabric_types::{
    CheckpointsResp, ReconStepResp, RunScalars, RunsSummary, TopoManifestResp, VizGalleryResp,
    VizLoadMeta, VizOpenResp, VizStepRequest,
};
use crate::topology::images::{image_b64_to_render_image, image_bytes_to_render_image};
use fabric_viz::{
    build_drive, decode_quant8_frame, default_viz_source, gallery_dataset, parse_fab1,
    regions_from_fab_spans, viz_eligible_checkpoint, BuildDriveOpts,
};
use futures::channel::mpsc::UnboundedSender;
use gpui::{div, prelude::*, px, Context, Entity, Render, RenderImage, SharedString, Window};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

type RunKey = (String, String);

/// How many dataset thumbnails to request per gallery batch.
const GALLERY_COUNT: u32 = 256;
/// Requested thumbnail edge in pixels (matches the 48×48 grid cells).
const GALLERY_THUMB_PX: u32 = 48;
/// Input preview fetch size when the gallery batch does not cover the active idx.
const INPUT_PREVIEW_PX: u32 = 96;
/// Debounce search keystrokes so the heavy canvas/gallery panes are not repainted every char.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
/// Interval between frames when timeline playback is active.
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Structure,
    LiveFlow,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GridHitInfo {
    pub origin_x: f32,
    pub origin_y: f32,
    pub width: f32,
    pub height: f32,
    pub cols: u32,
    pub total_rows: u32,
}

/// Identity of a rasterized substrate bitmap. While every field is unchanged the cached
/// [`RenderImage`] can be re-blitted (one `paint_image`) instead of rebuilt — hover/select
/// repaints never touch it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GridImageKey {
    pub scrub_tick: usize,
    pub display_mode: DisplayMode,
    pub cols: u32,
    pub rows: u32,
    pub width_bucket: u32,
    pub height_bucket: u32,
}

pub(crate) struct GridImageCache {
    pub key: GridImageKey,
    pub image: Arc<RenderImage>,
}

/// Shared raster cache: the canvas paint callback writes/reads, render reads nothing.
pub(crate) type GridPaintCache = Rc<RefCell<Option<GridImageCache>>>;

pub struct TopologyView {
    pub(crate) summary: Option<RunsSummary>,
    pub(crate) checkpoints: Option<CheckpointsResp>,
    pub(crate) manifest: Option<TopoManifestResp>,
    pub(crate) selected: Option<RunKey>,
    pub(crate) selected_file: Option<String>,
    pub(crate) selected_fleet: Option<String>,
    pub(crate) viz_meta: Option<VizLoadMeta>,
    pub(crate) step_resp: Option<ReconStepResp>,
    pub(crate) decoded_frames: Vec<Vec<f32>>,
    pub(crate) scrub_tick: usize,
    pub(crate) display_mode: DisplayMode,
    pub(crate) topo_data: Option<fabric_types::TopoDataInMemory>,
    pub(crate) selected_cell: Option<usize>,
    pub(crate) input_source: String,
    pub(crate) input_idx: u32,
    pub(crate) ticks: u32,
    /// Dataset thumbnail batch for the gallery pane (`/viz/default/api/gallery`).
    pub(crate) gallery: Option<VizGalleryResp>,
    pub(crate) gallery_loading: bool,
    pub(crate) loading: bool,
    pub(crate) stepping: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) refreshing: bool,
    pub(crate) search: Entity<SearchInput>,
    pub(crate) search_query: String,
    /// Filtered run rows for the left rail, cached so the `uniform_list` only
    /// virtualizes precomputed data instead of rebuilding every frame.
    pub(crate) picker_rows: Vec<PickerRowData>,
    pub(crate) grid_hit: Rc<RefCell<Option<GridHitInfo>>>,
    /// Rasterized substrate, rebuilt only when [`GridImageKey`] changes.
    pub(crate) grid_cache: GridPaintCache,
    /// Cell under the cursor as `(row, col, activation)` for the floating readout.
    pub(crate) hover_cell: Option<(u32, u32, f32)>,
    /// Decoded gallery / input / recon images keyed by `gal:{idx}`, `input:{idx}`, `recon:{tick}`.
    pub(crate) image_cache: HashMap<String, Arc<RenderImage>>,
    /// Monotonic counter so debounced search tasks only apply the latest query.
    search_debounce_gen: u64,
    /// Auto-advance scrub tick through decoded frames (timeline ▶ PLAY).
    pub(crate) playing: bool,
    play_gen: u64,
    cmd_tx: Option<UnboundedSender<NetworkCommand>>,
    pending_ckpt: Option<String>,
    /// Retry viz open with `force: true` once when the box session is stale (web visualizer parity).
    viz_force_retry: bool,
}

impl TopologyView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(SearchInput::new);
        cx.observe(&search, |this, search, cx| {
            let q = search.read(cx).query().to_string();
            if this.search_query == q {
                return;
            }
            this.search_query = q;
            this.search_debounce_gen = this.search_debounce_gen.wrapping_add(1);
            let gen = this.search_debounce_gen;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SEARCH_DEBOUNCE).await;
                let _ = this.update(cx, |this, cx| {
                    if this.search_debounce_gen == gen {
                        this.rebuild_picker_rows();
                        cx.notify();
                    }
                });
            })
            .detach();
        })
        .detach();

        Self {
            summary: None,
            checkpoints: None,
            manifest: None,
            selected: None,
            selected_file: None,
            selected_fleet: None,
            viz_meta: None,
            step_resp: None,
            decoded_frames: Vec::new(),
            scrub_tick: 0,
            display_mode: DisplayMode::Structure,
            topo_data: None,
            selected_cell: None,
            input_source: "mnist".into(),
            input_idx: 0,
            ticks: 16,
            gallery: None,
            gallery_loading: false,
            loading: false,
            stepping: false,
            error: None,
            refreshing: false,
            search,
            search_query: String::new(),
            picker_rows: Vec::new(),
            grid_hit: Rc::new(RefCell::new(None)),
            grid_cache: Rc::new(RefCell::new(None)),
            hover_cell: None,
            image_cache: HashMap::new(),
            search_debounce_gen: 0,
            playing: false,
            play_gen: 0,
            cmd_tx: None,
            pending_ckpt: None,
            viz_force_retry: false,
        }
    }

    pub fn attach(&mut self, cmd_tx: UnboundedSender<NetworkCommand>) {
        self.cmd_tx = Some(cmd_tx);
        self.refresh_deck();
    }

    pub fn detach(&mut self) {
        self.cmd_tx = None;
        self.refreshing = false;
        self.loading = false;
        self.stepping = false;
    }

    pub fn on_visible(&mut self, cx: &mut Context<Self>) {
        if self.summary.is_none() {
            self.refresh(cx);
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        self.error = None;
        self.refresh_deck();
        cx.notify();
    }

    pub fn refreshing(&self) -> bool {
        self.refreshing
    }

    pub fn picker_query(&self) -> &str {
        &self.search_query
    }

    /// Recompute the cached, search-filtered run rows for the left rail. Call
    /// whenever `search_query`, `summary`, or `checkpoints` change.
    pub(crate) fn rebuild_picker_rows(&mut self) {
        self.picker_rows = picker::build_picker_rows(self);
    }

    fn refresh_deck(&mut self) {
        self.send(NetworkCommand::TopologyRefreshDeck);
    }

    fn send(&self, cmd: NetworkCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.unbounded_send(cmd);
        }
    }

    pub fn select_from_war_room(
        &mut self,
        fleet: String,
        pod: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if let Some((ck_fleet, ck_pod, ck_run, file)) =
            resolve_checkpoint_selection(self.checkpoints.as_ref(), self.summary.as_ref(), &pod, &name)
        {
            self.select_run(ck_fleet, ck_pod, ck_run, file, cx);
            return;
        }
        self.select_run(fleet, pod, name, String::new(), cx);
    }

    pub fn select_run(
        &mut self,
        fleet: String,
        pod: String,
        run: String,
        file: String,
        cx: &mut Context<Self>,
    ) {
        self.selected = Some((pod.clone(), run.clone()));
        // Carry the fleet straight from the checkpoint row (the web visualizer sends
        // `fleet: c.run.fleet`). The GCS object key is `<fleet>/<pod>/<run>/<file>`, so an
        // empty fleet segment here makes the portal look up a path that does not exist and
        // surfaces as "checkpoint … not found in GCS".
        self.selected_fleet = if fleet.is_empty() { None } else { Some(fleet) };
        self.selected_file = if file.is_empty() {
            default_checkpoint_file(self.checkpoints.as_ref(), &pod, &run, self.summary.as_ref())
        } else {
            Some(file)
        };
        self.error = None;
        cx.notify();
    }

    pub fn load_checkpoint(&mut self, cx: &mut Context<Self>) {
        let Some((pod, run)) = self.selected.clone() else {
            return;
        };
        let Some(file) = self.selected_file.clone() else {
            return;
        };
        // Prefer the fleet captured from the selected checkpoint row (matches the web
        // visualizer's `fleet: c.run.fleet`). Only fall back to deriving it from the runs
        // summary when the checkpoint row carried no fleet — and match the way the web app
        // pairs checkpoints with runs (pod tag + run/group), since the checkpoint pod/run
        // formats differ from the summary's.
        let fleet = self
            .selected_fleet
            .clone()
            .or_else(|| {
                self.summary.as_ref().and_then(|s| {
                    s.runs
                        .iter()
                        .find(|r| run_matches_checkpoint(r, &pod, &run))
                        .and_then(|r| {
                            if r.fleet.is_empty() {
                                None
                            } else {
                                Some(r.fleet.clone())
                            }
                        })
                })
            })
            .unwrap_or_default();

        if fleet.is_empty() {
            self.set_error(
                "Missing fleet for checkpoint — pick a run from the list (needs GCS path fleet/pod/run/file)",
                cx,
            );
            return;
        }

        self.loading = true;
        self.error = None;
        self.topo_data = None;
        self.viz_meta = None;
        self.step_resp = None;
        self.gallery = None;
        self.gallery_loading = false;
        self.image_cache.clear();
        self.decoded_frames.clear();
        self.scrub_tick = 0;
        self.selected_cell = None;
        self.hover_cell = None;
        self.display_mode = DisplayMode::Structure;
        self.rebuild_grid_cache();
        self.viz_force_retry = false;
        self.playing = false;
        self.play_gen = self.play_gen.wrapping_add(1);
        self.send(NetworkCommand::TopologyVizOpen {
            fleet,
            pod: pod.clone(),
            run: run.clone(),
            file: file.clone(),
            force: false,
        });

        if let Some(topo_file) = self
            .manifest
            .as_ref()
            .and_then(|m| topo_entry_file(m, &run))
        {
            self.send(NetworkCommand::TopologyFetchTopoFab {
                run: run.clone(),
                file: topo_file,
            });
        }
        cx.notify();
    }

    pub fn step(&mut self, cx: &mut Context<Self>) {
        let Some(meta) = self.viz_meta.as_ref() else {
            return;
        };
        let run_dataset = meta.dataset.as_deref();
        let has_dataset = meta.has_dataset.unwrap_or(false);
        let body = VizStepRequest {
            ticks: self.ticks as i64,
            drive: build_drive(BuildDriveOpts {
                source: &self.input_source,
                idx: self.input_idx as i64,
                run_dataset,
                has_dataset,
                pixel_b64: None,
            }),
            want_state: true,
            state_fmt: Some("quant8".into()),
        };
        self.stepping = true;
        self.playing = false;
        self.play_gen = self.play_gen.wrapping_add(1);
        self.send(NetworkCommand::TopologyVizStep {
            body: serde_json::to_value(body).unwrap_or(Value::Null),
        });
        cx.notify();
    }

    pub fn set_scrub_tick(&mut self, tick: usize, cx: &mut Context<Self>) {
        if tick < self.decoded_frames.len() {
            self.scrub_tick = tick;
            self.rebuild_grid_cache();
            cx.notify();
        }
    }

    pub fn bump_scrub_tick(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.decoded_frames.is_empty() {
            return;
        }
        let n = self.decoded_frames.len();
        let next = (self.scrub_tick as i32 + delta).clamp(0, n as i32 - 1) as usize;
        self.set_scrub_tick(next, cx);
    }

    pub fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        let n = self.decoded_frames.len();
        if n == 0 {
            return;
        }
        if self.playing {
            self.stop_playback();
            cx.notify();
            return;
        }

        self.playing = true;
        self.play_gen = self.play_gen.wrapping_add(1);
        let gen = self.play_gen;
        self.scrub_tick = 0;
        self.set_display_mode(DisplayMode::LiveFlow, cx);

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PLAYBACK_INTERVAL).await;
                let mut finished = false;
                let _ = this.update(cx, |this, cx| {
                    if !this.playing || this.play_gen != gen {
                        finished = true;
                        return;
                    }
                    let n = this.decoded_frames.len();
                    if this.scrub_tick + 1 >= n {
                        this.playing = false;
                        finished = true;
                        cx.notify();
                        return;
                    }
                    this.set_scrub_tick(this.scrub_tick + 1, cx);
                });
                if finished {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| {
                this.playing = false;
            });
        })
        .detach();
        cx.notify();
    }

    /// Clear a prior rollout when the drive input changes — back to structure view until RUN.
    fn reset_rollout(&mut self) {
        self.stop_playback();
        self.decoded_frames.clear();
        self.step_resp = None;
        self.scrub_tick = 0;
        self.display_mode = DisplayMode::Structure;
        self.selected_cell = None;
        self.hover_cell = None;
        self.image_cache
            .retain(|key, _| !key.starts_with("recon:"));
        self.rebuild_grid_cache();
        if self.viz_meta.is_some() {
            self.send(NetworkCommand::TopologyVizReset);
        }
    }

    fn stop_playback(&mut self) {
        self.playing = false;
        self.play_gen = self.play_gen.wrapping_add(1);
    }

    pub fn set_display_mode(&mut self, mode: DisplayMode, cx: &mut Context<Self>) {
        self.display_mode = mode;
        self.rebuild_grid_cache();
        cx.notify();
    }

    pub fn select_cell(&mut self, cell: usize, cx: &mut Context<Self>) {
        self.selected_cell = Some(cell);
        cx.notify();
    }

    /// Cursor moved over the substrate — update the floating activation readout. Repaints
    /// reuse the cached bitmap (only `hover_cell` changed), so this is a cheap blit.
    pub fn set_hover_cell(&mut self, hover: Option<(u32, u32, f32)>, cx: &mut Context<Self>) {
        if self.hover_cell != hover {
            self.hover_cell = hover;
            cx.notify();
        }
    }

    /// Drop the rasterized substrate so the next paint rebuilds it from the current
    /// `scrub_tick` / `display_mode` / decoded frames. The actual RGBA build happens lazily
    /// in the canvas paint callback where the pixel bounds are known.
    pub(crate) fn rebuild_grid_cache(&self) {
        *self.grid_cache.borrow_mut() = None;
    }

    pub fn set_input_source(&mut self, source: String, cx: &mut Context<Self>) {
        if self.input_source == source {
            return;
        }
        self.input_source = source;
        self.input_idx = 0;
        self.reset_rollout();
        self.fetch_gallery();
        self.fetch_input_preview();
        cx.notify();
    }

    pub fn set_input_idx(&mut self, idx: u32, cx: &mut Context<Self>) {
        if self.input_idx == idx {
            return;
        }
        self.input_idx = idx;
        self.reset_rollout();
        self.fetch_input_preview();
        cx.notify();
    }

    pub fn bump_input_idx(&mut self, delta: i32, cx: &mut Context<Self>) {
        let next = (self.input_idx as i32 + delta).max(0) as u32;
        self.set_input_idx(next, cx);
    }

    pub(crate) fn gallery_image(&self, idx: i64) -> Option<Arc<RenderImage>> {
        self.image_cache.get(&format!("gal:{idx}")).cloned()
    }

    pub(crate) fn input_preview_image(&self) -> Option<Arc<RenderImage>> {
        self.image_cache
            .get(&format!("input:{}", self.input_idx))
            .cloned()
            .or_else(|| self.gallery_image(self.input_idx as i64))
    }

    pub(crate) fn recon_preview_image(&self, tick: usize) -> Option<Arc<RenderImage>> {
        self.image_cache.get(&format!("recon:{tick}")).cloned()
    }

    fn cache_b64_image(&mut self, key: String, b64: &str) {
        if self.image_cache.contains_key(&key) {
            return;
        }
        if let Some(img) = image_b64_to_render_image(b64) {
            self.image_cache.insert(key, img);
        }
    }

    fn cache_bytes_image(&mut self, key: String, bytes: &[u8]) {
        if self.image_cache.contains_key(&key) {
            return;
        }
        if let Some(img) = image_bytes_to_render_image(bytes) {
            self.image_cache.insert(key, img);
        }
    }

    fn cache_gallery(&mut self, resp: &VizGalleryResp) {
        for item in &resp.items {
            if let Some(b64) = item.png.as_deref() {
                self.cache_b64_image(format!("gal:{}", item.idx), b64);
            }
        }
    }

    fn cache_recon_frames(&mut self, resp: &ReconStepResp) {
        for (tick, b64) in resp.recon.iter().enumerate() {
            self.cache_b64_image(format!("recon:{tick}"), b64);
        }
    }

    /// Request the dataset thumbnail batch for the active input source. No-op until a run is
    /// loaded (the gallery endpoint is served by the per-run viewer daemon).
    fn fetch_gallery(&mut self) {
        let Some(meta) = self.viz_meta.as_ref() else {
            return;
        };
        let dataset = gallery_dataset(meta, &self.input_source);
        self.gallery = None;
        self.gallery_loading = true;
        self.send(NetworkCommand::TopologyFetchGallery {
            dataset,
            start: 0,
            count: GALLERY_COUNT,
            size: GALLERY_THUMB_PX,
        });
    }

    /// Fetch a single input image when the gallery batch does not include the active idx.
    fn fetch_input_preview(&mut self) {
        let Some(meta) = self.viz_meta.as_ref() else {
            return;
        };
        let key = format!("input:{}", self.input_idx);
        if self.image_cache.contains_key(&key) || self.gallery_image(self.input_idx as i64).is_some()
        {
            return;
        }
        let dataset = gallery_dataset(meta, &self.input_source);
        self.send(NetworkCommand::TopologyFetchInputImage {
            dataset,
            idx: self.input_idx,
            size: INPUT_PREVIEW_PX,
        });
    }

    pub fn bump_ticks(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.ticks = (self.ticks as i32 + delta).clamp(1, 128) as u32;
        cx.notify();
    }

    pub fn handle_msg(&mut self, msg: TopologyMsg, cx: &mut Context<Self>) {
        match msg {
            TopologyMsg::RefreshStarted => {
                self.refreshing = true;
                cx.notify();
            }
            TopologyMsg::Deck {
                summary,
                checkpoints,
                manifest,
            } => {
                self.refreshing = false;
                match summary {
                    Ok(s) => {
                        self.error = None;
                        self.summary = Some(s);
                    }
                    Err(e) => self.set_error(format!("API ERR — {e}"), cx),
                }
                if let Ok(ckpts) = checkpoints {
                    self.checkpoints = Some(ckpts);
                }
                if let Ok(m) = manifest {
                    self.manifest = Some(m);
                }
                self.rebuild_picker_rows();
                cx.notify();
            }
            TopologyMsg::VizOpen(result) => {
                match result {
                    Ok(v) => {
                        if let Ok(resp) = serde_json::from_value::<VizOpenResp>(v) {
                            if let Some(ckpt) = resp.ckpt.filter(|c| !c.is_empty()) {
                                self.pending_ckpt = Some(ckpt.clone());
                                self.send(NetworkCommand::TopologyPollVizReady { ckpt });
                            } else {
                                self.loading = false;
                                self.set_error("Viz open — no checkpoint id", cx);
                            }
                        } else {
                            self.loading = false;
                            self.set_error("Viz open — bad response", cx);
                        }
                    }
                    Err(e) => {
                        self.loading = false;
                        self.set_error(format!("Viz open — {e}"), cx);
                    }
                }
                cx.notify();
            }
            TopologyMsg::VizReady(result) => match result {
                Ok(status) => {
                    if viz_status_ready(&status) {
                        self.send(NetworkCommand::TopologyFetchVizState);
                    } else if let Some(err) = viz_status_error(&status) {
                        self.loading = false;
                        self.set_error(format!("Viz ready — {err}"), cx);
                    } else {
                        self.loading = false;
                        self.set_error("Viz load failed", cx);
                    }
                }
                Err(e) => {
                    self.loading = false;
                    self.set_error(format!("Viz ready — {e}"), cx);
                }
            },
            TopologyMsg::VizState(result) => {
                self.loading = false;
                match result {
                    Ok(v) => {
                        if let Ok(meta) = serde_json::from_value::<VizLoadMeta>(v) {
                            if meta.loaded == Some(false) {
                                if !self.viz_force_retry {
                                    // Mirror index.tsx: retry once with force when the box
                                    // session vanished but the open job reported ready.
                                    self.viz_force_retry = true;
                                    self.loading = true;
                                    if let (Some((pod, run)), Some(file)) =
                                        (self.selected.clone(), self.selected_file.clone())
                                    {
                                        let fleet = self.selected_fleet.clone().unwrap_or_default();
                                        self.send(NetworkCommand::TopologyVizOpen {
                                            fleet,
                                            pod,
                                            run,
                                            file,
                                            force: true,
                                        });
                                    } else {
                                        self.set_error(
                                            "Model did not load on the inference box — try again",
                                            cx,
                                        );
                                    }
                                } else {
                                    self.set_error(
                                        "Model did not load on the inference box — try again",
                                        cx,
                                    );
                                }
                            } else {
                                // Adopt the run's default source + tick count (index.tsx
                                // onSuccess) so STEP drives the right dataset and the input
                                // picker can list this run's built-ins.
                                self.input_source = default_viz_source(&meta);
                                self.ticks = meta
                                    .def_ticks
                                    .filter(|t| *t > 0)
                                    .map(|t| (t as u32).clamp(1, 128))
                                    .unwrap_or(16);
                                self.viz_meta = Some(meta);
                                self.error = None;
                                self.fetch_gallery();
                                self.fetch_input_preview();
                            }
                        } else {
                            self.set_error("Viz state — bad response", cx);
                        }
                    }
                    Err(e) => self.set_error(format!("Viz state — {e}"), cx),
                }
                cx.notify();
            }
            TopologyMsg::VizStep(result) => {
                self.stepping = false;
                match result {
                    Ok(v) => {
                        if let Ok(resp) = serde_json::from_value::<ReconStepResp>(v) {
                            let meta = resp.state_meta.clone().unwrap_or(fabric_types::VizStateMeta {
                                r: 1,
                                c: 1,
                                scale: 1.0,
                                zero: 0,
                                signed: false,
                            });
                            self.decoded_frames = resp
                                .state
                                .iter()
                                .map(|b64| decode_quant8_frame(b64, &meta).unwrap_or_default())
                                .collect();
                            // Start at the first returned pass so the user sees the rollout from
                            // the beginning before scrubbing or pressing PLAY.
                            self.scrub_tick = 0;
                            self.display_mode = DisplayMode::LiveFlow;
                            self.step_resp = Some(resp.clone());
                            self.cache_recon_frames(&resp);
                            self.rebuild_grid_cache();
                            self.error = None;
                        } else {
                            self.set_error("Viz step — bad response", cx);
                        }
                    }
                    Err(e) => self.set_error(format!("Viz step — {e}"), cx),
                }
                cx.notify();
            }
            TopologyMsg::TopoFab { run, file, result } => match result {
                Ok(bytes) => {
                    if let Ok((header, _arrays)) = parse_fab1(&bytes) {
                        self.topo_data = Some(topo_from_fab(header));
                        self.rebuild_grid_cache();
                    }
                    cx.notify();
                }
                Err(e) => self.set_error(format!("Topo FAB {run}/{file} — {e}"), cx),
            },
            TopologyMsg::Gallery(result) => {
                self.gallery_loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_gallery(&resp);
                        self.gallery = Some(resp);
                        self.fetch_input_preview();
                    }
                    Err(_) => self.gallery = None,
                }
                cx.notify();
            }
            TopologyMsg::InputImage { idx, result } => {
                if let Ok(bytes) = result {
                    self.cache_bytes_image(format!("input:{idx}"), &bytes);
                    cx.notify();
                }
            }
        }
    }

    fn set_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    fn status_left(&self) -> SharedString {
        let runs = self.summary.as_ref().map(|s| s.runs.len()).unwrap_or(0);
        let mode = match self.display_mode {
            DisplayMode::Structure => "STRUCT",
            DisplayMode::LiveFlow => "LIVE",
        };
        SharedString::from(format!("{runs} runs · {mode}"))
    }
}

fn topo_from_fab(header: fabric_types::FabHeader) -> fabric_types::TopoDataInMemory {
    let cols = header.c.unwrap_or(1).max(1) as u32;
    let band_rows = header.band_rows.unwrap_or(1).max(1) as u32;
    let planes = header.n_planes.unwrap_or(1).max(1) as u32;
    let n = header
        .n
        .unwrap_or((cols * band_rows * planes) as i64)
        .max(0) as usize;

    let regions = if let Some(spans) = header.regions {
        regions_from_fab_spans(n.max(1), cols, band_rows, &spans)
    } else {
        vec![fabric_types::RegionKind::Compute; n.max(1)]
    };

    fabric_types::TopoDataInMemory {
        n,
        cols,
        band_rows,
        planes,
        regions,
    }
}

fn topo_entry_file(manifest: &TopoManifestResp, run: &str) -> Option<String> {
    let entry = manifest.topologies.iter().find(|e| e.run == run)?;
    // Mirror TopologyViewer.tsx: `entry.file || entry.archive_file`. The primary
    // `file` can be empty when only an archive export exists; fetching an empty
    // file name would hit `/api/topology/file?file=` and pull the wrong blob.
    let file = if !entry.file.is_empty() {
        entry.file.clone()
    } else {
        entry.archive_file.clone().unwrap_or_default()
    };
    if file.is_empty() {
        None
    } else {
        Some(file)
    }
}

fn pod_tag(pod: &str) -> &str {
    pod.rsplit(':').next().unwrap_or(pod)
}

/// Pair a summary run with a checkpoint's `(pod, run)` the way the web visualizer's
/// `matchRunForCheckpoint` does: tolerate the checkpoint pod carrying a `fleet:pod` prefix
/// and the checkpoint run being a `group` or `group_<podtag>` rather than the exact run name.
pub(crate) fn run_matches_checkpoint(run: &RunScalars, ckpt_pod: &str, ckpt_run: &str) -> bool {
    let tag = pod_tag(ckpt_pod);
    let pod_ok = run.pod == ckpt_pod || pod_tag(&run.pod) == tag;
    if !pod_ok {
        return run.name == ckpt_run || run.group == ckpt_run;
    }
    run.name == ckpt_run
        || (!run.group.is_empty() && format!("{}_{}", run.group, tag) == ckpt_run)
        || run.group == ckpt_run
}

fn default_checkpoint_file(
    checkpoints: Option<&CheckpointsResp>,
    pod: &str,
    run: &str,
    summary: Option<&RunsSummary>,
) -> Option<String> {
    let run_row = summary.and_then(|s| {
        s.runs
            .iter()
            .find(|r| r.pod == pod && r.name == run)
    });
    let ckpt_run = checkpoints.and_then(|c| {
        c.runs.iter().find(|r| {
            if r.pod == pod && r.run == run {
                return true;
            }
            run_row.is_some_and(|row| run_matches_checkpoint(row, &r.pod, &r.run))
        })
    })?;
    let prefer = ckpt_run
        .files
        .iter()
        .find(|f| f.filename.contains(".best"))
        .or_else(|| ckpt_run.files.first());
    prefer.and_then(|f| {
        if viz_eligible_checkpoint(ckpt_run, f, run_row) {
            Some(f.filename.clone())
        } else {
            ckpt_run
                .files
                .iter()
                .find(|file| viz_eligible_checkpoint(ckpt_run, file, run_row))
                .map(|file| file.filename.clone())
        }
    })
}

/// Resolve GCS path fields from a War Room run row by finding the matching checkpoint index entry.
fn resolve_checkpoint_selection(
    checkpoints: Option<&CheckpointsResp>,
    summary: Option<&RunsSummary>,
    pod: &str,
    name: &str,
) -> Option<(String, String, String, String)> {
    let run_row = summary.and_then(|s| s.runs.iter().find(|r| r.pod == pod && r.name == name))?;
    let ck = checkpoints?.runs.iter().find(|ck| {
        run_matches_checkpoint(run_row, &ck.pod, &ck.run)
    })?;
    let file = ck
        .files
        .iter()
        .find(|f| f.filename.contains(".best"))
        .or_else(|| ck.files.first())
        .filter(|f| viz_eligible_checkpoint(ck, f, Some(run_row)))?;
    Some((
        ck.fleet.clone(),
        ck.pod.clone(),
        ck.run.clone(),
        file.filename.clone(),
    ))
}

impl Render for TopologyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        let err = self.error.clone();
        let operator = None::<String>;

        theme
            .block()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when_some(err, |el, e| {
                        el.child(
                            div()
                                .flex_none()
                                .px(px(8.))
                                .py(px(4.))
                                .bg(gpui::rgb(0x180000))
                                .text_color(theme.warn)
                                .child(format!("■ {e}")),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(picker_rail(self, &theme, cx))
                            .child(gallery_pane(self, &theme, cx))
                            .child(explorer_canvas(self, &theme, cx))
                            .child(controls_rail(self, &theme, cx)),
                    )
                    .child(timeline_bar(self, &theme, cx)),
            )
            .child(theme.status_bar(self.status_left(), operator.map(SharedString::from)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn default_input_is_mnist(cx: &mut TestAppContext) {
        let view = cx.new(TopologyView::new);
        view.read_with(cx, |view, _| {
            assert_eq!(view.input_source, "mnist");
            assert_eq!(view.ticks, 16);
        });
    }

    #[gpui::test]
    fn viz_step_starts_at_first_tick(cx: &mut TestAppContext) {
        let view = cx.new(TopologyView::new);
        let resp = ReconStepResp {
            kind: "recon".into(),
            tick: 3,
            state: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };

        view.update(cx, |view, cx| {
            view.scrub_tick = 42;
            view.handle_msg(
                TopologyMsg::VizStep(Ok(serde_json::to_value(resp).unwrap())),
                cx,
            );
            assert_eq!(view.decoded_frames.len(), 3);
            assert_eq!(view.scrub_tick, 0);
            assert_eq!(view.display_mode, DisplayMode::LiveFlow);
        });
    }

    #[test]
    fn matches_checkpoint_across_pod_and_group_forms() {
        let run = RunScalars {
            pod: "blackstar:gpu0".into(),
            name: "count20_gpu0".into(),
            group: "count20".into(),
            fleet: "blackstar".into(),
            ..Default::default()
        };
        // Exact pod + exact run name.
        assert!(run_matches_checkpoint(&run, "blackstar:gpu0", "count20_gpu0"));
        // Checkpoint pod carries no fleet prefix but the tag still matches.
        assert!(run_matches_checkpoint(&run, "gpu0", "count20_gpu0"));
        // Checkpoint run is the bare group.
        assert!(run_matches_checkpoint(&run, "gpu0", "count20"));
        // group_<podtag> reconstruction.
        assert!(run_matches_checkpoint(&run, "gpu0", "count20_gpu0"));
        // A different run on the same pod must not match.
        assert!(!run_matches_checkpoint(&run, "gpu0", "other_run"));
    }
}
