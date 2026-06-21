//! Sign-in gate — Google SSO or saved service token.

use crate::auth::{spawn_google_sign_in, spawn_saved_token_sign_in};
use crate::brand::brand_header;
use crate::network::AppUiMsg;
use crate::theme::Theme;
use fabric_api::SessionBundle;
use futures::channel::mpsc::UnboundedSender;
use gpui::{div, prelude::*, px, Context, Render, SharedString, Window};

pub struct LoginView {
    ui_tx: Option<UnboundedSender<AppUiMsg>>,
    error: Option<SharedString>,
    busy: bool,
}

impl LoginView {
    pub fn new() -> Self {
        Self {
            ui_tx: None,
            error: None,
            busy: false,
        }
    }

    pub fn attach_ui(&mut self, ui_tx: UnboundedSender<AppUiMsg>) {
        self.ui_tx = Some(ui_tx);
    }

    pub fn on_auth_failed(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(message.into());
        cx.notify();
    }

    fn sign_in_google(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(ui_tx) = self.ui_tx.clone() else {
            return;
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        spawn_google_sign_in(ui_tx, SessionBundle::sso_portal_url());
    }

    fn sign_in_saved_token(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(ui_tx) = self.ui_tx.clone() else {
            return;
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        spawn_saved_token_sign_in(ui_tx);
    }
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        let busy = self.busy;
        let err = self.error.clone();

        theme
            .shell()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(320.))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .px(px(28.))
                    .py(px(32.))
                    .bg(theme.panel)
                    .border_1()
                    .border_color(theme.border_bright)
                    .child(brand_header(&theme))
                    .child(
                        theme
                            .title_button(if busy { " SIGNING IN… " } else { " SIGN IN " }, !busy)
                            .id("sign-in-google")
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.sign_in_google(cx)),
                            ),
                    )
                    .child(
                        theme
                            .title_button(" USE SAVED TOKEN ", !busy)
                            .id("sign-in-token")
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.sign_in_saved_token(cx)),
                            ),
                    )
                    .when_some(err, |el, e| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.warn)
                                .child(e),
                        )
                    }),
            )
    }
}
