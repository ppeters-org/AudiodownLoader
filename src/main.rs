#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod config;
mod downloader;

use clap::Parser;
use config::Config;
use eframe::egui;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "yt-audio-downloader", about = "YouTube Audio Downloader")]
struct Args {
    /// Default download path for audio files
    #[arg(short, long)]
    download_path: Option<String>,
    /// YouTube URL to download (optional)
    #[arg(short, long)]
    url: Option<String>,
    /// Audio format (default: mp3)
    #[arg(short, long)]
    format: Option<String>,
    /// Audio quality in kbps (default: 192)
    #[arg(short, long)]
    quality: Option<String>,
    /// Maximum number of tracks to download (default: unlimited)
    #[arg(short, long)]
    max_tracks: Option<u32>,
    /// Save video files in addition to audio
    #[arg(short, long)]
    save_videos: bool,
}

/// Priority: 1. --download-path, 2. saved config path (if it exists), 3. current directory.
fn resolve_download_path(arg: Option<&str>, cfg: &Config) -> PathBuf {
    let cwd = || std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(p) = arg {
        let p = PathBuf::from(p);
        let p = if p.is_absolute() { p } else { cwd().join(p) };
        if p.exists() {
            return p;
        }
        match std::fs::create_dir_all(&p) {
            Ok(()) => return p,
            Err(e) => {
                eprintln!("Error creating download directory: {e}");
                eprintln!("Using current directory instead");
            }
        }
    } else if !cfg.download_path.is_empty() && Path::new(&cfg.download_path).exists() {
        return PathBuf::from(&cfg.download_path);
    }
    cwd()
}

fn main() -> eframe::Result<()> {
    let args = Args::parse();
    let mut cfg = Config::load();

    cfg.download_path = resolve_download_path(args.download_path.as_deref(), &cfg)
        .to_string_lossy()
        .into_owned();
    if let Some(u) = args.url {
        cfg.url = u;
    }
    if let Some(f) = args.format {
        cfg.audio_format = f;
    }
    if let Some(q) = args.quality {
        cfg.audio_quality = q;
    }
    if let Some(m) = args.max_tracks {
        cfg.max_tracks = m;
    }
    if args.save_videos {
        cfg.save_videos = true;
    }
    cfg.save();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("YouTube Audio Downloader")
            .with_position([100.0, 100.0])
            .with_inner_size([800.0, 820.0])
            .with_min_inner_size([800.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "YouTube Audio Downloader",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg)))),
    )
}
