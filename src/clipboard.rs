use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub enum ClipMsg {
    Changed(String),
    Status(String),
}

/// Polls the system clipboard and reports changes until `stop` is set.
/// Whatever is on the clipboard when watching starts is treated as the baseline.
pub fn spawn_watcher(
    interval: Duration,
    stop: Arc<AtomicBool>,
    tx: Sender<ClipMsg>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(ClipMsg::Status(format!("Clipboard unavailable: {e}")));
                ctx.request_repaint();
                return;
            }
        };
        let mut last = clipboard.get_text().ok();
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(interval);
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(text) = clipboard.get_text() else {
                continue;
            };
            if last.as_deref() != Some(text.as_str()) {
                last = Some(text.clone());
                if tx.send(ClipMsg::Changed(text)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        }
    });
}