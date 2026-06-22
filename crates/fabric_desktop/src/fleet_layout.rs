//! Relay-tree layout — port of `web_app/src/routes/Fleets.tsx` `layoutTree()`.
//!
//! Constants match the web client's edge anchoring (`CARD_W`/`CARD_H`/`XS`/`YS`/`GUTTER`)
//! so parent→child Bézier links meet the card centers exactly.

use fabric_types::TreeNode;
use std::collections::{HashMap, HashSet};

// Must stay in sync with web_app/src/routes/Fleets.tsx (lines ~191–197).
pub const CARD_W: f32 = 152.;
pub const CARD_H: f32 = 84.;
pub const XS: f32 = 174.;
pub const YS: f32 = 128.;
pub const GUTTER: f32 = 14.;
pub const OFFX: f32 = CARD_W / 2. + GUTTER;
pub const OFFY: f32 = GUTTER;

#[derive(Clone, Debug, Default)]
pub struct NodePos {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, Default)]
pub struct TreeLayout {
    pub pos: HashMap<String, NodePos>,
    pub leaves: usize,
    pub rows: usize,
}

pub fn layout_tree(nodes: &[TreeNode], root: Option<&str>) -> TreeLayout {
    let by_tag: HashMap<_, _> = nodes.iter().map(|n| (n.tag.clone(), n.clone())).collect();
    let mut pos = HashMap::new();
    let mut leaf = 0usize;
    let mut max_depth = 0usize;

    fn place(
        tag: &str,
        depth: usize,
        seen: &mut HashSet<String>,
        by_tag: &HashMap<String, TreeNode>,
        pos: &mut HashMap<String, NodePos>,
        leaf: &mut usize,
        max_depth: &mut usize,
    ) -> f32 {
        let Some(n) = by_tag.get(tag) else {
            return 0.;
        };
        if seen.contains(tag) {
            return 0.;
        }
        seen.insert(tag.to_string());
        *max_depth = (*max_depth).max(depth);
        let kids: Vec<_> = n
            .children
            .iter()
            .filter(|c| by_tag.contains_key(*c) && !seen.contains(*c))
            .cloned()
            .collect();
        let x = if kids.is_empty() {
            let x = *leaf as f32 * XS;
            *leaf += 1;
            x
        } else {
            let xs: Vec<f32> = kids
                .iter()
                .map(|c| place(c, depth + 1, seen, by_tag, pos, leaf, max_depth))
                .collect();
            (xs.iter().copied().fold(f32::INFINITY, f32::min)
                + xs.iter().copied().fold(f32::NEG_INFINITY, f32::max))
                / 2.
        };
        pos.insert(tag.to_string(), NodePos {
            x,
            y: depth as f32 * YS,
        });
        x
    }

    let mut seen = HashSet::new();
    if let Some(r) = root {
        if by_tag.contains_key(r) {
            place(r, 0, &mut seen, &by_tag, &mut pos, &mut leaf, &mut max_depth);
        }
    }

    let orphan_row = max_depth + 1;
    for n in nodes {
        if !pos.contains_key(&n.tag) {
            let x = leaf as f32 * XS;
            leaf += 1;
            pos.insert(
                n.tag.clone(),
                NodePos {
                    x,
                    y: orphan_row as f32 * YS,
                },
            );
            max_depth = max_depth.max(orphan_row);
        }
    }

    let leaves = leaf.max(1);
    let rows = max_depth + 1;
    let mut layout = TreeLayout { pos, leaves, rows };
    center_layout(&mut layout);
    layout
}

/// Shift every node so the card bounding box sits centred in [`content_size`].
fn center_layout(layout: &mut TreeLayout) {
    if layout.pos.is_empty() {
        return;
    }
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut top = f32::INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for p in layout.pos.values() {
        let (lx, ly) = card_origin(p);
        left = left.min(lx);
        right = right.max(lx + CARD_W);
        top = top.min(ly);
        bottom = bottom.max(ly + CARD_H);
    }
    let (cw, ch) = content_size(layout);
    let shift_x = (cw - (right - left)) / 2. - left;
    let shift_y = (ch - (bottom - top)) / 2. - top;
    for p in layout.pos.values_mut() {
        p.x += shift_x;
        p.y += shift_y;
    }
}

/// Full content bounds (pre-scale) needed to show every card plus gutters.
pub fn content_size(layout: &TreeLayout) -> (f32, f32) {
    let w = (layout.leaves.saturating_sub(1) as f32) * XS + CARD_W + 2. * GUTTER;
    let h = (layout.rows.saturating_sub(1) as f32) * YS + CARD_H + 2. * GUTTER;
    (w.max(CARD_W + 2. * GUTTER), h.max(CARD_H + 2. * GUTTER))
}

/// Top-left corner of a card in content space (pre-scale).
pub fn card_origin(pos: &NodePos) -> (f32, f32) {
    (pos.x + GUTTER, pos.y + OFFY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::TreeNode;

    fn node(tag: &str, parent: Option<&str>, children: &[&str]) -> TreeNode {
        TreeNode {
            tag: tag.into(),
            parent: parent.map(str::to_string),
            children: children.iter().map(|c| (*c).to_string()).collect(),
            up: true,
            ..Default::default()
        }
    }

    #[test]
    fn layout_tree_centers_in_content_box() {
        let nodes = vec![
            node("r", None, &["a", "b"]),
            node("a", Some("r"), &[]),
            node("b", Some("r"), &[]),
        ];
        let layout = layout_tree(&nodes, Some("r"));
        let (cw, ch) = content_size(&layout);
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        let mut top = f32::INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for p in layout.pos.values() {
            let (lx, ly) = card_origin(p);
            left = left.min(lx);
            right = right.max(lx + CARD_W);
            top = top.min(ly);
            bottom = bottom.max(ly + CARD_H);
        }
        let bbox_w = right - left;
        let bbox_h = bottom - top;
        assert!((left - (cw - bbox_w) / 2.).abs() < 0.01);
        assert!((top - (ch - bbox_h) / 2.).abs() < 0.01);
    }
}
