//! Render the window and the popover from a fleet file and save a PNG of
//! each. No menu bar item, no clicking: a quick way to look at layout and
//! type with real fonts and pixels.
//!
//!     cargo run -p menagerie --bin preview -- [out-dir] [fleet.yaml]
//!
//! Defaults: `target/preview` and the bundled example fleet. Capturing
//! uses `screencapture`, which needs the Screen Recording permission for
//! the terminal that runs it.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AppContext, Application, Bounds, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, point, px, size,
};

use menagerie_app::store::Store;
use menagerie_app::ui::popover::{self, Hooks, Popover};
use menagerie_app::ui::window::FleetWindow;
use menagerie_app::{WINDOW_HEIGHT, WINDOW_WIDTH, assets, main_window_options};

/// Where the two windows sit while they are photographed, in points from
/// the primary display's top-left corner.
const WINDOW_AT: (f32, f32) = (60., 80.);
const POPOVER_AT: (f32, f32) = (WINDOW_AT.0 + WINDOW_WIDTH + 40., 80.);

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/preview".into()));
    let fleet = args.next().map(PathBuf::from).unwrap_or_else(example_fleet);
    std::fs::create_dir_all(&out).expect("create the output directory");

    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx| {
            let store = cx.new(|_| Store::load(fleet.clone()));
            let height = popover::height(store.read(cx));

            let window_bounds = Bounds {
                origin: point(px(WINDOW_AT.0), px(WINDOW_AT.1)),
                size: size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
            };
            let window_store = store.clone();
            cx.open_window(main_window_options(window_bounds), |_, cx| {
                cx.new(|cx| FleetWindow::new(window_store, cx))
            })
            .expect("open the window");

            let hooks = Hooks {
                open_window: Rc::new(|_, _| {}),
            };
            let popover_store = store.clone();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(POPOVER_AT.0), px(POPOVER_AT.1)),
                        size: size(px(popover::WIDTH), px(height)),
                    })),
                    titlebar: None,
                    focus: false,
                    kind: WindowKind::PopUp,
                    window_background: WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| Popover::new(popover_store, hooks, cx)),
            )
            .expect("open the popover");
            // Bring both to the front, or the window is photographed behind
            // whatever app was active.
            cx.activate(true);

            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(900))
                    .await;
                capture(
                    WINDOW_AT,
                    (WINDOW_WIDTH, WINDOW_HEIGHT),
                    &out.join("window.png"),
                );
                capture(
                    POPOVER_AT,
                    (popover::WIDTH, height),
                    &out.join("popover.png"),
                );
                let _ = cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

fn example_fleet() -> PathBuf {
    let path = std::env::temp_dir().join("menagerie-preview-fleet.yaml");
    std::fs::write(&path, menagerie_app::store::EXAMPLE).expect("write the example fleet");
    path
}

fn capture(origin: (f32, f32), size: (f32, f32), path: &Path) {
    let region = format!("{},{},{},{}", origin.0, origin.1, size.0, size.1);
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", "-R", &region])
        .arg(path)
        .status();
    match status {
        Ok(s) if s.success() => println!("{}", path.display()),
        _ => eprintln!("failed to capture {}", path.display()),
    }
}
