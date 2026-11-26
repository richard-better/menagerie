//! Colours and type. The app has one look: dark, with colour only where it
//! carries meaning (green healthy or charging, amber needs attention).

use gpui::{Hsla, Rgba, rgb};

pub const SANS: &str = ".SystemUIFont";
pub const MONO: &str = "Menlo";

pub fn bg() -> Rgba {
    rgb(0x18181a)
}
pub fn panel() -> Rgba {
    rgb(0x1f1f22)
}
pub fn raise() -> Rgba {
    rgb(0x26262a)
}
pub fn line() -> Rgba {
    rgb(0x303035)
}
pub fn ink() -> Rgba {
    rgb(0xf2f2f4)
}
/// Secondary text. 4.99:1 on `panel()`, so it passes WCAG AA.
pub fn mute() -> Rgba {
    rgb(0x8d8d94)
}
pub fn green() -> Rgba {
    rgb(0x5ad18a)
}
pub fn amber() -> Rgba {
    rgb(0xe9c548)
}
pub fn grey_dot() -> Rgba {
    rgb(0x8d8d94)
}
/// A lit dot in a healthy meter or chart.
pub fn dot_on() -> Hsla {
    rgb(0xc8c8ce).into()
}
/// An unlit dot.
pub fn dot_off() -> Hsla {
    rgb(0x3a3a40).into()
}
