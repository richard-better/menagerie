//! The full window: the machine strip on top, and the selected machine's
//! health, services and sync controls below it.

use std::time::Duration;

use gpui::{
    AnyElement, Context, Entity, FontWeight, Hsla, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Task, Window, div, prelude::*, px, rgb,
};
use menagerie_fleet::Machine;
use menagerie_protocol as protocol;

use crate::assets::icon;
use crate::store::{Health, Status, Store};
use crate::theme;
use crate::ui::commit::{CommitPage, Committed};
use crate::ui::widgets::{
    device_glyph, dot_chart, icon_button, meter, page_bar, service_icon, status_dot, text_button,
};

/// How much load history the chart shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    Hour,
    Day,
}

impl Range {
    fn millis(self) -> i64 {
        match self {
            Range::Hour => 60 * 60 * 1000,
            Range::Day => 24 * 60 * 60 * 1000,
        }
    }

    fn ticks(self) -> [&'static str; 5] {
        match self {
            Range::Hour => ["-60m", "-45m", "-30m", "-15m", "now"],
            Range::Day => ["-24h", "-18h", "-12h", "-6h", "now"],
        }
    }
}

pub struct FleetWindow {
    store: Entity<Store>,
    range: Range,
    show_changes: bool,
    show_log: bool,
    show_more: bool,
    /// Open shells by machine, with where each one runs. Dropping one ends
    /// it.
    terminals: std::collections::HashMap<
        String,
        (Entity<crate::terminal::TerminalView>, crate::sync::Place),
    >,
    /// Machines whose panel shows their shell instead of their details.
    in_terminal: std::collections::HashSet<String>,
    /// Committing the checkout's changes, shown in place of the panel.
    commit_page: Option<(Entity<CommitPage>, Subscription)>,
    _observe: Subscription,
    /// Re-renders every few seconds so "Updated 8s ago" stays true.
    _ticker: Task<()>,
}

