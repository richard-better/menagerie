//! A one-line text field. gpui 0.2.2 has none built in.
//!
//! It handles typing, pasting, deleting and moving the caret, and emits
//! `Submit` on Return. There is no selection and no input method support,
//! which a commit message doesn't need.

use gpui::{
    Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, Styled, Window, div, prelude::*, px,
};

use crate::theme;

/// Return was pressed.
pub struct Submit;

pub struct TextField {
    text: String,
    /// A byte offset on a char boundary.
    caret: usize,
    focus: FocusHandle,
    placeholder: SharedString,
    /// Whether the person has typed in it.
    edited: bool,
}

impl EventEmitter<Submit> for TextField {}

impl TextField {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            edited: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the person has typed in it, so programmatic text shouldn't
    /// replace theirs.
    pub fn edited(&self) -> bool {
        self.edited
    }

    /// Replace the text, as the program rather than the person.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = one_line(&text.into());
        self.caret = self.text.len();
        cx.notify();
    }

    fn insert(&mut self, text: &str) {
        let text = one_line(text);
        self.text.insert_str(self.caret, &text);
        self.caret += text.len();
        self.edited = true;
    }

    fn prev(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let cmd = k.modifiers.platform;
        match k.key.as_str() {
            "enter" => cx.emit(Submit),
            "backspace" if cmd => {
                self.text.replace_range(..self.caret, "");
                self.caret = 0;
                self.edited = true;
            }
            "backspace" => {
                let from = self.prev();
                self.text.replace_range(from..self.caret, "");
                self.caret = from;
                self.edited = true;
            }
            "delete" => {
                let to = self.next();
                self.text.replace_range(self.caret..to, "");
                self.edited = true;
            }
            "left" if cmd => self.caret = 0,
            "right" if cmd => self.caret = self.text.len(),
            "home" => self.caret = 0,
            "end" => self.caret = self.text.len(),
            "left" => self.caret = self.prev(),
            "right" => self.caret = self.next(),
            "v" if cmd => {
                if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    self.insert(&text);
                }
            }
            _ => match k.key_char.as_deref() {
                Some(typed)
                    if !cmd && !k.modifiers.control && !typed.chars().any(char::is_control) =>
                {
                    self.insert(typed)
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// Newlines and tabs become spaces: the field holds one line.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let caret = || div().w(px(1.5)).h(px(15.)).bg(theme::ink()).flex_none();
        let content = if self.text.is_empty() {
            div()
                .flex()
                .items_center()
                .when(focused, |el| el.child(caret()))
                .child(
                    div()
                        .text_color(theme::mute())
                        .child(self.placeholder.clone()),
                )
        } else {
            let (before, after) = self.text.split_at(self.caret);
            div()
                .flex()
                .items_center()
                .child(before.to_owned())
                .when(focused, |el| el.child(caret()))
                .child(after.to_owned())
        };
        div()
            .id("text-field")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_click(cx.listener(|this, _, window, _| this.focus.focus(window)))
            .h(px(32.))
            .px(px(10.))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded(px(7.))
            .bg(theme::well())
            .border_1()
            .border_color(if focused {
                theme::focus()
            } else {
                theme::line()
            })
            .text_size(px(12.5))
            .cursor_text()
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::one_line;

    #[test]
    fn pasted_lines_become_one() {
        assert_eq!(one_line("a\nb\tc"), "a b c");
    }
}
