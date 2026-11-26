//! The app. For now it opens the main window; the menu bar comes next.

use gpui::{AppContext, Application};

use menagerie_app::store::Store;
use menagerie_app::{assets, fleet_path, open_main_window};

fn main() {
    Application::new().with_assets(assets::Assets).run(|cx| {
        let store = cx.new(|_| Store::load(fleet_path()));
        open_main_window(store, None, cx);
    });
}
