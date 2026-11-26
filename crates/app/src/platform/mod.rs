//! The macOS menu bar half of the app: the status item, the panel the
//! popover is drawn in, and dismissing it on outside clicks.
//!
//! The approach follows hi5 (https://github.com/jondot/hi5, MIT), a gpui
//! menu bar app: `tray-icon` for the status item, a gpui `PopUp` window
//! that is moved under the icon through AppKit, and a global mouse monitor
//! for dismissal. Everything here works in global, top-left points.

pub mod dismiss;
pub mod panel;
pub mod tray;

/// A rectangle in global, top-left points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Converts AppKit's bottom-left global coordinates to top-left ones,
/// pivoting on the height of the primary display (the one with the menu
/// bar), which is the origin of AppKit's global space.
#[derive(Debug, Clone, Copy)]
pub struct Flip {
    primary_height: f64,
}

impl Flip {
    pub fn new(primary_height: f64) -> Self {
        Self { primary_height }
    }

    /// Read fresh on every call: the primary display can change while the
    /// app runs.
    pub fn current() -> Self {
        let height = objc2::MainThreadMarker::new()
            .and_then(|mtm| {
                objc2_app_kit::NSScreen::screens(mtm)
                    .iter()
                    .next()
                    .map(|s| s.frame().size.height)
            })
            .unwrap_or(0.0);
        Self::new(height)
    }

    pub fn rect(self, x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y: self.primary_height - y - h,
            w,
            h,
        }
    }

    pub fn point(self, x: f64, y: f64) -> (f64, f64) {
        (x, self.primary_height - y)
    }

    /// The AppKit y for a window whose top edge is at top-left `top`.
    pub fn to_cocoa_y(self, top: f64, height: f64) -> f64 {
        self.primary_height - top - height
    }
}

/// Every attached monitor in global, top-left points.
pub fn monitors() -> Vec<Rect> {
    display_info::DisplayInfo::all()
        .unwrap_or_default()
        .into_iter()
        .map(|d| Rect {
            x: d.x as f64,
            y: d.y as f64,
            w: d.width as f64,
            h: d.height as f64,
        })
        .collect()
}

/// The top-left corner for a window `w` wide hanging centred under
/// `icon`: 4pt below it, kept 8pt inside the monitor's edges.
pub fn position_under(icon: Rect, w: f64, monitor: Option<Rect>) -> (f64, f64) {
    let mut x = icon.x + icon.w / 2.0 - w / 2.0;
    if let Some(m) = monitor {
        x = x.clamp(m.x + 8.0, (m.x + m.w - w - 8.0).max(m.x + 8.0));
    }
    (x, icon.y + icon.h + 4.0)
}

pub fn monitor_containing(x: f64, y: f64, monitors: &[Rect]) -> Option<Rect> {
    monitors.iter().copied().find(|m| m.contains(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flips_against_the_primary_height() {
        let flip = Flip::new(1000.0);
        assert_eq!(flip.rect(10.0, 970.0, 30.0, 30.0).y, 0.0);
        assert_eq!(flip.to_cocoa_y(24.0, 400.0), 576.0);
    }

    #[test]
    fn hangs_centred_and_clamped() {
        let monitor = Rect {
            x: 0.0,
            y: 0.0,
            w: 1512.0,
            h: 982.0,
        };
        let icon = Rect {
            x: 1490.0,
            y: 0.0,
            w: 20.0,
            h: 24.0,
        };
        let (x, y) = position_under(icon, 340.0, Some(monitor));
        assert_eq!(x, 1512.0 - 340.0 - 8.0);
        assert_eq!(y, 28.0);
    }
}
