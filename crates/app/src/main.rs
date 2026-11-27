//! The menu bar app.

use std::cell::Cell;
use std::rc::Rc;

use futures::StreamExt;
use gpui::{
    AppContext, Application, Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, point, px, size,
};
use objc2::MainThreadMarker;

use menagerie_app::platform::panel::{self, Panel};
use menagerie_app::platform::{dismiss, tray};
use menagerie_app::store::Store;
use menagerie_app::ui::popover::{self, Hooks, Popover};
use menagerie_app::{assets, fleet_path, open_main_window};

fn main() {
    // sudo runs this executable as its askpass helper during a sync.
    if std::env::var_os(menagerie_app::sync::ASKPASS_ENV).is_some() {
        std::process::exit(menagerie_app::sync::askpass());
    }
    Application::new().with_assets(assets::Assets).run(|cx| {
        let mtm = MainThreadMarker::new().expect("gpui runs this on the main thread");
        // After gpui's own launch handler, which sets the policy to Regular.
        panel::become_accessory(mtm);

        let store = cx.new(|_| Store::load(fleet_path()));
        let (tray, mut presses) = tray::Tray::new().expect("create the status item");
        let tray = Rc::new(tray);

        // The status item's count follows the store.
        let badge = tray.clone();
        badge.set_count(store.read(cx).needs_sync_count());
        cx.observe(&store, move |store, cx| {
            badge.set_count(store.read(cx).needs_sync_count())
        })
        .detach();

        // Quitting ends every open shell at once.
        cx.on_app_quit(|_| {
            menagerie_app::terminal::kill_all();
            async {}
        })
        .detach();

        // Poll the agents, service health and git for as long as the app runs.
        menagerie_app::live::start(store.clone(), cx);

        // The popover's hooks need the panel, which exists only once its
        // window does.
        let panel_cell: Rc<Cell<Option<Panel>>> = Rc::new(Cell::new(None));
        let hooks = {
            let store = store.clone();
            let panel_cell = panel_cell.clone();
            let tray = tray.clone();
            Hooks {
                open_window: Rc::new(move |machine, cx| {
                    if let (Some(panel), Some(mtm)) = (panel_cell.get(), MainThreadMarker::new()) {
                        panel.hide(mtm);
                    }
                    tray.set_active(false);
                    open_main_window(store.clone(), machine, cx);
                }),
            }
        };

        let popover_store = store.clone();
        let window = cx
            .open_window(popover_options(), |_, cx| {
                cx.new(|cx| Popover::new(popover_store, hooks, cx))
            })
            .expect("open the popover window");
        let panel = window
            .update(cx, |_, window, _| Panel::adopt(window))
            .ok()
            .flatten()
            .expect("a gpui window is backed by an NSWindow on macOS");
        panel_cell.set(Some(panel));

        // Hide when the popover stops being the active window.
        let blur_tray = tray.clone();
        let _ = window.update(cx, |popover, window, cx| {
            let subscription = cx.observe_window_activation(window, move |_, window, _| {
                if !window.is_window_active() {
                    if let Some(mtm) = MainThreadMarker::new() {
                        panel.hide(mtm);
                    }
                    blur_tray.set_active(false);
                }
            });
            popover.keep(subscription);
        });

        // Hide on clicks in other apps, including other menu bar items.
        let icon_rect = Rc::new(Cell::new(None));
        let (monitor, mut outside) = dismiss::watch();
        std::mem::forget(monitor);
        let dismiss_tray = tray.clone();
        let dismiss_icon = icon_rect.clone();
        cx.spawn(async move |_| {
            while let Some((x, y)) = outside.next().await {
                let Some(mtm) = MainThreadMarker::new() else {
                    continue;
                };
                if !panel.is_visible(mtm) {
                    continue;
                }
                if dismiss_icon
                    .get()
                    .is_some_and(|r: menagerie_app::platform::Rect| r.contains(x, y))
                {
                    continue;
                }
                panel.hide(mtm);
                dismiss_tray.set_active(false);
            }
        })
        .detach();

        // Toggle the popover from the status item.
        let press_store = store.clone();
        cx.spawn(async move |cx| {
            while let Some(press) = presses.next().await {
                let Some(mtm) = MainThreadMarker::new() else {
                    continue;
                };
                match press {
                    tray::Press::Up => tray.set_active(panel.is_visible(mtm)),
                    tray::Press::Right => {
                        panel.hide(mtm);
                        tray.set_active(false);
                        tray.show_quit_menu();
                    }
                    tray::Press::Down => {
                        let rect = tray.icon_rect();
                        icon_rect.set(rect);
                        if panel.is_visible(mtm) {
                            panel.hide(mtm);
                            tray.set_active(false);
                            continue;
                        }
                        let Ok(height) =
                            cx.read_entity(&press_store, |store, _| popover::height(store))
                        else {
                            continue;
                        };
                        if let Some(rect) = rect {
                            panel.show_under(rect, popover::WIDTH as f64, height as f64, mtm);
                        }
                        // A PopUp is a non-activating panel; activate the app
                        // so it can take focus, then it hides itself on blur.
                        let _ = cx.update(|cx| cx.activate(true));
                        tray.set_active(true);
                    }
                }
            }
        })
        .detach();

        // First run: no fleet file yet, so show the window that explains it.
        if !store.read(cx).file_exists() {
            open_main_window(store.clone(), None, cx);
        }
    });
}

fn popover_options() -> WindowOptions {
    WindowOptions {
        show: false,
        focus: false,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        kind: WindowKind::PopUp,
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(popover::WIDTH), px(300.)),
        })),
        ..Default::default()
    }
}
