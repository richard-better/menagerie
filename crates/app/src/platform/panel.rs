//! What gpui 0.2.2 cannot do to a window: move, resize and hide one after
//! it is open (its `PlatformWindow` has no setters, and `hide` hides the
//! whole app), and run without a Dock icon (`did_finish_launching` forces
//! the Regular activation policy).

use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{Flip, Rect, monitor_containing, monitors, position_under};

/// The `NSWindow` behind the gpui window the popover draws into.
#[derive(Clone, Copy)]
pub struct Panel {
    window: *mut AnyObject,
}

// Only touched on the main thread: every method takes a MainThreadMarker.
unsafe impl Send for Panel {}
unsafe impl Sync for Panel {}

impl Panel {
    pub fn adopt(handle: &impl HasWindowHandle) -> Option<Self> {
        let raw = handle.window_handle().ok()?;
        let RawWindowHandle::AppKit(h) = raw.as_raw() else {
            return None;
        };
        let view: *mut AnyObject = h.ns_view.as_ptr().cast();
        let window: *mut AnyObject = unsafe { msg_send![view, window] };
        (!window.is_null()).then_some(Self { window })
    }

    pub fn is_visible(&self, _mtm: MainThreadMarker) -> bool {
        unsafe { msg_send![self.window, isVisible] }
    }

    pub fn hide(&self, _mtm: MainThreadMarker) {
        unsafe {
            let _: () = msg_send![self.window, orderOut: std::ptr::null::<AnyObject>()];
        }
    }

    /// Size the panel to `w` by `h` and show it centred under `icon`.
    pub fn show_under(&self, icon: Rect, w: f64, h: f64, mtm: MainThreadMarker) {
        let monitors = monitors();
        let monitor = monitor_containing(icon.x + icon.w / 2.0, icon.y + icon.h / 2.0, &monitors)
            .or(monitors.first().copied());
        let (x, y) = position_under(icon, w, monitor);
        self.show_at(x, y, w, h, mtm);
    }

    fn show_at(&self, x: f64, top: f64, w: f64, h: f64, _mtm: MainThreadMarker) {
        let frame = NSRect::new(
            NSPoint::new(x, Flip::current().to_cocoa_y(top, h)),
            NSSize::new(w, h),
        );
        unsafe {
            let _: () = msg_send![self.window, setFrame: frame, display: true];
            let _: () = msg_send![self.window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
    }
}

/// Drop the Dock icon. Must run after gpui's own launch handler, which
/// sets the policy back to Regular.
pub fn become_accessory(mtm: MainThreadMarker) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}
