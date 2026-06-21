//! Single source of truth for run-table columns (header + row widths, sort keys, cell render).

use crate::format::{fmt_ago, fmt_epoch, fmt_eta, fmt_num, status_label};
use crate::theme::Theme;
use fabric_types::{RunScalars, SortColumn};
use gpui::{div, prelude::*, px, Div, Pixels, SharedString};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColWidth {
    Fixed(Pixels),
    Flex,
}

#[derive(Clone, Copy, Debug)]
pub struct Column {
    pub id: &'static str,
    pub label: &'static str,
    pub width: ColWidth,
    pub sort: Option<SortColumn>,
}

pub const RUN_TABLE: &[Column] = &[
    Column {
        id: "status",
        label: "ST",
        width: ColWidth::Fixed(px(28.)),
        sort: Some(SortColumn::Status),
    },
    Column {
        id: "name",
        label: "RUN",
        width: ColWidth::Flex,
        sort: Some(SortColumn::Name),
    },
    Column {
        id: "pod",
        label: "POD",
        width: ColWidth::Fixed(px(100.)),
        sort: None,
    },
    Column {
        id: "fleet",
        label: "FLEET",
        width: ColWidth::Fixed(px(88.)),
        sort: None,
    },
    Column {
        id: "best",
        label: "BEST",
        width: ColWidth::Fixed(px(72.)),
        sort: Some(SortColumn::Best),
    },
    Column {
        id: "epoch",
        label: "EPOCH",
        width: ColWidth::Fixed(px(72.)),
        sort: Some(SortColumn::Epoch),
    },
    Column {
        id: "eta",
        label: "ETA",
        width: ColWidth::Fixed(px(52.)),
        sort: None,
    },
    Column {
        id: "started",
        label: "STARTED",
        width: ColWidth::Fixed(px(108.)),
        sort: Some(SortColumn::Created),
    },
];

/// Minimum list width so every column (incl. STARTED) fits; used when the run table is in a
/// narrow split pane beside the War Room. Fixed cols + vrules + row padding + min RUN flex.
pub const RUN_TABLE_MIN_W: Pixels = px(680.);

pub fn header_label(col: &Column, active: bool, desc: bool) -> SharedString {
    if !active {
        return col.label.into();
    }
    let arrow = if desc { " ▼" } else { " ▲" };
    format!("{}{}", col.label, arrow).into()
}

pub fn render_cell(theme: &Theme, col: &Column, run: &RunScalars) -> Div {
    match col.id {
        "status" => theme.status_block(status_label(run.status.as_deref())),
        "name" => theme.cell_flex(theme.link, run.name.clone()),
        "pod" => theme.cell_fixed(px(100.), theme.text_dim, run.pod.clone()),
        "fleet" => theme.cell_fixed(px(88.), theme.text_dim, run.fleet.clone()),
        "best" => theme.cell_fixed(px(72.), theme.data, fmt_num(run.best, 3)),
        "epoch" => theme.cell_fixed(
            px(72.),
            theme.data,
            fmt_epoch(run.last_epoch, run.total_epochs),
        ),
        "eta" => theme.cell_fixed(px(52.), theme.text_dim, fmt_eta(run.eta_sec)),
        "started" => theme.cell_fixed(px(108.), theme.text_dim, fmt_ago(run.created)),
        _ => div().child("—"),
    }
}

pub fn header_shell(theme: &Theme, col: &Column, label: impl Into<SharedString>) -> Div {
    match col.width {
        ColWidth::Fixed(w) => theme.col_fixed(label, w),
        ColWidth::Flex => theme.col_flex(label),
    }
}
