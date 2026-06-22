//! FAB1 binary parser — mirrors `web_app/src/routes/Visualizer/topology/fab.ts`.

use fabric_types::{FabArrayDesc, FabHeader};
use std::collections::HashMap;

const MAGIC: &[u8; 4] = b"FAB1";

/// Parse a FAB1 blob: magic + u32 header length + JSON header (4-byte padded) + raw arrays.
pub fn parse_fab1(buf: &[u8]) -> Result<(FabHeader, HashMap<String, Vec<u8>>), String> {
    if buf.len() < 8 {
        return Err("FAB file too short".into());
    }
    if buf.get(0..4) != Some(MAGIC.as_slice()) {
        let magic = String::from_utf8_lossy(&buf[0..4]);
        return Err(format!("not a FAB file (magic={magic})"));
    }
    let hlen = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let hdr_end = 8usize
        .checked_add(hlen)
        .ok_or_else(|| "header length overflow".to_string())?;
    if buf.len() < hdr_end {
        return Err("truncated FAB header".into());
    }
    let hdr: FabHeader = serde_json::from_slice(&buf[8..hdr_end])
        .map_err(|e| format!("invalid FAB header JSON: {e}"))?;
    let mut off = hdr_end;
    let mut arrays = HashMap::new();
    for FabArrayDesc { name, dtype, len } in &hdr.arrays {
        let byte_len = array_byte_len(dtype, *len)?;
        let end = off
            .checked_add(byte_len)
            .ok_or_else(|| format!("array {name} length overflow"))?;
        if buf.len() < end {
            return Err(format!("truncated array {name}"));
        }
        arrays.insert(name.clone(), buf[off..end].to_vec());
        off = end;
    }
    Ok((hdr, arrays))
}

fn array_byte_len(dtype: &str, len: i64) -> Result<usize, String> {
    if len < 0 {
        return Err(format!("negative array length for dtype {dtype}"));
    }
    let elem = match dtype {
        "int32" | "float32" => 4,
        other => return Err(format!("unknown dtype {other}")),
    };
    (len as usize)
        .checked_mul(elem)
        .ok_or_else(|| format!("array byte length overflow for dtype {dtype}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::FabArrayDesc;

    fn write_fab(header: &FabHeader, payloads: &[&[u8]]) -> Vec<u8> {
        let mut hb = serde_json::to_vec(header).expect("header json");
        let pad = (4 - (hb.len() % 4)) % 4;
        hb.extend(std::iter::repeat_n(b' ', pad));
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(hb.len() as u32).to_le_bytes());
        out.extend_from_slice(&hb);
        for p in payloads {
            out.extend_from_slice(p);
        }
        out
    }

    fn i32_bytes(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn parses_minimal_fab1() {
        let header = FabHeader {
            r#type: "topo".into(),
            run: Some("test_run".into()),
            n: Some(2),
            e: Some(3),
            arrays: vec![
                FabArrayDesc {
                    name: "row_ptr".into(),
                    dtype: "int32".into(),
                    len: 3,
                },
                FabArrayDesc {
                    name: "col".into(),
                    dtype: "int32".into(),
                    len: 3,
                },
            ],
            ..Default::default()
        };
        let buf = write_fab(
            &header,
            &[&i32_bytes(&[0, 1, 3]), &i32_bytes(&[1, 0, 2])],
        );
        let (hdr, arrays) = parse_fab1(&buf).expect("parse");
        assert_eq!(hdr.r#type, "topo");
        assert_eq!(arrays.len(), 2);
        assert_eq!(arrays["row_ptr"].len(), 12);
        assert_eq!(arrays["col"].len(), 12);
    }

    #[test]
    fn rejects_bad_magic() {
        let err = parse_fab1(b"NOPE____").unwrap_err();
        assert!(err.contains("not a FAB file"));
    }
}
