//! Checks GitHub releases for a newer version, at most once a day.
//!
//! Only tells the user; packages own /usr/bin, so the app never replaces
//! itself.

use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use eframe::egui;
use update_informer::{registry, Check};

const REPO: &str = "totalretribution/sdFormatLinux-Gui";
pub const RELEASES_URL: &str = "https://github.com/totalretribution/sdFormatLinux-Gui/releases/latest";

/// Set to any value to turn the check off.
const OPT_OUT: &str = "SDFORMAT_GUI_NO_UPDATE_CHECK";

/// Start the check in the background. The receiver gets the new version's
/// tag ("v0.2.0") if there is one. None when the check is turned off.
pub fn spawn(ctx: &egui::Context) -> Option<Receiver<String>> {
    if std::env::var_os(OPT_OUT).is_some() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    thread::spawn(move || {
        let informer = update_informer::new(registry::GitHub, REPO, env!("CARGO_PKG_VERSION"))
            .interval(Duration::from_secs(24 * 60 * 60))
            .timeout(Duration::from_secs(5));
        // Offline, rate limited or a bad response: say nothing.
        if let Ok(Some(v)) = informer.check_version() {
            let _ = tx.send(v.to_string());
            ctx.request_repaint();
        }
    });
    Some(rx)
}
