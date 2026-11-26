//! The full window: the machine strip on top, and the selected machine's
//! health, services and sync controls below it.

use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div, prelude::*, px, rgb,
};
use menagerie_fleet::Machine;

use crate::assets::icon;
use crate::store::{Status, Store};
use crate::theme;
use crate::ui::widgets::{
    device_glyph, dot_chart, icon_button, meter, service_icon, status_dot, text_button,
};

pub struct FleetWindow {
    store: Entity<Store>,
    _observe: Subscription,
}

impl FleetWindow {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            store,
            _observe: observe,
        }
    }

    fn titlebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let reload = self.store.clone();
        let edit = self.store.clone();
        div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(6.))
            .px(px(14.))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme::mute())
                    .mr(px(4.))
                    .child("No live data yet"),
            )
            .child(icon_button(
                "refresh",
                icon::REFRESH,
                26.,
                move |_, _, cx| {
                    reload.update(cx, |store, cx| {
                        store.reload();
                        cx.notify();
                    });
                },
            ))
            .child(icon_button(
                "settings",
                icon::SETTINGS,
                26.,
                move |_, _, cx| {
                    crate::edit_fleet_file(&edit, cx);
                },
            ))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|_, _, window, _| {
                    window.start_window_move();
                }),
            )
    }

    fn strip(&self, store: &Store, cx: &Context<Self>) -> impl IntoElement {
        let selected = store.selected.clone();
        let tiles = store.machines().into_iter().map(|(name, machine)| {
            let status = store.status(name);
            let active = selected.as_deref() == Some(name);
            let target: SharedString = name.to_owned().into();
            let tile = div()
                .id(SharedString::from(format!("tile-{name}")))
                .flex_1()
                .min_w(px(0.))
                .max_w(px(220.))
                .flex()
                .flex_col()
                .items_center()
                .pt(px(12.))
                .pb(px(14.))
                .px(px(6.))
                .rounded(px(12.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.store.update(cx, |store, cx| {
                        store.select(&target);
                        cx.notify();
                    });
                }))
                .child(device_glyph(machine.device, 1.0, active))
                .child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .max_w_full()
                        .child(status_dot(status))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if active { theme::ink() } else { theme::mute() })
                                .truncate()
                                .child(name.to_owned()),
                        ),
                )
                .child(
                    div()
                        .mt(px(3.))
                        .font_family(theme::MONO)
                        .text_size(px(11.))
                        .text_color(match status {
                            Status::Drift { .. } => theme::amber(),
                            _ => theme::mute(),
                        })
                        .child(status.label()),
                );
            if active {
                tile.bg(theme::panel())
            } else {
                tile.hover(|s| s.bg(rgb(0x1c1c1f)))
            }
        });
        let edit = self.store.clone();
        div()
            .flex_none()
            .flex()
            .items_end()
            .gap(px(10.))
            .px(px(18.))
            .children(tiles)
            .child(
                div().pb(px(30.)).child(
                    icon_button("add-machine", icon::PLUS, 28., move |_, _, cx| {
                        crate::edit_fleet_file(&edit, cx);
                    })
                    .rounded_full(),
                ),
            )
    }

    fn panel(&self, name: &str, machine: &Machine, status: Status) -> impl IntoElement {
        let mut body = div()
            .flex()
            .flex_col()
            .px(px(20.))
            .pt(px(18.))
            .pb(px(20.))
            .child(header(name, machine, status));

        body = body.child(section().child(notice(machine, status)));
        if machine.agent.is_some() {
            body = body.child(section().child(health(status)));
        }
        if !machine.services.is_empty() {
            body = body.child(section().child(services(machine)));
        }
        body
    }

    fn empty_state(&self, store: &Store, cx: &Context<Self>) -> AnyElement {
        let path = store.path.display().to_string();
        let (title, detail): (&str, String) = if !store.file_exists() {
            (
                "No fleet file yet",
                format!("Menagerie reads its machines from {path}."),
            )
        } else if let Some(err) = &store.error {
            ("The fleet file has a problem", err.clone())
        } else {
            ("No machines", format!("{path} lists no machines yet."))
        };
        let create = !store.file_exists();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .px(px(40.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .max_w(px(520.))
                    .text_size(px(12.5))
                    .text_color(theme::mute())
                    .text_center()
                    .child(detail),
            )
            .child(div().mt(px(6.)).flex().gap(px(8.)).child(if create {
                text_button("create", "Create from example", true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.store.update(cx, |store, cx| {
                            if let Err(err) = store.create_from_example() {
                                store.error = Some(err.to_string());
                            }
                            cx.notify();
                        });
                    }))
                    .into_any_element()
            } else {
                text_button("open-file", "Open fleet file", true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        crate::edit_fleet_file(&this.store, cx);
                    }))
                    .into_any_element()
            }))
            .into_any_element()
    }
}

impl Render for FleetWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let selected = store
            .selected
            .clone()
            .and_then(|name| store.machine(&name).cloned().map(|m| (name, m)));
        let status = selected.as_ref().map(|(name, _)| store.status(name));
        let has_machines = !store.machines().is_empty();

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::ink())
            .font_family(theme::SANS)
            .child(self.titlebar(cx));

        if has_machines {
            let strip = self.strip(self.store.read(cx), cx);
            root = root.child(strip);
            if let (Some((name, machine)), Some(status)) = (selected, status) {
                root = root.child(
                    div()
                        .id("panel")
                        .flex_1()
                        .overflow_y_scroll()
                        .border_t_1()
                        .border_color(theme::line())
                        .bg(theme::panel())
                        .child(self.panel(&name, &machine, status)),
                );
            }
        } else {
            let empty = self.empty_state(self.store.read(cx), cx);
            root = root.child(empty);
        }
        root
    }
}

