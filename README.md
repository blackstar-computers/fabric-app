# fabric-app

Native **Fabric** dashboard for macOS — GPU-accelerated (Metal via [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)), talking to the same portal APIs as the web dashboard.

**v0.1:** a barebones run list with live SSE updates.

## Prerequisites (macOS)

- [Rust](https://rustup.rs/) (stable)
- Xcode Command Line Tools (Metal shader compilation)
- A Fabric service token (`fabric auth <token>` on your laptop, or `FABRIC_SERVICE_TOKEN`)

If the build fails with *"missing Metal Toolchain"*, install Xcode components:

```bash
xcode-select --install
# or, with full Xcode:
xcodebuild -downloadComponent MetalToolchain
```

## Run

```bash
cargo run -p fabric_desktop
```

The app reads your token from (in order):

1. `$FABRIC_SERVICE_TOKEN`
2. macOS Keychain (`inc.blackstar.fabric` / `service-token`)
3. `~/.config/fabric/service_token` (same file as `fabric auth`)

Portal URL defaults to `https://agents.fabric.blackstar.inc` (`$FABRIC_PORTAL_URL` to override).

## Workspace layout

| Crate | Role |
|---|---|
| `fabric_types` | Serde models mirroring `web_app/src/types.ts` |
| `fabric_api` | HTTP client (`X-Fleet-Token` auth) |
| `fabric_live` | SSE stream + in-place summary patching |
| `fabric_desktop` | GPUI app (Metal rendering) |

## Development

```bash
# Unit tests (no GPU / Metal required for library crates)
cargo test -p fabric_types -p fabric_api -p fabric_live

# Full app (requires Metal)
cargo run -p fabric_desktop
```

First GPUI build compiles Zed's UI stack from git — expect a long initial `cargo build`.

## Roadmap

- [x] v0.1 — run list + SSE live rows
- [ ] v0.2 — sort columns, manual refresh, error retry
- [ ] v0.3 — Overview sparklines (`/api/runs/series`)
- [ ] v1.0 — War Room detail pane
- [ ] v2.0 — embedded terminal + launch blocks (Warp-style)

## Related

Control plane and web dashboard live in the main [fabric](https://github.com/blackstar-inc/fabric) repo (`web_app/`, `server/`, `fleet/`).
