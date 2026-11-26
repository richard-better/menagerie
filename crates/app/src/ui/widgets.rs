//! Small pieces both views are built from.

use gpui::{
    App, Bounds, ClickEvent, Div, ElementId, Hsla, IntoElement, ParentElement, Pixels, Rgba,
    SharedString, Stateful, Styled, Window, canvas, div, fill, point, prelude::*, px, rgb, size,
    svg,
};
use menagerie_fleet::{Device, Service};

use crate::store::Status;
use crate::theme;

/// The coloured dot before a machine's name: green online, amber needs a
/// sync, grey no agent, hollow offline.
pub fn status_dot(status: Status) -> Div {
    let dot = div().size(px(7.)).rounded_full().flex_none();
    match status {
        Status::InSync { dirty: false } | Status::Online => dot.bg(theme::green()),
        Status::InSync { dirty: true } | Status::Drift { .. } => dot.bg(theme::amber()),
        Status::NoAgent => dot.bg(theme::grey_dot()),
        Status::Offline => dot.border_1().border_color(theme::grey_dot()),
    }
}

/// A drawing of the machine, at `scale` (1.0 is the window strip's size,
/// 80x44pt).
pub fn device_glyph(device: Device, scale: f32, active: bool) -> Div {
    let s = |v: f32| px(v * scale);
    let edge = if active { rgb(0x8a8a92) } else { rgb(0x5a5a62) };
    let body = rgb(0x2b2b30);
    let frame = div()
        .w(s(80.))
        .h(s(44.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center();
    match device {
        Device::Laptop => frame
            .child(
                div()
                    .w(s(52.))
                    .h(s(32.))
                    .rounded(s(3.))
                    .border_1()
                    .border_color(edge)
                    .bg(body)
                    .p(s(3.))
                    .child(div().size_full().rounded(s(1.5)).bg(rgb(0x3d6fd8))),
            )
            .child(div().w(s(68.)).h(s(4.)).rounded(s(2.)).bg(edge)),
        Device::Desktop => frame
            .child(
                div()
                    .w(s(50.))
                    .h(s(30.))
                    .rounded(s(3.))
                    .border_1()
                    .border_color(edge)
                    .bg(body),
            )
            .child(div().w(s(6.)).h(s(6.)).bg(edge))
            .child(div().w(s(22.)).h(s(3.)).rounded(s(1.5)).bg(edge)),
        Device::Server => frame.gap(s(3.)).children((0..3).map(|_| {
            div()
                .w(s(44.))
                .h(s(10.))
                .rounded(s(2.))
                .border_1()
                .border_color(edge)
                .bg(body)
                .flex()
                .items_center()
                .pl(s(4.))
                .child(div().size(s(3.)).rounded_full().bg(theme::green()))
        })),
        Device::Box => frame.child(
            div()
                .w(s(36.))
                .h(s(28.))
                .rounded(s(3.))
                .border_1()
                .border_color(edge)
                .bg(body)
                .flex()
                .flex_col()
                .justify_between()
                .p(s(5.))
                .child(div().size(s(4.)).rounded_full().bg(theme::green()))
                .child(div().w(s(22.)).h(s(3.)).rounded(s(1.5)).bg(edge)),
        ),
        Device::Phone => frame.child(
            div()
                .w(s(18.))
                .h(s(36.))
                .rounded(s(4.))
                .border_1()
                .border_color(edge)
                .bg(body)
                .flex()
                .justify_center()
                .pt(s(2.))
                .child(div().w(s(8.)).h(s(2.)).rounded(s(1.)).bg(edge)),
        ),
        Device::Other => frame.child(
            div()
                .size(s(30.))
                .rounded(s(6.))
                .border_1()
                .border_color(edge)
                .bg(body),
        ),
    }
}

/// The colour behind a service's letter. Known icon keys get their brand's
/// colour; anything else gets a stable colour from its name.
fn service_colour(service: &Service) -> Rgba {
    match service.icon.as_deref() {
        Some("jellyfin") => rgb(0x7f5bc8),
        Some("jellyseerr") => rgb(0x5b4bdb),
        Some("home-assistant") => rgb(0x18bcf2),
        Some("plex") => rgb(0xd9a20b),
        Some("grafana") => rgb(0xe8622c),
        _ => {
            const PALETTE: [u32; 6] = [0x4c7bd9, 0x3aa38a, 0xb35c9e, 0xc4793a, 0x6a6fd1, 0x4a9fb5];
            let hash = service
                .name
                .bytes()
                .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
            rgb(PALETTE[(hash as usize) % PALETTE.len()])
        }
    }
}

/// A service's square icon. Real app icons come later; until then, its
/// first letter on its colour.
pub fn service_icon(service: &Service, side: f32) -> Div {
    let letter: SharedString = match service.icon.as_deref() {
        // Two services that start with the same letter, side by side.
        Some("jellyseerr") => "S".into(),
        _ => service
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            .into(),
    };
    div()
        .size(px(side))
        .flex_none()
        .rounded(px(side * 0.27))
        .bg(service_colour(service))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(0xffffff))
        .text_size(px(side * 0.48))
        .font_weight(gpui::FontWeight::BOLD)
        .child(letter)
}

/// A service icon that opens the service when clicked.
pub fn service_link(id: impl Into<ElementId>, service: &Service, side: f32) -> Stateful<Div> {
    let url = service.url.clone();
    service_icon(service, side)
        .id(id)
        .cursor_pointer()
        .on_click(move |_, _, cx| cx.open_url(&url))
}

/// A small square button with an icon in it.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    side: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(side))
        .flex_none()
        .rounded(px(side * 0.26))
        .bg(theme::raise())
        .border_1()
        .border_color(theme::line())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x2e2e33)))
        .on_click(on_click)
        .child(
            svg()
                .path(icon)
                .size(px(side * 0.55))
                .text_color(theme::mute()),
        )
}