fn section() -> gpui::Div {
    div()
        .mt(px(18.))
        .pt(px(16.))
        .border_t_1()
        .border_color(theme::line())
}

fn header(name: &str, machine: &Machine, status: Status) -> impl IntoElement {
    let details = match (&machine.sync, &machine.agent) {
        (Some(sync), _) => format!("{} sync", sync.preset.label()),
        (None, Some(_)) => "No sync configured".to_owned(),
        (None, None) => "Not managed by Menagerie".to_owned(),
    };
    let can_sync = machine.sync.is_some() && status.is_online();
    let can_shell = machine.ssh.is_some() || machine.sync.is_some();
    div()
        .flex()
        .justify_between()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::BOLD)
                        .truncate()
                        .child(name.to_owned()),
                )
                .when_some(machine.role.clone(), |el, role| {
                    el.child(
                        div()
                            .mt(px(3.))
                            .text_size(px(12.5))
                            .text_color(theme::mute())
                            .child(role),
                    )
                })
                .child(
                    div()
                        .mt(px(2.))
                        .text_size(px(11.5))
                        .text_color(theme::mute())
                        .child(details),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_end()
                .flex_none()
                .when(status.needs_sync(), |el| {
                    el.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::amber())
                            .child(status.label()),
                    )
                })
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_size(px(11.))
                        .text_color(theme::mute())
                        .mt(px(2.))
                        .child(if status.is_online() {
                            "Last sync unknown"
                        } else {
                            ""
                        }),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .gap(px(6.))
                        .child(text_button("changes", "Changes", status.is_online()))
                        .child(text_button("terminal", "Terminal", can_shell))
                        .child(text_button(
                            "sync",
                            if status.needs_sync() {
                                format!("Sync {}", status.label())
                            } else {
                                "Sync".to_owned()
                            },
                            can_sync,
                        ))
                        .child(text_button("more", "⋯", status.is_online())),
                ),
        )
}

/// One line explaining why there is no data, until the agent exists.
fn notice(machine: &Machine, status: Status) -> impl IntoElement {
    let text = match (&machine.agent, status) {
        (None, _) => "No agent configured. Add `agent:` to this machine in the fleet file to see its health and sync state.".to_owned(),
        (Some(url), Status::Offline) => format!("No answer from the agent at {url} yet."),
        (Some(_), _) => String::new(),
    };
    div()
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .bg(theme::raise())
        .border_1()
        .border_color(theme::line())
        .text_size(px(12.))
        .text_color(theme::mute())
        .child(text)
}

fn health(status: Status) -> impl IntoElement {
    let live = status.is_online();
    let value = |text: &'static str| {
        div()
            .font_family(theme::MONO)
            .text_size(px(13.))
            .text_color(if live { theme::ink() } else { theme::mute() })
            .child(text)
    };
    let meter_block = |label: &'static str| {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(13.))
                    .child(label)
                    .child(value("--")),
            )
            .child(meter(None, theme::dot_on()))
    };
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::BOLD)
                                .child("Load"),
                        )
                        .child(value("--")),
                )
                .child(
                    div()
                        .flex()
                        .bg(theme::raise())
                        .rounded(px(8.))
                        .p(px(2.))
                        .text_size(px(12.))
                        .children(["1h", "24h", "7d"].into_iter().map(|range| {
                            let on = range == "24h";
                            div()
                                .px(px(10.))
                                .py(px(3.))
                                .rounded(px(6.))
                                .when(on, |el| {
                                    el.bg(rgb(0x3a3a40)).font_weight(FontWeight::SEMIBOLD)
                                })
                                .text_color(if on { theme::ink() } else { theme::mute() })
                                .child(range)
                        })),
                ),
        )
        .child(div().mt(px(14.)).child(dot_chart(Vec::new(), 44.)))
        .child(
            div()
                .mt(px(6.))
                .flex()
                .justify_between()
                .font_family(theme::MONO)
                .text_size(px(10.5))
                .text_color(theme::mute())
                .children(["00:00", "06:00", "12:00", "18:00", "now"]),
        )
        .child(
            div()
                .mt(px(16.))
                .grid()
                .grid_cols(2)
                .gap_x(px(22.))
                .gap_y(px(16.))
                .child(meter_block("Memory"))
                .child(meter_block("Disk"))
                .child(meter_block("Battery"))
                .child(meter_block("Temperature")),
        )
}

fn services(machine: &Machine) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::BOLD)
                .mb(px(10.))
                .child("Services"),
        )
        .child(div().grid().grid_cols(2).gap(px(10.)).children(
            machine.services.iter().enumerate().map(|(i, service)| {
                let url = service.url.clone();
                div()
                    .id(("service", i))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded(px(10.))
                    .bg(theme::raise())
                    .border_1()
                    .border_color(theme::line())
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x2b2b30)))
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .child(service_icon(service, 32.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{} ↗", service.name)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .font_family(theme::MONO)
                                    .text_size(px(11.))
                                    .text_color(theme::mute())
                                    .child(status_dot(Status::NoAgent))
                                    .child(if service.health.is_some() {
                                        "not checked yet"
                                    } else {
                                        "no health check"
                                    }),
                            ),
                    )
            }),
        ))
}
