//! Decode quant8 state frames from base64 wire buffers.

use base64::Engine;
use fabric_types::VizStateMeta;

/// Decode one quant8 state frame into signed activations (row-major `R×C`).
///
/// `float = (u8 - zero) * scale` — mirrors `decodeQuant8Frame` in `viz.ts`.
pub fn decode_quant8_frame(b64: &str, meta: &VizStateMeta) -> Result<Vec<f32>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("base64 decode: {e}"))?;
    let n = (meta.r * meta.c).max(0) as usize;
    let len = n.min(bytes.len());
    let zero = meta.zero as f32;
    let scale = meta.scale as f32;
    Ok((0..len)
        .map(|i| (bytes[i] as f32 - zero) * scale)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn decodes_quant8_frame() {
        let raw = [128u8, 138, 118, 148];
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        let meta = VizStateMeta {
            r: 2,
            c: 2,
            scale: 0.1,
            zero: 128,
            signed: true,
        };
        let out = decode_quant8_frame(&b64, &meta).expect("decode");
        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[1] - 1.0).abs() < 1e-6);
        assert!((out[2] - (-1.0)).abs() < 1e-6);
        assert!((out[3] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn truncates_when_buffer_shorter_than_grid() {
        let b64 = base64::engine::general_purpose::STANDARD.encode([100u8]);
        let meta = VizStateMeta {
            r: 2,
            c: 2,
            scale: 1.0,
            zero: 0,
            signed: false,
        };
        let out = decode_quant8_frame(&b64, &meta).expect("decode");
        assert_eq!(out.len(), 1);
        assert!((out[0] - 100.0).abs() < 1e-6);
    }
}
