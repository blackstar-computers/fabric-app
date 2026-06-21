//! Compact search field for the overview toolbar.

use crate::theme::Theme;
use gpui::{
    div, prelude::*, px, App, Context, FocusHandle, Focusable, KeyDownEvent, Keystroke, MouseDownEvent,
    MouseButton, Render, SharedString, Window,
};
use std::ops::Range;

pub struct SearchInput {
    focus_handle: FocusHandle,
    query: String,
    selection_anchor: usize,
    selection_focus: usize,
    blur_listener_active: bool,
}

impl SearchInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            query: String::new(),
            selection_anchor: 0,
            selection_focus: 0,
            blur_listener_active: false,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    fn selection_range(&self) -> Range<usize> {
        let lo = self.selection_anchor.min(self.selection_focus);
        let hi = self.selection_anchor.max(self.selection_focus);
        lo..hi
    }

    fn has_selection(&self) -> bool {
        self.selection_anchor != self.selection_focus
    }

    fn collapse_caret(&mut self, at: usize) {
        self.selection_anchor = at;
        self.selection_focus = at;
    }

    fn dismiss_selection(&mut self) {
        let at = self.selection_focus.min(self.query.len());
        self.collapse_caret(at);
    }

    fn ensure_blur_listener(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blur_listener_active {
            return;
        }
        self.blur_listener_active = true;
        let handle = self.focus_handle.clone();
        cx.on_blur(&handle, window, |this, _, cx| {
            this.dismiss_selection();
            cx.notify();
        })
        .detach();
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection_anchor = 0;
        self.selection_focus = self.query.len();
        cx.notify();
    }

    fn delete_selection(&mut self) -> bool {
        let range = self.selection_range();
        if range.is_empty() {
            return false;
        }
        self.query.replace_range(range.clone(), "");
        self.collapse_caret(range.start);
        true
    }

    fn insert_str(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.has_selection() {
            self.delete_selection();
        }
        let at = self.selection_anchor;
        self.query.insert_str(at, text);
        self.collapse_caret(at + text.len());
        cx.notify();
    }

    fn is_select_all_keystroke(keystroke: &Keystroke) -> bool {
        keystroke.key == "a" && (keystroke.modifiers.platform || keystroke.modifiers.control)
    }

    fn handle_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        if Self::is_select_all_keystroke(keystroke) {
            self.select_all(cx);
            return;
        }

        if keystroke.modifiers.platform || keystroke.modifiers.control || keystroke.modifiers.function
        {
            return;
        }

        match keystroke.key.as_str() {
            "backspace" => {
                if self.has_selection() {
                    self.delete_selection();
                } else if !self.query.is_empty() {
                    let at = self.selection_anchor.saturating_sub(1);
                    self.query.remove(at);
                    self.collapse_caret(at);
                }
                cx.notify();
            }
            "delete" => {
                if self.has_selection() {
                    self.delete_selection();
                } else if self.selection_anchor < self.query.len() {
                    self.query.remove(self.selection_anchor);
                }
                cx.notify();
            }
            "escape" => {
                self.query.clear();
                self.collapse_caret(0);
                cx.notify();
            }
            "left" => {
                if self.has_selection() {
                    self.collapse_caret(self.selection_range().start);
                } else {
                    self.collapse_caret(self.selection_anchor.saturating_sub(1));
                }
                cx.notify();
            }
            "right" => {
                if self.has_selection() {
                    self.collapse_caret(self.selection_range().end);
                } else {
                    self.collapse_caret((self.selection_anchor + 1).min(self.query.len()));
                }
                cx.notify();
            }
            _ => {
                if let Some(s) = keystroke.key_char.as_ref() {
                    if !s.chars().any(|c| c.is_control()) {
                        self.insert_str(s, cx);
                    }
                } else if keystroke.key.len() == 1 && !keystroke.modifiers.shift {
                    self.insert_str(&keystroke.key, cx);
                }
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left {
            self.focus_handle.focus(window, cx);
            self.collapse_caret(self.query.len());
            cx.notify();
        }
    }

    fn on_mouse_down_out(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_selection();
        window.blur();
        cx.notify();
    }
}

impl Focusable for SearchInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SearchInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_blur_listener(window, cx);
        let theme = Theme::get(cx);
        let focused = self.focus_handle.is_focused(window);

        let (border_color, thick) = if focused {
            (theme.amber, true)
        } else {
            (theme.amber_dim, false)
        };

        let shell = div()
            .id("search-input")
            .track_focus(&self.focus_handle)
            .tab_index(0)
            .flex_none()
            .w(px(160.))
            .h(px(18.))
            .px(px(6.))
            .flex()
            .items_center()
            .border_color(border_color)
            .bg(if focused {
                theme.panel
            } else {
                theme.panel_edge
            })
            .text_size(px(10.))
            .cursor_text()
            .child(render_query_text(
                &theme,
                &self.query,
                self.selection_range(),
                focused,
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::on_mouse_down),
            )
            .on_mouse_down_out(cx.listener(Self::on_mouse_down_out))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.handle_key(&event.keystroke, cx);
            }));

        if thick {
            shell.border_2()
        } else {
            shell.border_1()
        }
    }
}

fn render_query_text(
    theme: &Theme,
    query: &str,
    selection: Range<usize>,
    focused: bool,
) -> impl IntoElement {
    if query.is_empty() {
        return div()
            .text_color(theme.text_dim)
            .child(SharedString::from("SEARCH…"))
            .into_any_element();
    }

    if focused && !selection.is_empty() {
        let before = &query[..selection.start];
        let selected = &query[selection.start..selection.end];
        let after = &query[selection.end..];
        return div()
            .flex()
            .items_center()
            .overflow_hidden()
            .truncate()
            .child(
                div()
                    .text_color(theme.text)
                    .child(before.to_string()),
            )
            .child(
                div()
                    .px(px(1.))
                    .bg(theme.amber_dim)
                    .text_color(theme.data)
                    .child(selected.to_string()),
            )
            .child(
                div()
                    .text_color(theme.text)
                    .child(after.to_string()),
            )
            .into_any_element();
    }

    div()
        .truncate()
        .text_color(theme.text)
        .child(SharedString::from(query.to_string()))
        .into_any_element()
}
