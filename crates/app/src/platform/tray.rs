//! The status item in the menu bar, via `tray-icon`.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc2::MainThreadMarker;
use tray_icon::menu::{Menu, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use super::{Flip, Rect};

/// The menu bar label. A text title rather than an image keeps it crisp
/// at every scale without shipping a template image yet.
const GLYPH: &str = "◆";

/// One edge of a left click on the status item.
///
/// The popover opens on `Down`, as menu bar items do on macOS; the
/// highlight is re-asserted on `Up`, because `tray-icon` clears it in its
/// own mouse-up handler after ours has run.
pub enum Press {
    Down,
    Up,
    /// A right click: show the Quit menu.
    Right,
}

/// A live status item. Dropping it removes the icon, so hold it for the
/// life of the app.
pub struct Tray {
    icon: TrayIcon,
}

impl Tray {
    pub fn new() -> anyhow::Result<(Self, UnboundedReceiver<Press>)> {
        let (tx, rx) = unbounded();
        set_click_handler(tx);
        // No menu is attached up front: AppKit pops an attached menu on any
        // click, before tray-icon can tell left from right, so a left click
        // would show Quit instead of the popover. See `show_quit_menu`.
        let icon = TrayIconBuilder::new()
            .with_title(GLYPH)
            .with_tooltip("Menagerie")
            .build()?;
        Ok((Self { icon }, rx))
    }

    /// The right-click menu: Quit, the one thing that must always be
    /// reachable. Attached only while it shows, for the reason in `new`.
    pub fn show_quit_menu(&self) {
        let menu = Menu::new();
        if menu
            .append(&PredefinedMenuItem::quit(Some("Quit Menagerie")))
            .is_err()
        {
            return;
        }
        self.icon.set_menu(Some(Box::new(menu)));
        // Shows the menu and returns once it closes.
        self.icon.show_menu();
        self.icon.set_menu(None);
    }

    /// `◆` alone, or `◆ 2` when two machines need attention.
    pub fn set_count(&self, count: usize) {
        let title = if count == 0 {
            GLYPH.to_owned()
        } else {
            format!("{GLYPH} {count}")
        };
        self.icon.set_title(Some(&title));
    }

    /// Where the icon is, in global top-left points.
    ///
    /// Read from the status item's own window: the rect `tray-icon`
    /// reports with each click is in physical pixels, not points.
    pub fn icon_rect(&self) -> Option<Rect> {
        let mtm = MainThreadMarker::new()?;
        let frame = self.icon.ns_status_item()?.button(mtm)?.window()?.frame();
        Some(Flip::current().rect(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ))
    }

    /// Keep the status item lit while the popover is open.
    pub fn set_active(&self, active: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(button) = self.icon.ns_status_item().and_then(|i| i.button(mtm)) {
            button.highlight(active);
        }
    }
}

fn set_click_handler(tx: UnboundedSender<Press>) {
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let TrayIconEvent::Click {
            button,
            button_state,
            ..
        } = event
        else {
            return;
        };
        let press = match (button, button_state) {
            (tray_icon::MouseButton::Left, tray_icon::MouseButtonState::Down) => Press::Down,
            (tray_icon::MouseButton::Left, tray_icon::MouseButtonState::Up) => Press::Up,
            (tray_icon::MouseButton::Right, tray_icon::MouseButtonState::Up) => Press::Right,
            _ => return,
        };
        let _ = tx.unbounded_send(press);
    }));
}
