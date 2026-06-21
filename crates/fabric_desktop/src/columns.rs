//! Single source of truth for run-table columns (header + row widths, sort keys, cell render).

use crate::format::{fmt_ago, fmt_epoch, fmt_eta, fmt_num, status_label};
use crate::sparkline::{sparkline_cell, SPARK_W};
use gpui::Path;
use std::sync::Arc;
use crate::theme::Theme;
use fabric_types::{RunScalars, SortColumn};
use gpui::{div, prelude::*, px, rgb, Div, Pixels, Rgba, SharedString};

/// Horizontal inset inside the TREND column (sparkline sits centered between these).
pub const TREND_PAD_X: Pixels = px(8.);
pub const TREND_COL_W: Pixels = px(SPARK_W + 16.);
pub const GROUP_COL_W: Pixels = px(120.);

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

pub const EYE_COL_W: Pixels = px(24.);

pub const RUN_TABLE: &[Column] = &[
    Column {
        id: "eye",
        label: "",
        width: ColWidth::Fixed(EYE_COL_W),
        sort: None,
    },
    Column {
        id: "status",
        label: "ST",
        width: ColWidth::Fixed(px(28.)),
        sort: Some(SortColumn::Status),
    },
    Column {
        id: "group",
        label: "GROUP",
        width: ColWidth::Fixed(GROUP_COL_W),
        sort: None,
    },
    Column {
        id: "name",
        label: "RUN",
        width: ColWidth::Flex,
        sort: Some(SortColumn::Name),
    },
    Column {
        id: "trend",
        label: "TREND",
        width: ColWidth::Fixed(TREND_COL_W),
        sort: None,
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

/// Minimum list width so every column fits in the War Room split pane.
pub const RUN_TABLE_MIN_W: Pixels = px(900.);

pub fn group_display(run: &RunScalars) -> SharedString {
    if !run.group.is_empty() && run.group != run.name {
        run.group.clone().into()
    } else {
        "—".into()
    }
}

pub fn header_label(col: &Column, active: bool, desc: bool) -> SharedString {
    if col.label.is_empty() {
        return SharedString::default();
    }
    if !active {
        return col.label.into();
    }
    let arrow = if desc { " ▼" } else { " ▲" };
    format!("{}{}", col.label, arrow).into()
}

pub fn render_cell(
    theme: &Theme,
    col: &Column,
    run: &RunScalars,
    hidden: bool,
    spark_values: Option<Arc<Path<gpui::Pixels>>>,
    spark_color: Rgba,
) -> Div {
    let dim = hidden;
    match col.id {
        "eye" => div().w(EYE_COL_W),
        "status" => theme.status_block(status_label(run.status.as_deref())),
        "group" => theme.cell_fixed(
            GROUP_COL_W,
            if dim { theme.text_dim } else { theme.text_dim },
            group_display(run),
        ),
        "name" => theme.cell_flex(
            if dim { theme.text_dim } else { theme.link },
            run.name.clone(),
        ),
        "trend" => div()
            .flex_shrink_0()
            .w(TREND_COL_W)
            .px(TREND_PAD_X)
            .flex()
            .items_center()
            .justify_center()
            .child(sparkline_cell(theme, spark_values, spark_color, dim)),
        "pod" => theme.cell_fixed(
            px(100.),
            if dim { theme.text_dim } else { theme.text_dim },
            run.pod.clone(),
        ),
        "fleet" => theme.cell_fixed(
            px(88.),
            if dim { theme.text_dim } else { theme.text_dim },
            run.fleet.clone(),
        ),
        "best" => theme.cell_fixed(
            px(72.),
            if dim { theme.text_dim } else { theme.data },
            fmt_num(run.best, 3),
        ),
        "epoch" => theme.cell_fixed(
            px(72.),
            if dim { theme.text_dim } else { theme.data },
            fmt_epoch(run.last_epoch, run.total_epochs),
        ),
        "eta" => theme.cell_fixed(
            px(52.),
            theme.text_dim,
            fmt_eta(run.eta_sec),
        ),
        "started" => theme.cell_fixed(
            px(108.),
            theme.text_dim,
            fmt_ago(run.created),
        ),
        _ => div().child("—"),
    }
}

pub fn header_shell(theme: &Theme, col: &Column, label: impl Into<SharedString>) -> Div {
    match col.width {
        ColWidth::Fixed(w) => theme.col_fixed(label, w),
        ColWidth::Flex => theme.col_flex(label),
    }
}

pub fn eye_button(theme: &Theme, hidden: bool, label: &'static str) -> Div {
    div()
        .flex_shrink_0()
        .w(EYE_COL_W)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.))
        .text_color(if hidden { theme.text_dim } else { theme.amber })
        .cursor_pointer()
        .hover(|s| s.bg(theme.panel_edge))
        .child(label)
}

pub fn rgba_from_hex(hex: u32) -> Rgba {
    rgb(hex & 0xffffff)
}
