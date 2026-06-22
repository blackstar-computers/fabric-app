//! Decode base64 / raw image payloads into GPUI [`RenderImage`]s for the gallery thumbnails and
//! the input/recon preview pane.
//!
//! GPUI stores textures in BGRA order, so every decoded RGBA pixel is byte-swapped before the
//! frame is handed to [`RenderImage`] (mirrors `Image::to_image_data` in gpui).

use base64::Engine;
use gpui::RenderImage;
use image::Frame;
use std::sync::Arc;

/// Decode a base64-encoded image (PNG/JPEG — the wire form used by `/api/step` recon frames,
/// gallery thumbnails, and `/api/image`) into a cached [`RenderImage`].
pub fn image_b64_to_render_image(b64: &str) -> Option<Arc<RenderImage>> {
    let trimmed = b64.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Tolerate an optional `data:image/...;base64,` prefix.
    let payload = trimmed
        .rsplit_once(',')
        .map(|(_, tail)| tail)
        .unwrap_or(trimmed);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    image_bytes_to_render_image(&bytes)
}

/// Back-compat alias — gallery and step payloads may be PNG or JPEG.
#[allow(dead_code)]
pub fn png_b64_to_render_image(b64: &str) -> Option<Arc<RenderImage>> {
    image_b64_to_render_image(b64)
}

/// Decode raw image bytes (PNG/JPEG from `/viz/default/api/image`) into a [`RenderImage`].
pub fn image_bytes_to_render_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let mut data = image::load_from_memory(bytes).ok()?.into_rgba8();

    // RGBA → BGRA for GPUI's texture upload path.
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    Some(Arc::new(RenderImage::new(vec![Frame::new(data)])))
}

/// Decode raw PNG bytes when the caller knows the format.
#[allow(dead_code)]
pub fn png_bytes_to_render_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    image_bytes_to_render_image(bytes)
}