/// A text button. Disabled buttons are drawn at reduced contrast and do
/// nothing.
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    let button = div()
        .id(id)
        .px(px(11.))
        .py(px(5.))
        .rounded(px(7.))
        .bg(theme::raise())
        .border_1()
        .border_color(theme::line())
        .text_size(px(12.))
        .child(label.into());
    if enabled {
        button
            .text_color(theme::ink())
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x2e2e33)))
    } else {
        button.text_color(rgb(0x5f5f66))
    }
}

/// A dotted meter: two rows of small squares, lit up to `fraction`.
/// `None` draws it unlit, for a value nobody has reported.
pub fn meter(fraction: Option<f32>, lit: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            window.paint_quad(
                fill(bounds, theme::raise())
                    .corner_radii(px(4.))
                    .border_widths(px(1.))
                    .border_color(theme::line()),
            );
            let pitch = 6.0;
            let dot = 3.4;
            let width = f32::from(bounds.size.width);
            let count = ((width - 6.0) / pitch).floor().max(0.0) as usize;
            for i in 0..count {
                let on = fraction.is_some_and(|f| (i as f32) / (count as f32) < f);
                for row in 0..2 {
                    let colour = if on {
                        if row == 0 { lit } else { lit.opacity(0.75) }
                    } else {
                        theme::dot_off().opacity(0.6)
                    };
                    let origin = bounds.origin
                        + point(px(4.0 + i as f32 * pitch), px(3.5 + row as f32 * 5.0));
                    window.paint_quad(
                        fill(Bounds::new(origin, size(px(dot), px(dot))), colour)
                            .corner_radii(px(0.6)),
                    );
                }
            }
        },
    )
    .w_full()
    .h(px(16.))
}

/// A dot-matrix chart. `points` are `(position, value)` pairs, both 0.0 to
/// 1.0: position across the time range (1.0 is now), value up the scale.
/// Each column shows the highest value that falls into it; columns with no
/// samples draw only a dim baseline dot, so a short history stays at the
/// right-hand end instead of being stretched across the range. The column
/// holding the newest sample is drawn brighter.
pub fn dot_chart(points: Vec<(f32, f32)>, height: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _| {
            let step = 6.0;
            let radius = 1.8;
            let rows = ((height - 4.0) / step).floor().max(1.0) as usize;
            let cols = (f32::from(bounds.size.width) / step).floor().max(1.0) as usize;
            let mut columns: Vec<Option<f32>> = vec![None; cols];
            let mut newest_col = None;
            let mut newest_pos = f32::MIN;
            for &(pos, value) in &points {
                let c = ((pos.clamp(0.0, 1.0) * cols as f32) as usize).min(cols - 1);
                columns[c] = Some(columns[c].map_or(value, |v: f32| v.max(value)));
                if pos > newest_pos {
                    newest_pos = pos;
                    newest_col = Some(c);
                }
            }
            for (c, column) in columns.iter().enumerate() {
                let (lit, empty) = match column {
                    Some(v) => (((v * rows as f32).round() as usize).clamp(1, rows), false),
                    None => (1, true),
                };
                for r in 0..lit {
                    let colour = if empty {
                        theme::dot_off()
                    } else if newest_col == Some(c) {
                        theme::ink().into()
                    } else {
                        theme::dot_on().opacity(0.35 + 0.4 * r as f32 / rows as f32)
                    };
                    let centre = bounds.origin
                        + point(
                            px(radius + c as f32 * step),
                            px(height - 4.0 - r as f32 * step),
                        );
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                centre - point(px(radius), px(radius)),
                                size(px(radius * 2.), px(radius * 2.)),
                            ),
                            colour,
                        )
                        .corner_radii(px(radius)),
                    );
                }
            }
        },
    )
    .w_full()
    .h(px(height))
}
