//! Colours and type. The app has one look: dark, from the brand's forest ink,
//! with warm ivory text and colour only where it carries meaning (green
//! healthy or charging, amber needs attention, red failed) and the brand's
//! sage on the one primary action.
//!
//! Every colour the views use is here. Contrast ratios are against `panel()`.

use gpui::{Hsla, Rgba, rgb};

pub const SANS: &str = ".SystemUIFont";
pub const MONO: &str = "Menlo";

/// The window and popover background: the brand's forest ink.
pub fn bg() -> Rgba {
    rgb(0x151c19)
}
/// `bg()` under the pointer.
pub fn bg_hover() -> Rgba {
    rgb(0x19211d)
}
/// The selected machine's details.
pub fn panel() -> Rgba {
    rgb(0x1b231f)
}
/// Buttons and boxes on the panel.
pub fn raise() -> Rgba {
    rgb(0x232d28)
}
/// `raise()` under the pointer.
pub fn raise_hover() -> Rgba {
    rgb(0x2c3832)
}
/// A selected segment or option.
pub fn selected() -> Rgba {
    rgb(0x33413a)
}
pub fn line() -> Rgba {
    rgb(0x2e3a34)
}
/// A focused field's border.
pub fn focus() -> Rgba {
    rgb(0x4c5c54)
}
/// Sunken areas: the terminal, the log, text fields.
pub fn well() -> Rgba {
    rgb(0x0e1311)
}
/// Text: the brand's warm ivory. 13.9:1.
pub fn ink() -> Rgba {
    rgb(0xf0eee8)
}
/// Output in the log and terminal, a step down from `ink()`. 8.7:1.
pub fn soft() -> Rgba {
    rgb(0xb9c1bc)
}
/// Secondary text. 5.8:1, and 5.1:1 on `raise()`, so it passes WCAG AA.
pub fn mute() -> Rgba {
    rgb(0x939e97)
}
/// Disabled controls' text.
pub fn faint() -> Rgba {
    rgb(0x5e6963)
}
/// Healthy, charging. 8.3:1.
pub fn green() -> Rgba {
    rgb(0x7ccb93)
}
/// Needs attention: the brand's amber. 7.3:1.
pub fn amber() -> Rgba {
    rgb(0xd9a657)
}
/// Failed. 4.9:1.
pub fn red() -> Rgba {
    rgb(0xe2685a)
}
/// The primary action's fill, and links: the brand's soft sage.
pub fn accent() -> Rgba {
    rgb(0xc5dcc0)
}
pub fn accent_hover() -> Rgba {
    rgb(0xd5e7d1)
}
/// Text on `accent()`: the brand's forest ink. 11.9:1.
pub fn on_accent() -> Rgba {
    rgb(0x151c19)
}
pub fn grey_dot() -> Rgba {
    mute()
}
/// A lit dot in a healthy meter or chart.
pub fn dot_on() -> Hsla {
    rgb(0xcfd8d1).into()
}
/// An unlit dot.
pub fn dot_off() -> Hsla {
    rgb(0x2f3b35).into()
}
/// A device glyph's body, and its outline when unselected and selected.
pub fn device_body() -> Rgba {
    rgb(0x26302b)
}
pub fn device_edge() -> Rgba {
    rgb(0x56625c)
}
pub fn device_edge_active() -> Rgba {
    rgb(0x9aa69f)
}
/// A device glyph's lit screen: a deeper sage.
pub fn device_screen() -> Rgba {
    rgb(0x6e9c86)
}
