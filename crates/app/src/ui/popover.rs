//! The menu bar popover: a glance at the fleet. One row per machine with
//! its status, its services, and a sync button when it needs one; a thin
//! status bar at the bottom.

use std::rc::Rc;

use gpui::{
    App, Context, Entity, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div, prelude::*, px, rgb,
};
use menagerie_fleet::Machine;

use crate::assets::icon;
use crate::store::{Status, Store};
use crate::theme;
use crate::ui::widgets::{device_glyph, icon_button, service_link, status_dot};

pub const WIDTH: f32 = 340.;
const ROW: f32 = 58.;
const ROW_WITH_SERVICES: f32 = 88.;
const PADDING: f32 = 8.;
const STATUS_BAR: f32 = 40.;

/// The popover's height for the fleet as it stands, so the panel can be
/// sized before it is shown.
pub fn height(store: &Store) -> f32 {
    let rows: f32 = store
        .machines()
        .iter()
        .map(|(_, m)| {
            if m.services.is_empty() {
                ROW
            } else {
                ROW_WITH_SERVICES
            }
        })
        .sum();
    let rows = if rows == 0. { ROW } else { rows };
    PADDING * 2. + rows + STATUS_BAR
}

/// Opens the full window, showing the named machine if one is given.
pub type OpenWindow = Rc<dyn Fn(Option<String>, &mut App)>;

/// What the popover asks of the app around it.
#[derive(Clone)]
pub struct Hooks {
    pub open_window: OpenWindow,
}

pub struct Popover {
    store: Entity<Store>,
    hooks: Hooks,
    subscriptions: Vec<Subscription>,
}

impl Popover {
    pub fn new(store: Entity<Store>, hooks: Hooks, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            store,
            hooks,
            subscriptions: vec![observe],
        }
    }

    /// Keep a subscription alive for as long as the popover is.
    pub fn keep(&mut self, subscription: Subscription) {
        self.subscriptions.push(subscription);
    }

    fn row(&self, name: &str, machine: &Machine, status: Status) -> impl IntoElement {
        let open = self.hooks.open_window.clone();
        let target = name.to_owned();
        let offline = status == Status::Offline;
        div()
            .id(SharedString::from(format!("row-{name}")))
            .h(px(if machine.services.is_empty() {
                ROW
            } else {
                ROW_WITH_SERVICES
            }))
            .flex()
            .gap(px(10.))
            .px(px(8.))
            .py(px(9.))
            .rounded(px(9.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x1e1e21)))
            .on_click(move |_, _, cx| open(Some(target.clone()), cx))
            .child(
                div()
                    .pt(px(2.))
                    .child(device_glyph(machine.device, 0.38, false)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(status_dot(status))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if offline { theme::mute() } else { theme::ink() })
                                    .truncate()
                                    .child(name.to_owned()),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(13.))
                            .mt(px(2.))
                            .text_size(px(11.5))
                            .text_color(theme::mute())
                            .truncate()
                            .child(machine.role.clone().unwrap_or_default()),
                    )
                    .when(!machine.services.is_empty(), |el| {
                        el.child(div().pl(px(13.)).mt(px(6.)).flex().gap(px(5.)).children(
                            machine.services.iter().enumerate().map(|(i, service)| {
                                service_link(
                                    SharedString::from(format!("svc-{name}-{i}")),
                                    service,
                                    22.,
                                )
                            }),
                        ))
                    }),
            )
            .child(div().pt(px(2.)).child(trailing(name, status)))
    }
}

/// What sits at the right end of a row: a quiet sync button with the
/// ahead/behind counts when the machine needs one, otherwise its status.
fn trailing(name: &str, status: Status) -> impl IntoElement {
    if status.needs_sync() {
        div()
            .id(SharedString::from(format!("sync-{name}")))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(3.))
            .rounded(px(7.))
            .bg(theme::raise())
            .border_1()
            .border_color(theme::line())
            .font_family(theme::MONO)
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .child(status.label())
            .child(
                gpui::svg()
                    .path(icon::SYNC)
                    .size(px(11.))
                    .text_color(theme::mute()),
            )
            .into_any_element()
    } else {
        div()
            .text_size(px(11.5))
            .text_color(theme::mute())
            .child(status.label())
            .into_any_element()
    }
}

impl Render for Popover {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let rows: Vec<_> = store
            .machines()
            .into_iter()
            .map(|(name, machine)| {
                self.row(name, machine, store.status(name))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let summary = if store.fleet.is_some() {
            store.summary()
        } else {
            "No fleet file".to_owned()
        };
        let open = self.hooks.open_window.clone();
        let edit = self.store.clone();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .border_1()
            .border_color(theme::line())
            .rounded(px(12.))
            .overflow_hidden()
            .text_color(theme::ink())
            .font_family(theme::SANS)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .p(px(PADDING))
                    .children(rows)
                    .when(empty, |el| {
                        el.child(
                            div()
                                .h(px(ROW))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.))
                                .text_color(theme::mute())
                                .child("Open Menagerie to set up your fleet"),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(STATUS_BAR))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(16.))
                    .pr(px(12.))
                    .border_t_1()
                    .border_color(theme::line())
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme::mute())
                            .child(summary),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(icon_button("open", icon::WINDOW, 20., move |_, _, cx| {
                                open(None, cx)
                            }))
                            .child(icon_button("edit", icon::SETTINGS, 20., move |_, _, cx| {
                                crate::edit_fleet_file(&edit, cx)
                            })),
                    ),
            )
    }
}