impl FleetWindow {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&store, |this: &mut Self, store, cx| {
            // A shell whose machine is gone, or now runs elsewhere, ends.
            let store = store.read(cx);
            this.terminals.retain(|name, (_, place)| {
                store.machine(name).is_some_and(|m| {
                    crate::sync::place(name, m, &store.hostname).as_ref() == Ok(place)
                })
            });
            let terminals = &this.terminals;
            this.in_terminal.retain(|name| terminals.contains_key(name));
            cx.notify()
        });
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        });
        Self {
            store,
            range: Range::Day,
            show_changes: false,
            show_log: false,
            show_more: false,
            terminals: std::collections::HashMap::new(),
            in_terminal: std::collections::HashSet::new(),
            commit_page: None,
            _observe: observe,
            _ticker: ticker,
        }
    }

    fn titlebar(&self, store: &Store, cx: &Context<Self>) -> impl IntoElement {
        let refresh = self.store.clone();
        let edit = self.store.clone();
        let dirty = matches!(&store.checkout, Some(Ok(co)) if co.dirty);
        let checkout = match &store.checkout {
            Some(Ok(co)) => {
                let mut parts = Vec::new();
                if co.unpushed > 0 {
                    parts.push(format!("{} unpushed", co.unpushed));
                }
                if co.unpulled > 0 {
                    parts.push(format!("{} to pull", co.unpulled));
                }
                if co.dirty {
                    parts.push("uncommitted changes".to_owned());
                }
                (!parts.is_empty()).then(|| format!("Checkout: {}", parts.join(" · ")))
            }
            Some(Err(err)) => Some(format!("Checkout: {err}")),
            None => None,
        };
        div()
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(6.))
            .px(px(14.))
            .when_some(checkout, |el, text| {
                el.child(
                    div()
                        .id("checkout")
                        .text_size(px(11.5))
                        .text_color(theme::mute())
                        .mr(px(10.))
                        .truncate()
                        .child(text)
                        // Uncommitted changes: a click opens the commit page.
                        .when(dirty, |el| {
                            el.cursor_pointer()
                                .hover(|s| s.text_color(theme::ink()))
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_commit_page(window, cx)
                                }))
                        }),
                )
            })
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme::mute())
                    .mr(px(4.))
                    .child(
                        store
                            .updated_label()
                            .unwrap_or_else(|| "No live data yet".to_owned()),
                    ),
            )
            .child(icon_button(
                "refresh",
                icon::REFRESH,
                26.,
                move |_, _, cx| {
                    refresh.update(cx, |store, cx| {
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
                        .text_color(if status.needs_sync() {
                            theme::amber()
                        } else {
                            theme::mute()
                        })
                        .child(store.label(name)),
                );
            if active {
                tile.relative().child(notch())
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

    fn panel(
        &self,
        store: &Store,
        name: &str,
        machine: &Machine,
        cx: &Context<Self>,
    ) -> AnyElement {
        let status = store.status(name);
        let report = store.report(name);
        let mut body = div()
            .flex()
            .flex_col()
            .px(px(20.))
            .pt(px(18.))
            .pb(px(20.))
            .child(header(
                store,
                name,
                machine,
                status,
                self.actions(store, name, machine, status, cx),
            ));

        if self.show_more {
            body = body.child(self.more_menu(store, name, machine, cx));
        }
        if let Some(text) = notice(store, name, machine, status) {
            body = body.child(section().child(notice_box(text)));
        }
        // Open while it runs, and after a failure until the next sync.
        if let Some(job) = store.jobs.get(name)
            && (self.show_log || job.running() || matches!(job.result, Some(Err(_))))
        {
            body = body.child(section().child(job_log(job)));
        }
        if self.show_changes
            && let Some(commits) = store.missing(name)
        {
            let behind = match status {
                Status::Drift { behind, .. } => behind as usize,
                _ => commits.len(),
            };
            body = body.child(section().child(changes(commits, behind)));
        }
        if machine.agent.is_some() {
            body = body.child(section().child(self.health(report, cx)));
        }
        if !machine.services.is_empty() {
            body = body.child(section().child(services(store, name, machine)));
        }
        body.into_any_element()
    }

    /// The bar above a machine's shell: back to its details, where the
    /// shell runs, and ending it.
    fn terminal_bar(
        &self,
        name: &str,
        place: &crate::sync::Place,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let back = cx.entity().downgrade();
        let target = name.to_owned();
        let end = name.to_owned();
        let place = match place {
            crate::sync::Place::Here => "this Mac".to_owned(),
            crate::sync::Place::Ssh(to) => format!("ssh {to}"),
        };
        page_bar(name.to_owned(), place, move |_, _, cx| {
            let _ = back.update(cx, |this, cx| this.leave_terminal(&target, cx));
        })
        .child(
            text_button("end-session", "End session", true).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.in_terminal.remove(&end);
                    this.terminals.remove(&end);
                    cx.notify();
                },
            )),
        )
    }

    /// Show the page that commits the checkout's changes.
    fn open_commit_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let Some(fleet) = &store.fleet else {
            return;
        };
        let Some(repo) = fleet.repo_path() else {
            return;
        };
        let branch = fleet.branch().to_owned();
        let command = fleet.commit_message.clone();
        let page = cx.new(|cx| CommitPage::new(repo, branch, command, window, cx));
        let done = cx.subscribe(&page, |this: &mut Self, _, _: &Committed, cx| {
            this.commit_page = None;
            this.store.read(cx).refresh();
            cx.notify();
        });
        self.commit_page = Some((page, done));
        cx.notify();
    }

    /// Back to the details. The shell keeps running unless it has ended.
    fn leave_terminal(&mut self, name: &str, cx: &mut Context<Self>) {
        self.in_terminal.remove(name);
        if self
            .terminals
            .get(name)
            .is_some_and(|(view, _)| view.read(cx).ended())
        {
            self.terminals.remove(name);
        }
        cx.notify();
    }

    /// Changes, Log, Sync and the ⋯ menu.
    fn actions(
        &self,
        store: &Store,
        name: &str,
        machine: &Machine,
        status: Status,
        cx: &Context<Self>,
    ) -> AnyElement {
        let running = store.running(name);
        let has_changes = store.missing(name).is_some_and(|c| !c.is_empty());
        // Needs a way in, not a live agent: SSH still works when the agent
        // is down, which is exactly when a sync or rollback may be needed.
        let reachable = crate::sync::place(name, machine, &store.hostname).is_ok();
        let can_sync = machine.sync.is_some() && reachable && !running;
        let sync_label = if running {
            store.label(name)
        } else if matches!(status, Status::Drift { .. }) {
            format!("Sync {}", status.label())
        } else {
            "Sync".to_owned()
        };
        let target = name.to_owned();
        div()
            .flex()
            .gap(px(6.))
            .child(
                text_button("changes", "Changes", has_changes).when(has_changes, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.show_changes = !this.show_changes;
                        cx.notify();
                    }))
                }),
            )
            .child({
                let running = self.terminals.contains_key(name);
                let target = name.to_owned();
                // A running shell can always be gone back to.
                let enabled = running || reachable;
                text_button("terminal", "Terminal", enabled).when(enabled, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        this.open_terminal(&target, window, cx)
                    }))
                })
            })
            .child(
                text_button("log", "Log", store.jobs.contains_key(name)).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.show_log = !this.show_log;
                        cx.notify();
                    },
                )),
            )
            .child(
                text_button("sync", sync_label, can_sync)
                    .when(can_sync, |b| {
                        b.bg(rgb(0x2563eb))
                            .text_color(rgb(0xffffff))
                            .hover(|s| s.bg(rgb(0x3b74f0)))
                    })
                    .when(can_sync, |b| {
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            this.show_log = true;
                            let _ = crate::start_job(
                                this.store.clone(),
                                target.clone(),
                                crate::sync::Action::Sync,
                                cx,
                            );
                        }))
                    }),
            )
            .child(
                text_button("more", "⋯", machine.sync.is_some()).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.show_more = !this.show_more;
                        cx.notify();
                    },
                )),
            )
            .into_any_element()
    }

    /// Show `name`'s shell, starting one (here, or over SSH) if none runs.
    fn open_terminal(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((view, _)) = self.terminals.get(name) {
            self.in_terminal.insert(name.to_owned());
            window.focus(&gpui::Focusable::focus_handle(view.read(cx), cx));
            cx.notify();
            return;
        }
        let store = self.store.read(cx);
        let Some(machine) = store.machine(name) else {
            return;
        };
        let Ok(place) = crate::sync::place(name, machine, &store.hostname) else {
            return;
        };
        let dir = machine
            .sync
            .as_ref()
            .and_then(|s| s.repo.as_deref())
            .map(menagerie_fleet::expand_home);
        let view = cx.new(|cx| crate::terminal::TerminalView::new(&place, dir, cx));
        self.terminals
            .insert(name.to_owned(), (view.clone(), place));
        self.in_terminal.insert(name.to_owned());
        window.focus(&gpui::Focusable::focus_handle(view.read(cx), cx));
        cx.notify();
    }

    /// What lives behind ⋯: rolling back.
    fn more_menu(
        &self,
        store: &Store,
        name: &str,
        machine: &Machine,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let custom = machine
            .sync
            .as_ref()
            .is_some_and(|s| s.preset == menagerie_fleet::SyncPreset::Custom);
        let reachable = crate::sync::place(name, machine, &store.hostname).is_ok();
        let can_roll_back = machine.sync.is_some() && !custom && reachable && !store.running(name);
        let target = name.to_owned();
        div()
            .mt(px(10.))
            .flex()
            .justify_end()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme::mute())
                    .child(if custom {
                        "A custom sync command can't be rolled back from here."
                    } else {
                        "Switch back to the generation before the current one."
                    }),
            )
            .child(
                text_button("rollback", "Roll back", can_roll_back).when(can_roll_back, |b| {
                    b.on_click(cx.listener(move |this, _, _, cx| {
                        this.show_more = false;
                        this.show_log = true;
                        let _ = crate::start_job(
                            this.store.clone(),
                            target.clone(),
                            crate::sync::Action::Rollback,
                            cx,
                        );
                    }))
                }),
            )
    }

    fn health(&self, report: Option<&protocol::Status>, cx: &Context<Self>) -> impl IntoElement {
        let probes = report.map(|r| &r.probes);
        let load = probes.and_then(|p| p.load.value.as_ref());
        let load_text = load
            .map(|l| format!("{:.2}", l.one))
            .unwrap_or_else(|| "--".to_owned());
        let cpus = load.map(|l| format!("/ {} cpus", l.cpus));

        let (points, peak) = report
            .map(|r| chart_points(r, self.range))
            .unwrap_or_default();

        let memory = probes.and_then(|p| p.memory.value);
        let disk = probes.and_then(|p| p.disk.value);
        let battery = probes.and_then(|p| p.battery.value);
        let temperature = probes.and_then(|p| p.temperature.value);

        let range_button = |range: Range, label: &'static str| {
            let on = self.range == range;
            div()
                .id(label)
                .px(px(10.))
                .py(px(3.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |el| {
                    el.bg(rgb(0x3a3a40)).font_weight(FontWeight::SEMIBOLD)
                })
                .text_color(if on { theme::ink() } else { theme::mute() })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.range = range;
                    cx.notify();
                }))
                .child(label)
        };

        let mut grid = div()
            .mt(px(16.))
            .grid()
            .grid_cols(2)
            .gap_x(px(22.))
            .gap_y(px(16.))
            .child(meter_block(
                "Memory",
                memory.map(|m| format!("{} / {} GB", gib(m.used_bytes), gib(m.total_bytes))),
                memory.map(|m| m.fraction() as f32),
                memory.map(|m| threshold(m.fraction() * 100.0, 85.0, 95.0)),
                None,
            ))
            .child(meter_block(
                "Disk",
                disk.map(|d| format!("{} / {} GB", gb(d.used_bytes), gb(d.total_bytes))),
                disk.map(|d| d.fraction() as f32),
                disk.map(|d| threshold(d.fraction() * 100.0, 85.0, 95.0)),
                None,
            ));
        // A machine without a battery gets no battery meter at all.
        let has_battery = report.is_none() || battery.is_some();
        if has_battery {
            grid = grid.child(meter_block(
                "Battery",
                battery.map(|b| format!("{:.0}%", b.percent)),
                battery.map(|b| (b.percent / 100.0) as f32),
                battery.map(|b| {
                    if b.charging {
                        theme::green().into()
                    } else {
                        threshold(100.0 - b.percent, 80.0, 90.0)
                    }
                }),
                battery.filter(|b| b.charging).map(|_| "charging"),
            ));
        }
        grid = grid.child(meter_block(
            "Temperature",
            temperature.map(|t| format!("{:.0}°C", t.celsius)),
            temperature.map(|t| (t.celsius / 100.0) as f32),
            temperature.map(|t| threshold(t.celsius, 80.0, 90.0)),
            None,
        ));

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
                            .child(mono_value(load_text, load.is_some()))
                            .when_some(cpus, |el, cpus| {
                                el.child(
                                    div()
                                        .font_family(theme::MONO)
                                        .text_size(px(11.5))
                                        .text_color(theme::mute())
                                        .child(cpus),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .bg(theme::raise())
                            .rounded(px(8.))
                            .p(px(2.))
                            .text_size(px(12.))
                            .child(range_button(Range::Hour, "1h"))
                            .child(range_button(Range::Day, "24h")),
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .h(px(14.))
                    .font_family(theme::MONO)
                    .text_size(px(10.5))
                    .text_color(theme::mute())
                    .when(peak > 0.0, |el| el.child(format!("peak {peak:.2}"))),
            )
            .child(dot_chart(points, 44.))
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .justify_between()
                    .font_family(theme::MONO)
                    .text_size(px(10.5))
                    .text_color(theme::mute())
                    .children(self.range.ticks()),
            )
            .child(grid)
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
        let has_machines = !store.machines().is_empty();

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::ink())
            .font_family(theme::SANS)
            .child(self.titlebar(store, cx));

        if has_machines {
            root = root.child(self.strip(store, cx));
            if let Some((page, _)) = &self.commit_page {
                let back = cx.entity().downgrade();
                let repo = store
                    .fleet
                    .as_ref()
                    .and_then(|f| f.repo.clone())
                    .unwrap_or_default();
                root = root.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .border_t_1()
                        .border_color(theme::line())
                        .bg(theme::panel())
                        .child(page_bar("Commit changes", repo, move |_, _, cx| {
                            let _ = back.update(cx, |this, cx| {
                                this.commit_page = None;
                                cx.notify();
                            });
                        }))
                        .child(
                            div()
                                .id("commit-page")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(page.clone()),
                        ),
                );
            } else if let Some((name, machine)) = selected {
                let panel = div()
                    .id("panel")
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(theme::line())
                    .bg(theme::panel());
                let shell = self
                    .terminals
                    .get(&name)
                    .filter(|_| self.in_terminal.contains(&name));
                root = root.child(match shell {
                    // The shell takes the whole panel, under a bar that
                    // leads back to the details.
                    Some((terminal, place)) => panel
                        .flex()
                        .flex_col()
                        .child(self.terminal_bar(&name, place, cx))
                        .child(div().flex_1().min_h_0().child(terminal.clone())),
                    None => panel
                        .overflow_y_scroll()
                        .child(self.panel(store, &name, &machine, cx)),
                });
            }
        } else {
            root = root.child(self.empty_state(store, cx));
        }
        root
    }
}

