# SubstrateExplorer — Topology Page Spec

Unified topology exploration for the Fabric desktop app. One surface combines **structure** (substrate wiring) and **live activations** (forward-pass flow) over a shared run context.

Reference: `fabric-visualizer` (`visualizer-overhaul`) — `VisualizerRoute`, `SubstrateGrid`, `TopologyEngine`.

---

## Goals

1. User selects a training run + checkpoint, then explores substrate structure and activation flow without switching modes.
2. Load an image from the run's dataset (MNIST, fashion, run `.npy`), step the model, scrub ticks, see activations propagate.
3. Structure and activations share cell indexing: flat `i = r * C + c` matches topo FAB `N` nodes.
4. Rich display: main structure canvas + activation coloring + input/recon insets + timeline.

## Non-goals (v1)

- Native Three.js / WebView 3D (use 2D projected structure canvas; 3D deferred)
- Pixel upload drive
- Higher-dimensional graph view (placeholder only)
- LM classify full parity (basic logits panel ok)

---

## App integration

### Navigation

Add `AppMode::Topology` as third top-level tab: `RUNS | FLEETS | TOPOLOGY`.

Persist: `~/.config/fabric/fabricApp.topology.json` — `{ pod, name, file, input_idx, input_source }`.

Deep link from War Room: when `runspec.ui_capabilities.topology_link`, command bar button → `set_mode(Topology)` + pre-select run.

### Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│  toolbar: RUNS FLEETS TOPOLOGY                    [refresh] [live]   │
├──────────┬───────────────────────────────────────────┬───────────────┤
│ PICKER   │  MAIN CANVAS (SubstrateExplorer)          │ CONTROLS      │
│ 200px    │  structure grid + activation overlay      │ 224px         │
│          │                                           │               │
│ runs +   │  ┌ input ┐ ┌ recon/state inset ┐          │ step, ticks   │
│ ckpts    │  └───────┘ └──────────────────┘          │ scrubber      │
│          ├───────────────────────────────────────────┤ plane toggle  │
│          │  TIMELINE (tick scrub + play)             │ cell info     │
└──────────┴───────────────────────────────────────────┴───────────────┘
│ status bar                                                           │
└──────────────────────────────────────────────────────────────────────┘
```

### Display modes (single view, not tabs)

| Mode | Trigger | Cube/cell coloring | Edges |
|------|---------|-------------------|-------|
| Structure | No step yet / toggle | Region colors (input/compute/readout/recon) | Full wiring (downsampled) |
| Live flow | After step | Diverging heatmap from quant8 @ tick | Highlight fan-in/out for selection |
| Compare | Toggle | Delta vs previous tick | Optional |

---

## Crates

### `fabric_types`

New modules: `viz.rs`, `topology.rs`

**viz.rs** — portal + infer-box wire types:
- `VizGeometry`, `VizRegion`, `VizStateMeta`, `VizLoadMeta`
- `DriveSpec`, `VizStepRequest`, `ReconStepResp`, `ClassifyStepResp`
- `CheckpointsResp`, `CheckpointRun`, `CheckpointFile`
- `VizStatusResp`, `VizOpenRequest`, `VizOpenResp`

**topology.rs**:
- `TopoManifestResp`, `TopoEntry`, `TopoWeight`
- `TopoData` (parsed in-memory, not serialized)

### `fabric_viz` (new workspace crate)

Pure Rust, no GPUI:
- `decode_quant8_frame(b64, meta) -> Vec<f32>`
- `parse_fab1(buf) -> (FabHeader, arrays)`
- `diverging_rgb(v, vmax) -> [u8;4]`
- `viz_eligible_checkpoint(...) -> bool`
- `build_drive(source, idx, ...) -> DriveSpec`
- `cell_index(r, c, meta) -> usize`

Dependencies: `base64`, `fabric_types`.

### `fabric_api`

New client methods:
- `fetch_checkpoints(fleet) -> CheckpointsResp`
- `fetch_topo_manifest() -> TopoManifestResp`
- `fetch_binary(path) -> Vec<u8>`
- `viz_open(body) -> VizOpenResp`
- `viz_status(ckpt) -> VizStatusResp`
- `viz_state() -> VizLoadMeta`
- `viz_step(body) -> serde_json::Value` (parse kind in caller)

Viz proxied paths use same portal origin: `/viz/default/api/*`.

### `fabric_desktop`

New module tree:

```
topology/
  mod.rs          — TopologyView state owner
  picker.rs       — run + checkpoint list
  explorer.rs     — main canvas (structure + activation paint)
  controls.rs     — right rail
  timeline.rs     — tick scrubber
  inset.rs        — input/recon preview (base64 PNG decode)
```

---

## Network commands

```rust
enum NetworkCommand {
  // existing...
  TopologyRefreshDeck,           // summary + checkpoints + topo manifest
  TopologyVizOpen { fleet, pod, run, file },
  TopologyPollVizReady { ckpt },
  TopologyFetchVizState,
  TopologyVizStep { ticks, drive, state_fmt },
  TopologyFetchTopoFab { run, file },
}
```

```rust
enum TopologyMsg {
  Deck { summary, checkpoints, manifest },
  VizOpen(Result<VizOpenResp, ClientError>),
  VizReady(Result<VizStatusResp, ClientError>),
  VizState(Result<VizLoadMeta, ClientError>),
  VizStep(Result<ReconStepResp, ClientError>),  // or Classify
  TopoFab { run, file, result: Result<Vec<u8>, ClientError> },
  RefreshStarted,
}
```

Poll viz ready: 1.5s interval, 6min timeout (match web visualizer).

---

## SubstrateExplorer canvas (v1: 2D projection)

GPUI canvas painting (pattern: `fleet_canvas.rs`, `sparkline.rs`):

1. Load `.topo` FAB → `TopoData` with CSR graph.
2. Layout: for each plane `p`, draw `bandRows × C` cell rects in stacked rows (plane label on left).
3. **Structure mode**: fill by `regionOf(col, row)`.
4. **Live flow mode**: after step, decode all quant8 ticks once → `Vec<Vec<f32>>`; on scrub, paint diverging colors.
5. **Selection**: click cell → highlight + show fan-in/out edges as lines overlay.
6. **Alignment guard**: if `state_meta.R * state_meta.C != topo.N`, show banner + 2D grid only.

Edge budget: use `.view.topo` downsampled export when `E > 8000`.

---

## User flow

1. Open TOPOLOGY tab → fetch summary + checkpoints + manifest.
2. Pick run row → filter checkpoints with `viz_eligible_checkpoint`.
3. Auto-select best `.pt.best` if present.
4. Click **Load** → `viz_open` background → poll until ready → `viz_state`.
5. If topo export `ready` for run, fetch `.topo` in parallel.
6. Pick input source (builtin mnist) + index, set ticks, click **Step**.
7. Decode frames, switch to Live flow mode, scrub timeline.
8. Click cell → fan-in/out highlight + stats in controls rail.

---

## Phase plan

| Phase | Deliverable |
|-------|-------------|
| **1** | Types, API, network, tab shell, picker |
| **2** | Viz open/step, 2D activation grid inset, timeline |
| **3** | Topo FAB load, structure canvas, combined live flow paint |
| **4** | War Room deep link, persistence, polish |
| **5** | Native 3D or WebView embed (future) |

---

## Testing

- `fabric_viz`: unit tests for quant8 decode, FAB1 parse (fixture bytes)
- `fabric_types`: serde round-trip on checkpoint/viz JSON fixtures
- Manual: load real portal, open topology tab, step MNIST on canvas run
