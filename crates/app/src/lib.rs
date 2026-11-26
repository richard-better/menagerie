//! Menagerie's app: a menu bar popover for a glance at the fleet, and a
//! full window for everything else.

use std::path::PathBuf;

use gpui::{
    App, AppContext, Bounds, Entity, Global, TitlebarOptions, WindowBounds, WindowHandle,
    WindowOptions, point, px, size,
};

pub mod assets;
pub mod live;
pub mod platform;
pub mod store;
pub mod sync;
pub mod terminal;
pub mod theme;
pub mod ui {
    pub mod popover;
    pub mod widgets;
    pub mod window;
}

use store::{Job, Store};
use ui::window::FleetWindow;

/// The fleet file to read: `$MENAGERIE_FLEET` if set, else the default
/// location under `~/.config/menagerie`.
pub fn fleet_path() -> PathBuf {
    std::env::var_os("MENAGERIE_FLEET")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(menagerie_fleet::default_path)
        .unwrap_or_else(|| PathBuf::from("fleet.yaml"))
}

/// Open the fleet file in the default text editor, creating it from the
/// example first if it does not exist.
pub fn edit_fleet_file(store: &Entity<Store>, cx: &mut App) {
    let created = store.update(cx, |store, cx| {
        if store.file_exists() {
            return Ok(());
        }
        let result = store.create_from_example();
        cx.notify();
        result
    });
    if let Err(err) = created {
        eprintln!("menagerie: could not create the fleet file: {err}");
        return;
    }
    let path = store.read(cx).path.clone();
    if let Err(err) = std::process::Command::new("open")
        .arg("-t")
        .arg(&path)
        .spawn()
    {
        eprintln!("menagerie: could not open {}: {err}", path.display());
    }
}

/// The main window, while it is open.
#[derive(Default)]
struct MainWindow(Option<WindowHandle<FleetWindow>>);
impl Global for MainWindow {}

pub const WINDOW_WIDTH: f32 = 820.;
pub const WINDOW_HEIGHT: f32 = 760.;

pub fn main_window_options(bounds: Bounds<gpui::Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Menagerie".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(16.), px(16.))),
        }),
        window_min_size: Some(size(px(620.), px(520.))),
        focus: true,
        show: true,
        app_id: Some("menagerie".into()),
        ..Default::default()
    }
}

/// Bring the main window forward, opening it if needed, and show
/// `machine` in it if one is given.
pub fn open_main_window(store: Entity<Store>, machine: Option<String>, cx: &mut App) {
    if let Some(name) = machine {
        store.update(cx, |store, cx| {
            store.select(&name);
            cx.notify();
        });
    }
    let existing = cx.default_global::<MainWindow>().0;
    if let Some(handle) = existing
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return;
    }
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    match cx.open_window(main_window_options(bounds), |_, cx| {
        cx.new(|cx| FleetWindow::new(store.clone(), cx))
    }) {
        Ok(handle) => {
            cx.set_global(MainWindow(Some(handle)));
            cx.activate(true);
        }
        Err(err) => eprintln!("menagerie: could not open the window: {err}"),
    }
}

/// Start a sync or rollback of `name`, unless one is already running. The
/// outcome lands in the store's `jobs`, a notification says when it ends,
/// and a poll follows so the new revision shows.
///
/// Returns why it could not start, after recording that as a failed job.
pub fn start_job(
    store: Entity<Store>,
    name: String,
    action: sync::Action,
    cx: &mut App,
) -> Result<(), String> {
    let planned = {
        let s = store.read(cx);
        if s.running(&name) {
            return Ok(());
        }
        match (s.fleet.as_ref(), s.machine(&name)) {
            (Some(fleet), Some(machine)) => sync::place(&name, machine, &s.hostname)
                .and_then(|place| sync::plan(action, fleet, &name, &place)),
            _ => Err(format!("{name} is not in the fleet file")),
        }
    };
    let failed = planned.as_ref().err().cloned();
    let cancel = sync::Cancel::default();
    let job_cancel = cancel.clone();
    store.update(cx, |s, cx| {
        let now = std::time::Instant::now();
        s.jobs.insert(
            name.clone(),
            Job {
                action,
                lines: Vec::new(),
                result: failed.clone().map(Err),
                started: now,
                finished: failed.as_ref().map(|_| now),
                cancel: job_cancel,
            },
        );
        cx.notify();
    });
    let steps = match planned {
        Ok(steps) => steps,
        Err(why) => {
            sync::notify(&format!("{} {name}: not started", action.label()), &why);
            return Err(why);
        }
    };

    // The runner blocks on child processes, so it gets its own thread; its
    // events cross into gpui through an async channel.
    let (tx, rx) = std::sync::mpsc::channel();
    let (forward, mut events) = futures::channel::mpsc::unbounded();
    std::thread::spawn(move || sync::run(steps, tx, cancel));
    std::thread::spawn(move || {
        for event in rx {
            if forward.unbounded_send(event).is_err() {
                return;
            }
        }
    });
    cx.spawn(async move |cx| {
        use futures::StreamExt;
        while let Some(event) = events.next().await {
            let done = matches!(event, sync::Event::Done(_));
            let _ = store.update(cx, |s, cx| {
                if let Some(job) = s.jobs.get_mut(&name) {
                    match event {
                        sync::Event::Line(line) => job.push(line),
                        sync::Event::Done(result) => {
                            let (title, body) = match &result {
                                Ok(()) => (format!("{} finished", action.label()), name.clone()),
                                Err(why) => (format!("{} failed", action.label()), why.clone()),
                            };
                            sync::notify(&title, &body);
                            job.result = Some(result);
                            job.finished = Some(std::time::Instant::now());
                            s.refresh();
                        }
                    }
                }
                cx.notify();
            });
            if done {
                return;
            }
        }
    })
    .detach();
    Ok(())
}
