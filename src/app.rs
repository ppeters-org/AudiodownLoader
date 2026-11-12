use crate::clipboard::{self, ClipMsg};
use crate::config::Config;
use crate::downloader::{self, Event, Job, JobHandle, Outcome, PlaylistInfo};
use eframe::egui;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

enum Dialog {
    Info { title: String, body: String },
    ConfirmNonYoutube,
    DebugError(String),
    Select {
        playlist_title: String,
        /// (1-based playlist index, title, checked)
        items: Vec<(usize, String, bool)>,
    },
}

enum Action {
    Close,
    ConfirmYes,
    ContinueDownload,
    CancelDownload,
    StartSelected(Vec<usize>),
}

pub struct App {
    ctx: egui::Context,
    cfg: Config,
    max_tracks_text: String,

    log: String,
    status: String,
    current_file: String,
    progress: Option<f32>,

    job: Option<JobHandle>,
    playlist_rx: Option<Receiver<Result<PlaylistInfo, String>>>,

    dialog: Option<Dialog>,
    queue: VecDeque<Dialog>,

    clip_tx: Sender<ClipMsg>,
    clip_rx: Receiver<ClipMsg>,
    clip_stop: Option<Arc<AtomicBool>>,
}

fn is_valid_url(url: &str) -> bool {
    url.contains("youtube.com") || url.contains("youtu.be")
}