/// A notch in the panel's top edge, pointing up at the selected machine,
/// so the details read as that machine's. A triangle in the line colour
/// with one in the panel colour just below it: together an outlined notch
/// that also covers the panel's border beneath it. Deferred, so it paints
/// over the panel, which comes later.
fn notch() -> impl IntoElement {
    const W: f32 = 22.;
    const H: f32 = 11.;
    // How far the inner triangle sits below the outer: the outline's width
    // along the slopes.
    const INSET: f32 = 1.5;
    let triangle = |top: f32, colour: gpui::Rgba| {
        gpui::svg()
            .path(icon::NOTCH)
            .absolute()
            .top(px(top))
            .left_0()
            .w(px(W))
            .h(px(H))
            .text_color(colour)
    };
    gpui::deferred(
        div()
            .absolute()
            // The outer triangle's base on the border line's lower edge.
            .bottom(px(-1. - INSET))
            .left(gpui::relative(0.5))
            .ml(px(-W / 2.))
            .w(px(W))
            .h(px(H + INSET))
            .child(triangle(0., theme::line()))
            .child(triangle(INSET, theme::panel())),
    )
}

fn section() -> gpui::Div {
    div()
        .mt(px(18.))
        .pt(px(16.))
        .border_t_1()
        .border_color(theme::line())
}

