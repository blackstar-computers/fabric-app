//! Input / recon preview thumbnails — rendered in the right controls rail under cell info.

use crate::format::fmt_num;
use crate::theme::Theme;
use crate::topology::TopologyView;
use gpui::{div, img, prelude::*, px, RenderImage};
use std::sync::Arc;

const PREVIEW_PX: f32 = 72.;

pub fn previews_column(view: &TopologyView, theme: &Theme) -> impl IntoElement {
    let input_img = view.input_preview_image();
    let recon_img = view.recon_preview_image(view.scrub_tick);
    let psnr = view
        .step_resp
        .as_ref()
        .and_then(|r| r.psnr.get(view.scrub_tick))
        .and_then(|p| *p);

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_label(theme, "PREVIEWS"))
        .child(preview_card(
            theme,
            format!("INPUT · {}", view.input_source.to_ascii_uppercase()),
            input_img,
            None,
        ))
        .child(preview_card(theme, "RECON".to_string(), recon_img, psnr))
}

fn section_label(theme: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(9.))
        .text_color(theme.text_dim)
        .child(text)
}

fn preview_card(
    theme: &Theme,
    title: String,
    image: Option<Arc<RenderImage>>,
    psnr: Option<f64>,
) -> impl IntoElement {
    let body = match image {
        Some(image) => img(image)
            .h(px(PREVIEW_PX))
            .w(px(PREVIEW_PX))
            .into_any_element(),
        None => div()
            .h(px(PREVIEW_PX))
            .w(px(PREVIEW_PX))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.))
            .text_color(theme.text_dim)
            .child("—")
            .into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme.amber_dim)
                .child(title),
        )
        .child(body)
        .when_some(psnr, |el, p| {
            el.child(
                div()
                    .text_size(px(9.))
                    .text_color(theme.live)
                    .child(format!("PSNR {}", fmt_num(Some(p), 2))),
            )
        })
}
