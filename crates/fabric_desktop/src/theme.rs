//! Bloomberg-terminal palette + block helpers (sharp corners, monospace, dense).
//!
//! Colors and layout tokens live on [`Theme`], installed once via [`Theme::init`]
//! and read anywhere with [`Theme::get`] (Zed `GlobalColors` pattern).

use gpui::{
    div, prelude::*, px, rgb, App, AppContext, Div, FontWeight, Global, Pixels, Rgba,
    SharedString, UpdateGlobal,
};

/// Application-wide visual tokens (palette, typography, density).
#[derive(Clone, Debug)]
pub struct Theme {
    pub font_mono: SharedString,
    pub font_brand: SharedString,
    pub row_h: f32,
    pub bg: Rgba,
    pub panel: Rgba,
    pub panel_edge: Rgba,
    pub border: Rgba,
    pub border_bright: Rgba,
    pub amber: Rgba,
    pub amber_dim: Rgba,
    pub text: Rgba,
    pub text_dim: Rgba,
    pub data: Rgba,
    pub link: Rgba,
    pub live: Rgba,
    pub warn: Rgba,
    pub idle: Rgba,
    pub row_a: Rgba,
    pub row_b: Rgba,
}

/// Bordered chrome pill sizing (buttons, LIVE/POLL, chips).
const PILL_PX: Pixels = px(6.);
const PILL_PY: Pixels = px(3.);
const PILL_BTN_TEXT: Pixels = px(11.);
const PILL_CHIP_TEXT: Pixels = px(10.);

impl Theme {
    /// Default Bloomberg-terminal palette (black field, amber chrome).
    pub fn bloomberg() -> Self {
        Self {
            font_mono: "SF Mono".into(),
            font_brand: "Space Grotesk".into(),
            row_h: 20.,
            bg: rgb(0x000000),
            panel: rgb(0x0a0a0a),
            panel_edge: rgb(0x1a1a1a),
            border: rgb(0x333333),
            border_bright: rgb(0x555555),
            amber: rgb(0xffa028),
            amber_dim: rgb(0x996018),
            text: rgb(0xcccccc),
            text_dim: rgb(0x777777),
            data: rgb(0xe8e8e8),
            link: rgb(0xffcc66),
            live: rgb(0x39ff14),
            warn: rgb(0xff3333),
            idle: rgb(0x666666),
            row_a: rgb(0x000000),
            row_b: rgb(0x080808),
        }
    }

    /// Install the default theme on the app (call once at startup).
    pub fn init(cx: &mut App) {
        GlobalTheme::set_global(cx, GlobalTheme(Self::bloomberg()));
    }

    /// Clone the active theme (cheap — palette + one shared string).
    pub fn get<C: AppContext>(cx: &C) -> Self {
        cx.read_global(|GlobalTheme(theme), _| theme.clone())
    }

    pub fn shell(&self) -> Div {
        div()
            .size_full()
            .bg(self.bg)
            .text_color(self.text)
            .font_family(self.font_mono.clone())
            .text_size(px(11.))
            .flex()
            .flex_col()
    }

    pub fn block(&self) -> Div {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(self.panel)
            .border_1()
            .border_color(self.border_bright)
    }

    pub fn vrule(&self) -> Div {
        div()
            .flex_none()
            .w(px(1.))
            .h_full()
            .mx_1()
            .bg(self.border)
    }

    fn live_pill_styles(&self, sse_live: bool) -> (&'static str, Rgba, Rgba, Rgba) {
        if sse_live {
            (" LIVE ", self.live, rgb(0x001800), self.live)
        } else {
            (" POLL ", self.text_dim, rgb(0x111111), self.border)
        }
    }

    pub fn title_bar_live_pill(&self, sse_live: bool) -> Div {
        let (tag, tag_color, fill, border) = self.live_pill_styles(sse_live);
        self.title_bar_control()
            .border_1()
            .bg(fill)
            .border_color(border)
            .text_color(tag_color)
            .child(self.pill_label(PILL_BTN_TEXT, tag))
    }

    /// Fixed-width refresh control so label swaps do not break click targets.
    pub fn title_bar_refresh_button(&self, fetching: bool) -> Div {
        self.title_bar_button(
            if fetching { " FETCH… " } else { " ↻ REFRESH " },
            fetching,
        )
        .min_w(px(96.))
        .justify_center()
    }

    pub fn status_bar(&self, text: impl Into<SharedString>) -> Div {
        div()
            .flex_none()
            .w_full()
            .h(px(22.))
            .flex()
            .items_center()
            .px(px(8.))
            .bg(self.panel_edge)
            .border_t_1()
            .border_color(self.border)
            .text_size(px(10.))
            .text_color(self.amber_dim)
            .child(text.into())
    }