fn header(
    store: &Store,
    name: &str,
    machine: &Machine,
    status: Status,
    actions: AnyElement,
) -> impl IntoElement {
    let details = match (&machine.sync, &machine.agent) {
        (Some(sync), _) => format!("{} sync", sync.preset.label()),
        (None, Some(_)) => "No sync configured".to_owned(),
        (None, None) => "Not managed by Menagerie".to_owned(),
    };
    let report = store.report(name);
    let uptime = report
        .and_then(|r| r.probes.system.value.as_ref())
        .map(|s| format!(" · up {}", duration(s.uptime_s)));
    let revision = store.revision(name).map(|rev| {
        let mut line = rev.short().to_owned();
        if let Some(generation) = rev.generation {
            line = format!("generation {generation} · {line}");
        }
        if rev.dirty {
            line.push_str(" · uncommitted changes");
        }
        line
    });
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
                        .child(format!("{details}{}", uptime.unwrap_or_default())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_end()
                .flex_none()
                .when(matches!(status, Status::Drift { .. }), |el| {
                    el.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::amber())
                            .child(status.label()),
                    )
                })
                .when_some(revision, |el, line| {
                    el.child(
                        div()
                            .font_family(theme::MONO)
                            .text_size(px(11.))
                            .text_color(theme::mute())
                            .mt(px(2.))
                            .child(line),
                    )
                })
                .child(div().mt(px(8.)).child(actions)),
        )
}

