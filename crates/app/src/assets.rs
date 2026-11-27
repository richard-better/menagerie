//! Icons, embedded in the binary so a missing file can never turn into a
//! blank square at runtime. Drawn on a 24x24 box with 2pt round strokes.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub mod icon {
    pub const REFRESH: &str = "icons/refresh.svg";
    pub const SETTINGS: &str = "icons/settings.svg";
    pub const WINDOW: &str = "icons/window.svg";
    pub const PLUS: &str = "icons/plus.svg";
    pub const SYNC: &str = "icons/sync.svg";
    pub const MORE: &str = "icons/more.svg";
    pub const EXTERNAL: &str = "icons/external.svg";
    pub const BACK: &str = "icons/back.svg";
}

const ICONS: &[(&str, &str)] = &[
    (icon::REFRESH, include_str!("../assets/icons/refresh.svg")),
    (icon::SETTINGS, include_str!("../assets/icons/settings.svg")),
    (icon::WINDOW, include_str!("../assets/icons/window.svg")),
    (icon::PLUS, include_str!("../assets/icons/plus.svg")),
    (icon::SYNC, include_str!("../assets/icons/sync.svg")),
    (icon::MORE, include_str!("../assets/icons/more.svg")),
    (icon::EXTERNAL, include_str!("../assets/icons/external.svg")),
    (icon::BACK, include_str!("../assets/icons/back.svg")),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, svg)| Cow::Borrowed(svg.as_bytes())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