    pub fn table_header_row(&self) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .w_full()
            .h(px(22.))
            .px(px(8.))
            .bg(self.panel_edge)
            .border_b_1()
            .border_color(self.border)
            .text_size(px(10.))
            .text_color(self.amber)
            .font_weight(FontWeight::SEMIBOLD)
    }

    pub fn col_fixed(&self, label: impl Into<SharedString>, width: Pixels) -> Div {
        div()
            .flex_shrink_0()
            .w(width)
            .truncate()
            .child(label.into())
    }

    pub fn col_flex(&self, label: impl Into<SharedString>) -> Div {
        div().flex_1().overflow_hidden().truncate().child(label.into())
    }

    /// Bordered pill shell — padding-sized like the original chrome; label sits in a tight inner
    /// wrapper so SF Mono centers visually instead of riding high in the line box.
    pub fn pill(&self) -> Div {
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .px(PILL_PX)
            .py(PILL_PY)
            .border_1()
            .border_color(self.border)
    }

    /// Inner label wrapper: line height matches font size; tiny top pad optically centers mono caps.
    fn pill_label(&self, size: Pixels, label: impl Into<SharedString>) -> Div {
        div()
            .text_size(size)
            .line_height(size)
            .pt(px(1.))
            .child(label.into())
    }

    /// Shared bordered tag shell — pod, fleet, and tone pills all use the same box + type size.
    fn tag_pill(
        &self,
        label: impl Into<SharedString>,
        border: Rgba,
        fg: Rgba,
    ) -> Div {
        self.pill()
            .border_color(border)
            .text_color(fg)
            .child(self.pill_label(PILL_CHIP_TEXT, label))
    }

    pub fn title_button(&self, label: impl Into<SharedString>, active: bool) -> Div {
        self.pill()
            .border_color(if active { self.amber } else { self.border })
            .text_color(if active { self.amber } else { self.text_dim })
            .cursor_pointer()
            .hover(|s| s.bg(self.panel_edge).text_color(self.amber))
            .child(self.pill_label(PILL_BTN_TEXT, label))
    }

    /// Full-height title-bar control (stretches to the unified window chrome row).
    fn title_bar_control(&self) -> Div {
        div()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .px(px(10.))
    }

    pub fn title_bar_button(&self, label: impl Into<SharedString>, active: bool) -> Div {
        self.title_bar_control()
            .text_size(PILL_BTN_TEXT)
            .line_height(PILL_BTN_TEXT)
            .border_1()
            .bg(self.panel_edge)
            .border_color(if active { self.amber } else { self.border })
            .text_color(if active { self.amber } else { self.text_dim })
            .cursor_pointer()
            .occlude()
            .hover(|s| s.bg(self.panel).text_color(self.amber))
            .child(label.into())
    }

    /// Kind-filter segment pill in the overview toolbar.
    pub fn filter_pill(&self, active: bool, label: &'static str) -> Div {
        self.pill()
            .border_color(if active { self.amber } else { self.border })
            .text_color(if active { self.amber } else { self.text_dim })
            .cursor_pointer()
            .hover(|s| s.bg(self.panel_edge).text_color(self.amber))
            .child(self.pill_label(PILL_CHIP_TEXT, label))
    }

    /// Pod / fleet tag in the War Room command bar (same size as [`Self::tone_chip`]).
    pub fn chip(&self, text: impl Into<SharedString>) -> Div {
        self.tag_pill(text, self.border_bright, self.text)
    }

    pub fn tone_chip(&self, tone: fabric_health::Tone, label: &'static str) -> Div {
        let color = self.tone_color(tone);
        self.tag_pill(label, color, color)
    }

    pub fn col_row(&self, children: impl IntoIterator<Item = impl IntoElement>) -> Div {
        let mut row = div().flex().items_center().w_full().overflow_hidden();
        for (i, child) in children.into_iter().enumerate() {
            if i > 0 {
                row = row.child(self.vrule());
            }
            row = row.child(child);
        }
        row
    }

    pub fn status_block(&self, status: &str) -> Div {
        let (label, color, fill) = match status {
            "running" => ("RUN", self.live, rgb(0x001a00)),
            "starting" => ("STT", self.amber, rgb(0x1a1000)),
            "stopping" => ("STP", self.warn, rgb(0x1a0000)),
            "idle" => ("IDL", self.idle, rgb(0x111111)),
            _ => ("---", self.text_dim, rgb(0x111111)),
        };

        div()
            .flex_shrink_0()
            .w(px(28.))
            .child(
                div()
                    .w(px(26.))
                    .h(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(fill)
                    .border_1()
                    .border_color(color)
                    .text_size(px(9.))
                    .text_color(color)
                    .child(label),
            )
    }

    pub fn cell_fixed(
        &self,
        width: Pixels,
        color: Rgba,
        value: impl Into<SharedString>,
    ) -> Div {
        div()
            .flex_shrink_0()
            .w(width)
            .overflow_hidden()
            .text_color(color)
            .truncate()
            .child(value.into())
    }

    pub fn cell_flex(&self, color: Rgba, value: impl Into<SharedString>) -> Div {
        div()
            .flex_1()
            .overflow_hidden()
            .text_color(color)
            .truncate()
            .child(value.into())
    }

    pub fn tone_color(&self, tone: fabric_health::Tone) -> Rgba {
        use fabric_health::Tone;
        match tone {
            Tone::Good => self.live,
            Tone::Warn => self.amber,
            Tone::Bad => self.warn,
            Tone::Neutral => self.text_dim,
        }
    }
}

#[derive(Clone, Debug)]
struct GlobalTheme(Theme);

impl Global for GlobalTheme {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloomberg_has_black_field() {
        let t = Theme::bloomberg();
        assert_eq!(t.bg, rgb(0x000000));
        assert_eq!(t.amber, rgb(0xffa028));
    }
}