/// One line explaining what is missing, when something is.
fn notice(store: &Store, name: &str, machine: &Machine, status: Status) -> Option<String> {
    let Some(url) = &machine.agent else {
        return Some(
            "No agent configured. Add `agent:` to this machine in the fleet file to see its health and sync state."
                .to_owned(),
        );
    };
    if status == Status::Offline {
        let why = store
            .live
            .get(name)
            .and_then(|l| l.error.clone())
            .map(|e| format!(": {e}"))
            .unwrap_or_default();
        return Some(format!("No answer from the agent at {url}{why}"));
    }
    // Nothing to compare for a machine Menagerie doesn't sync, such as one
    // that isn't built from the repo at all.
    machine.sync.as_ref()?;
    let Some(revision) = store.revision(name) else {
        return Some(
            "This machine doesn't report which commit built it. Rebuild it from a configuration that sets `system.configurationRevision`."
                .to_owned(),
        );
    };
    let fleet = store.fleet.as_ref()?;
    if fleet.repo.is_none() {
        return Some(
            "Add `repo:` to the fleet file to compare this machine with your checkout.".to_owned(),
        );
    }
    match store.drift.get(&revision.rev) {
        Some(Err(err)) => Some(format!("Can't compare with your checkout: {err}.")),
        _ => None,
    }
}

