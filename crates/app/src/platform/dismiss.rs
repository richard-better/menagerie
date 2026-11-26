//! Hiding the popover when a click lands in another app.
//!
//! The window's own blur handler covers clicks on other windows, but not
//! clicks on another menu bar item: that menu is tracked in its own process
//! and never makes us resign active. A global mouse monitor catches those.
//! It also sees the click on our own status item, so the caller ignores
//! clicks inside the icon's rect.

use block2::RcBlock;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask};

use super::Flip;

/// A live monitor. Dropping it removes the hook.
pub struct OutsideClicks(#[allow(dead_code)] Option<Retained<AnyObject>>);

/// Start watching; returns clicks outside the app, in global top-left points.
pub fn watch() -> (OutsideClicks, UnboundedReceiver<(f64, f64)>) {
    let (tx, rx) = unbounded();
    let block = RcBlock::new(move |_event: std::ptr::NonNull<NSEvent>| {
        let p = NSEvent::mouseLocation();
        let _ = tx.unbounded_send(Flip::current().point(p.x, p.y));
    });
    let token = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
        &block,
    );
    (OutsideClicks(token), rx)
}
