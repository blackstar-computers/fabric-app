//! Blackstar star mark + FABRIC wordmark (from fabric/web_app Logo.tsx + brand.css).

use gpui::{div, prelude::*, px, svg, Div, FontWeight, Pixels};

use crate::theme::Theme;

const STAR: &str = "blackstar.svg";
const STAR_SIZE: Pixels = px(14.);
const WORD_SIZE: Pixels = px(11.);

/// Star + uppercase wordmark for the app toolbar.
pub fn brand(theme: &Theme) -> Div {
    brand_inner(theme).mr(px(10.))
}

/// Centered brand block for the login screen.
pub fn brand_header(theme: &Theme) -> Div {
    brand_inner(theme)
}

fn brand_inner(theme: &Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            svg()
                .path(STAR)
                .size(STAR_SIZE)
                .text_color(theme.text),
        )
        .child(
            div()
                .text_size(WORD_SIZE)
                .line_height(WORD_SIZE)
                .font_family(theme.font_brand.clone())
                .font_weight(FontWeight::BOLD)
                .text_color(theme.text)
                .child("FABRIC"),
        )
}