fn notice_box(text: String) -> impl IntoElement {
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

fn mono_value(text: String, live: bool) -> impl IntoElement {
    div()
        .font_family(theme::MONO)
        .text_size(px(13.))
        .text_color(if live { theme::ink() } else { theme::mute() })
        .child(text)
}

fn meter_block(
    label: &'static str,
    value: Option<String>,
    fraction: Option<f32>,
    colour: Option<Hsla>,
    note: Option<&'static str>,
) -> impl IntoElement {
    let live = value.is_some();
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .flex()
                .justify_between()
                .items_baseline()
                .text_size(px(13.))
                .child(label)
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .items_baseline()
                        .child(mono_value(value.unwrap_or_else(|| "--".to_owned()), live))
                        .when_some(note, |el, note| {
                            el.child(
                                div()
                                    .font_family(theme::MONO)
                                    .text_size(px(12.))
                                    .text_color(theme::green())
                                    .child(note),
                            )
                        }),
                ),
        )
        .child(meter(fraction, colour.unwrap_or_else(theme::dot_on)))
}

/// Neutral until `warn`, amber until `critical`, red beyond.
fn threshold(value: f64, warn: f64, critical: f64) -> Hsla {
    if value >= critical {
        rgb(0xe5534b).into()
    } else if value >= warn {
        theme::amber().into()
    } else {
        theme::dot_on()
    }
}

/// The load history as chart points for `range`, and its peak.
fn chart_points(report: &protocol::Status, range: Range) -> (Vec<(f32, f32)>, f64) {
    let now = report.agent.now;
    // The timestamps come from another machine: keep the arithmetic total.
    let start = now.saturating_sub(range.millis());
    let samples: Vec<_> = report
        .load_history
        .iter()
        .filter(|s| (start..=now).contains(&s.at))
        .collect();
    let peak = samples.iter().map(|s| s.value).fold(0.0_f64, f64::max);
    // Scale to the peak, but never so tightly that an idle machine's noise
    // fills the chart.
    let scale = (peak * 1.15).max(1.0);
    let points = samples
        .iter()
        .map(|s| {
            (
                ((s.at - start) as f64 / range.millis() as f64) as f32,
                (s.value / scale) as f32,
            )
        })
        .collect();
    (points, peak)
}

fn services(store: &Store, name: &str, machine: &Machine) -> impl IntoElement {
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
                let health = store.health.get(&(name.to_owned(), i));
                let (dot, text) = match (health, &service.health) {
                    (Some(Health::Up { millis }), _) => {
                        (Status::Online, format!("up · {millis} ms"))
                    }
                    (Some(Health::Down(_)), _) => (Status::Offline, "down".to_owned()),
                    (None, Some(_)) => (Status::NoAgent, "checking".to_owned()),
                    (None, None) => (Status::NoAgent, "no health check".to_owned()),
                };
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
                                    .child(status_dot(dot))
                                    .child(text),
                            ),
                    )
            }),
        ))
}