/// egui's built-in font lacks CJK glyphs; video titles often need them.
/// Best effort: use the first common system CJK font that exists.
fn install_fallback_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\YuGothM.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("system_cjk".to_owned(), egui::FontData::from_owned(bytes).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("system_cjk".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: Config) -> Self {
        install_fallback_fonts(&cc.egui_ctx);
        let (clip_tx, clip_rx) = mpsc::channel();
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            max_tracks_text: cfg.max_tracks.to_string(),
            cfg,
            log: String::new(),
            status: "Ready to download".into(),
            current_file: "None".into(),
            progress: None,
            job: None,
            playlist_rx: None,
            dialog: None,
            queue: VecDeque::new(),
            clip_tx,
            clip_rx,
            clip_stop: None,
        };
        if app.cfg.clipboard_monitoring {
            app.start_clipboard();
        }
        app
    }

    // ----- helpers ---------------------------------------------------------

    fn push_log(&mut self, line: &str) {
        self.log.push_str(line);
        self.log.push('\n');
        if self.log.len() > 200_000 {
            let cut = self.log.len() - 150_000;
            let cut = self
                .log
                .char_indices()
                .find(|&(i, c)| i >= cut && c == '\n')
                .map(|(i, _)| i + 1)
                .unwrap_or(self.log.len());
            self.log.drain(..cut);
        }
    }

    fn push_dialog(&mut self, d: Dialog) {
        if self.dialog.is_none() {
            self.dialog = Some(d);
        } else {
            self.queue.push_back(d);
        }
    }

    fn info(&mut self, title: &str, body: &str) {
        self.push_dialog(Dialog::Info {
            title: title.into(),
            body: body.into(),
        });
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.playlist_rx.is_some()
    }

    fn cancel_job(&mut self) {
        if let Some(j) = &self.job {
            j.cancel();
        }
    }

    // ----- clipboard -------------------------------------------------------

    fn start_clipboard(&mut self) {
        if self.clip_stop.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        clipboard::spawn_watcher(
            Duration::from_millis(500),
            stop.clone(),
            self.clip_tx.clone(),
            self.ctx.clone(),
        );
        self.clip_stop = Some(stop);
        self.push_log("Clipboard monitoring started");
    }

    fn stop_clipboard(&mut self) {
        if let Some(stop) = self.clip_stop.take() {
            stop.store(true, Ordering::Relaxed);
            self.push_log("Clipboard monitoring stopped");
        }
    }

    fn on_clip(&mut self, msg: ClipMsg) {
        match msg {
            ClipMsg::Status(s) => self.push_log(&s),
            ClipMsg::Changed(text) => {
                let t = text.trim().lines().next().unwrap_or("").trim().to_string();
                if !is_valid_url(&t) {
                    return; // ignore invalid URLs
                }
                self.cfg.url = t;
                self.cfg.save();
                if self.cfg.autodownload_enabled {
                    if self.busy() || self.dialog.is_some() {
                        self.push_log("Auto-download skipped: busy");
                    } else {
                        self.start_download();
                    }
                }
            }
        }
    }

    // ----- download flow ---------------------------------------------------

    fn start_download(&mut self) {
        if self.busy() {
            self.info("Download in Progress", "Audio extraction is already in progress.");
            return;
        }
        let url = self.cfg.url.trim().to_string();
        if url.is_empty() {
            self.info("Empty URL", "Please enter a YouTube URL.");
            return;
        }
        if !is_valid_url(&url) {
            self.push_dialog(Dialog::ConfirmNonYoutube);
            return;
        }
        self.after_url_check();
    }

    fn after_url_check(&mut self) {
        self.cfg.url = self.cfg.url.trim().to_string();
        if self.cfg.download_path.trim().is_empty() {
            self.cfg.download_path = std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".into());
        }
        self.cfg.save();

        if self.cfg.extended_mode {
            self.status = "Loading playlist info...".into();
            let (tx, rx) = mpsc::channel();
            let url = self.cfg.url.clone();
            let ctx = self.ctx.clone();
            thread::spawn(move || {
                let _ = tx.send(downloader::fetch_playlist(&url));
                ctx.request_repaint();
            });
            self.playlist_rx = Some(rx);
        } else {
            self.begin_download(None);
        }
    }

    fn on_playlist(&mut self, res: Result<PlaylistInfo, String>) {
        self.status = "Ready to download".into();
        match res {
            Err(e) => self.info("Error", &format!("Failed to load playlist info: {e}")),
            Ok(info) => match info.entries {
                None => self.info(
                    "Not a Playlist",
                    "This URL is not a playlist or channel. No videos will be downloaded.",
                ),
                Some(e) if e.is_empty() => {
                    self.info("Empty Playlist", "No videos found in this playlist.")
                }
                Some(entries) => self.push_dialog(Dialog::Select {
                    playlist_title: info.title,
                    items: entries
                        .into_iter()
                        .enumerate()
                        .map(|(i, t)| (i + 1, t, true))
                        .collect(),
                }),
            },
        }
    }

    fn begin_download(&mut self, selected: Option<Vec<usize>>) {
        self.log.clear();
        self.progress = Some(0.0);
        self.status = format!("Downloading from: {}", self.cfg.url);
        let job = Job {
            url: self.cfg.url.trim().to_string(),
            download_path: self.cfg.download_path.clone(),
            audio_format: self.cfg.audio_format.clone(),
            audio_quality: self.cfg.audio_quality.clone(),
            max_tracks: self.cfg.max_tracks,
            save_videos: self.cfg.save_videos,
            prefer_h264: self.cfg.prefer_h264,
            playlist_items: selected.map(|mut v| {
                v.sort_unstable();
                v
            }),
        };
        self.job = Some(downloader::spawn(job, self.ctx.clone()));
    }

    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Log(s) => {
                self.status = s.clone();
                self.push_log(&s);
            }
            Event::Item { name, status } => self.push_log(&format!("{name}: {status}")),
            Event::Detail(s) => self.status = s,
            Event::CurrentFile(n) => self.current_file = n,
            Event::Percent(p) => self.progress = Some(p),
            Event::Error(m) => {
                self.push_log(&format!("Error: {m}"));
                if self.cfg.debug_mode {
                    self.push_dialog(Dialog::DebugError(m));
                } else {
                    self.status = format!("Error: {m}");
                }
            }
            Event::Finished(f) => {
                self.job = None;
                self.progress = None;
                self.current_file = "None".into();
                match f.outcome {
                    Outcome::Success => {
                        self.status = format!(
                            "Audio extraction completed! Downloaded {} audio file(s).",
                            f.audio
                        );
                        let body = if f.videos > 0 {
                            format!(
                                "Download completed successfully!\nAudio files: {}\nVideo files: {}",
                                f.audio, f.videos
                            )
                        } else {
                            format!(
                                "Audio extraction completed successfully!\nDownloaded {} audio file(s).",
                                f.audio
                            )
                        };
                        self.info("Success", &body);
                    }
                    Outcome::Failed(m) => {
                        self.status = m.clone();
                        self.info("Error", &m);
                    }
                    Outcome::Cancelled => {
                        self.status = "Download cancelled".into();
                        self.push_log("Download cancelled");
                        self.info("Cancelled", "Download cancelled.");
                    }
                }
            }
        }
    }

    fn poll(&mut self) {
        let events = self
            .job
            .as_ref()
            .map(|j| j.rx.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for ev in events {
            self.on_event(ev);
        }

        let polled = self.playlist_rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(res)) => {
                self.playlist_rx = None;
                self.on_playlist(res);
            }
            Some(Err(TryRecvError::Disconnected)) => self.playlist_rx = None,
            _ => {}
        }

        let clips: Vec<ClipMsg> = self.clip_rx.try_iter().collect();
        for m in clips {
            self.on_clip(m);
        }

        if self.dialog.is_none() {
            self.dialog = self.queue.pop_front();
        }
    }

    // ----- UI --------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Options", |ui| {
                let mut changed = false;
                changed |= ui.checkbox(&mut self.cfg.save_videos, "Save video files").changed();
                changed |= ui.checkbox(&mut self.cfg.prefer_h264, "Prefer H.264 codec").changed();
                changed |= ui
                    .checkbox(&mut self.cfg.debug_mode, "Debug mode (pause on errors)")
                    .changed();
                changed |= ui
                    .checkbox(&mut self.cfg.extended_mode, "Extended (select videos)")
                    .changed();
                if changed {
                    self.cfg.save();
                }
            });
        });
    }

    fn main_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("YouTube Audio Downloader").size(22.0).strong());
        ui.add_space(8.0);

        ui.label("YouTube URL (Video, Playlist, or Channel):");
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 70.0).max(100.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.cfg.url)
                    .hint_text("Paste YouTube URL here...")
                    .desired_width(w),
            );
            if r.changed() {
                self.cfg.save();
            }
            if ui.button("Clear").clicked() {
                self.cfg.url.clear();
                self.cfg.save();
            }
        });
        ui.add_space(6.0);

        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Audio Settings").strong());
            egui::Grid::new("audio_settings")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Format:");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.cfg.audio_format)
                            .hint_text("mp3, m4a, wav, etc.")
                            .desired_width(500.0),
                    );
                    if r.changed() {
                        self.cfg.save();
                    }
                    ui.end_row();

                    ui.label("Quality (kbps):");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.cfg.audio_quality)
                            .hint_text("128, 192, 256, 320")
                            .desired_width(500.0),
                    );
                    if r.changed() {
                        self.cfg.save();
                    }
                    ui.end_row();

                    // The limit only applies when videos are not hand-picked.
                    if !self.cfg.extended_mode {
                        ui.label("Max Tracks (0 = unlimited):");
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.max_tracks_text)
                                .hint_text("0 for unlimited, or number")
                                .desired_width(500.0),
                        );
                        if r.changed() {
                            self.cfg.max_tracks = self.max_tracks_text.trim().parse().unwrap_or(0);
                            self.cfg.save();
                        }
                        ui.end_row();
                    }
                });
        });
        ui.add_space(6.0);

        ui.label("Save to:");
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 80.0).max(100.0);
            let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.download_path).desired_width(w));
            if r.changed() {
                self.cfg.save();
            }
            if ui.button("Browse").clicked() {
                let mut dlg = rfd::FileDialog::new().set_title("Select Directory");
                if !self.cfg.download_path.is_empty() {
                    dlg = dlg.set_directory(&self.cfg.download_path);
                }
                if let Some(p) = dlg.pick_folder() {
                    self.cfg.download_path = p.to_string_lossy().into_owned();
                    self.cfg.save();
                }
            }
        });
        ui.add_space(6.0);

        let busy = self.busy();
        let running = self.job.is_some();
        ui.horizontal(|ui| {
            let cancel_w = if running { 90.0 } else { 0.0 };
            let gap = if running { 8.0 } else { 0.0 };
            let w = (ui.available_width() - cancel_w - gap).max(100.0);
            if ui
                .add_enabled(!busy, egui::Button::new("Download").min_size(egui::vec2(w, 30.0)))
                .clicked()
            {
                self.start_download();
            }
            if running
                && ui
                    .add(egui::Button::new("Cancel").min_size(egui::vec2(cancel_w, 30.0)))
                    .clicked()
            {
                self.cancel_job();
            }
        });

        if let Some(p) = self.progress {
            ui.add(egui::ProgressBar::new((p / 100.0).clamp(0.0, 1.0)).show_percentage());
        }
        ui.label(&self.status);
        ui.label(egui::RichText::new(format!("Currently downloading: {}", self.current_file)).weak());
        ui.add_space(6.0);

        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Download Progress").strong());
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let mut text: &str = self.log.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(8),
                    );
                });
        });
        ui.add_space(6.0);

        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Clipboard Monitoring").strong());
            if ui
                .checkbox(&mut self.cfg.clipboard_monitoring, "Enable clipboard monitoring")
                .changed()
            {
                self.cfg.save();
                if self.cfg.clipboard_monitoring {
                    self.start_clipboard();
                } else {
                    self.stop_clipboard();
                }
            }
            if ui
                .checkbox(
                    &mut self.cfg.autodownload_enabled,
                    "Auto-download when clipboard changes",
                )
                .changed()
            {
                self.cfg.save();
            }
        });
        ui.add_space(6.0);

        ui.label(
            egui::RichText::new(
                "Instructions:\n\
                 1. Paste a YouTube URL (video, playlist, or channel)\n\
                 2. Choose audio format and quality\n\
                 3. Set max tracks limit (optional)\n\
                 4. Choose where to save the audio files\n\
                 5. Click 'Download' to start the process\n\n\
                 Use the Options menu to:\n\
                 - Save video files in addition to audio\n\
                 - Prefer H.264 codec for better compatibility\n\
                 - Enable debug mode to show a dialog on errors\n\
                 - Enable extended mode to select specific videos from a playlist",
            )
            .weak(),
        );
    }

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        let mut action: Option<Action> = None;
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };
        let anchor = egui::Align2::CENTER_CENTER;

        match dialog {
            Dialog::Info { title, body } => {
                egui::Window::new(title.as_str())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(anchor, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_max_width(460.0);
                        ui.label(body.as_str());
                        ui.add_space(8.0);
                        if ui.button("OK").clicked() {
                            action = Some(Action::Close);
                        }
                    });
            }
            Dialog::ConfirmNonYoutube => {
                egui::Window::new("Invalid URL")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(anchor, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("This doesn't appear to be a valid URL. Continue anyway?");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Yes").clicked() {
                                action = Some(Action::ConfirmYes);
                            }
                            if ui.button("No").clicked() {
                                action = Some(Action::Close);
                            }
                        });
                    });
            }
            Dialog::DebugError(msg) => {
                egui::Window::new("Download Error")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(anchor, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.set_max_width(460.0);
                        ui.label(format!(
                            "{msg}\n\nClick OK to continue or Cancel to stop the download."
                        ));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                action = Some(Action::ContinueDownload);
                            }
                            if ui.button("Cancel").clicked() {
                                action = Some(Action::CancelDownload);
                            }
                        });
                    });
            }
            Dialog::Select { playlist_title, items } => {
                egui::Window::new("Select Videos to Download")
                    .collapsible(false)
                    .resizable(false)
                    .fixed_size([600.0, 500.0])
                    .anchor(anchor, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "Select videos from '{}' ({} videos):",
                            playlist_title,
                            items.len()
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Select all").clicked() {
                                items.iter_mut().for_each(|i| i.2 = true);
                            }
                            if ui.button("Select none").clicked() {
                                items.iter_mut().for_each(|i| i.2 = false);
                            }
                        });
                        egui::ScrollArea::vertical()
                            .max_height(380.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (idx, title, checked) in items.iter_mut() {
                                    ui.checkbox(checked, format!("{:03} - {}", idx, title));
                                }
                            });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                let sel: Vec<usize> =
                                    items.iter().filter(|i| i.2).map(|i| i.0).collect();
                                action = Some(Action::StartSelected(sel));
                            }
                            if ui.button("Cancel").clicked() {
                                action = Some(Action::Close);
                            }
                        });
                    });
            }
        }

        if let Some(a) = action {
            self.dialog = None;
            match a {
                Action::Close | Action::ContinueDownload => {}
                Action::ConfirmYes => self.after_url_check(),
                Action::CancelDownload => self.cancel_job(),
                Action::StartSelected(sel) => {
                    if sel.is_empty() {
                        self.info(
                            "No Selection",
                            "No videos selected. Please select at least one video.",
                        );
                    } else {
                        self.begin_download(Some(sel));
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| self.menu_bar(ui));

        let blocked = self.dialog.is_some();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!blocked, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.main_ui(ui));
            });
        });

        self.show_dialogs(ctx);
    }
}