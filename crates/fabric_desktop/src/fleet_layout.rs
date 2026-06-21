//! Relay-tree layout — assigns each node an (x, y) in content space.
//!
//! Coordinates are content-local pixels (pre-scale). Cards are centred on the
//! subtree they root; [`OFFX`]/[`OFFY`] add the outer gutter so nothing clips
//! the viewport edge. The board and edge painter share these constants so DIV
//! node cards and the Bézier links behind them stay pixel-aligned.

use fabric_types::TreeNode;
use std::collections::{HashMap, HashSet};

pub const CARD_W: f32 = 168.;
pub const CARD_H: f32 = 88.;
pub const XS: f32 = 196.;
pub const YS: f32 = 132.;
pub const GUTTER: f32 = 18.;
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
    TreeLayout { pos, leaves, rows }
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