/// Memory in GiB, as macOS labels it "GB".
fn gib(bytes: u64) -> String {
    let v = bytes as f64 / (1u64 << 30) as f64;
    if v >= 10.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Storage in decimal GB, as Finder shows it.
fn gb(bytes: u64) -> String {
    format!("{:.0}", bytes as f64 / 1e9)
}

fn duration(secs: u64) -> String {
    match secs {
        0..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// The latest sync or rollback: how it went, and its output.
fn job_log(job: &crate::store::Job) -> impl IntoElement {
    let elapsed = job
        .finished
        .unwrap_or_else(std::time::Instant::now)
        .duration_since(job.started)
        .as_secs();
    let red = rgb(0xe5534b);
    let action = job.action.label();
    let (state, colour) = match &job.result {
        None => (format!("{action} running · {elapsed}s"), theme::mute()),
        Some(Ok(())) => (format!("{action} finished · {elapsed}s"), theme::green()),
        Some(Err(why)) if why == "Cancelled" => (format!("{action} cancelled"), theme::mute()),
        Some(Err(_)) => (format!("{action} failed · {elapsed}s"), red),
    };
    let running = job.running();
    let cancel = job.cancel.clone();
    let mut tail: Vec<String> = job.lines.iter().rev().take(400).rev().cloned().collect();
    // The reason a step failed is the last thing it said, already the last
    // line here: it turns red. A step that failed silently gets the error.
    let mut failed_line = None;
    if let Some(Err(why)) = &job.result
        && why != "Cancelled"
    {
        if tail.last().is_none_or(|l| l.starts_with('▸')) {
            tail.push(why.clone());
        }
        failed_line = Some(tail.len() - 1);
    }
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .mb(px(10.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::BOLD)
                                .child("Log"),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(11.))
                                .text_color(colour)
                                .child(state),
                        ),
                )
                .when(running, |el| {
                    el.child(
                        text_button("cancel", "Cancel", true)
                            .on_click(move |_, _, _| cancel.cancel()),
                    )
                }),
        )
        .child(
            div()
                .id("job-log")
                .max_h(px(320.))
                .overflow_y_scroll()
                .p(px(10.))
                .rounded(px(9.))
                .bg(rgb(0x0f0f11))
                .border_1()
                .border_color(theme::line())
                .font_family(theme::MONO)
                .text_size(px(11.5))
                .text_color(rgb(0xb9b9c0))
                .when(tail.is_empty(), |el| el.child("Starting…"))
                .children(tail.into_iter().enumerate().map(|(i, line)| {
                    let step = line.starts_with('▸');
                    // A link, such as Tailscale SSH asking you to sign in,
                    // opens when clicked.
                    let url = line
                        .split_whitespace()
                        .find(|w| w.starts_with("https://"))
                        .map(str::to_owned);
                    div()
                        .id(("log-line", i))
                        .when(step, |el| el.text_color(theme::ink()).mt(px(4.)))
                        .when(failed_line == Some(i), |el| el.text_color(red))
                        .when_some(url, |el, url| {
                            el.text_color(rgb(0x6fa0ff))
                                .cursor_pointer()
                                .on_click(move |_, _, cx| cx.open_url(&url))
                        })
                        .child(line)
                })),
        )
}

/// The commits the machine is missing, newest first.
fn changes(commits: &[(String, String)], behind: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::BOLD)
                .mb(px(10.))
                .child(format!("Changes · {behind} to pull")),
        )
        .when(commits.len() < behind, |el| {
            el.child(
                div()
                    .mb(px(6.))
                    .text_size(px(11.5))
                    .text_color(theme::mute())
                    .child(format!("Showing the latest {}.", commits.len())),
            )
        })
        .children(commits.iter().map(|(hash, subject)| {
            div()
                .flex()
                .gap(px(12.))
                .py(px(3.))
                .text_size(px(12.5))
                .child(
                    div()
                        .font_family(theme::MONO)
                        .text_color(theme::mute())
                        .w(px(64.))
                        .flex_none()
                        .child(hash.clone()),
                )
                .child(div().min_w(px(0.)).truncate().child(subject.clone()))
        }))
}
