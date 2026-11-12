//! Download engine. There is no yt-dlp library for Rust, so this drives the
//! `yt-dlp` and `ffmpeg` executables (which must be on PATH) and parses
//! machine-readable progress lines from yt-dlp's stdout.

use eframe::egui;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MB: f64 = 1024.0 * 1024.0;
const OUTPUT_NAME: &str = "%(playlist_index)03d - %(title).100B.[%(id)s].%(ext)s";

// Progress lines are prefixed so they can be told apart from yt-dlp's other output.
// The free-text field (filename / title) is always last so a '|' in it is harmless.
const DL_TEMPLATE: &str = "download:RSDL|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(progress.filename)s";
const PP_TEMPLATE: &str = "postprocess:RSPP|%(progress.status)s|%(progress.postprocessor)s|%(info.title)s";
const FILE_PRINT: &str = "after_move:RSFILE|%(filepath)s|%(title)s";

/// Maps a requested audio format to (ffmpeg codec, file extension, uses_bitrate).
fn codec_for(fmt: &str) -> (&'static str, &'static str, bool) {
    match fmt {
        "mp3" => ("libmp3lame", "mp3", true),
        "aac" => ("aac", "aac", true),
        "m4a" => ("aac", "m4a", true),
        "opus" => ("libopus", "opus", true),
        "vorbis" => ("libvorbis", "ogg", true),
        "wav" => ("pcm_s16le", "wav", false),
        "flac" => ("flac", "flac", false),
        _ => ("libmp3lame", "mp3", true),
    }
}

#[derive(Clone)]
pub struct Job {
    pub url: String,
    pub download_path: String,
    pub audio_format: String,
    pub audio_quality: String,
    pub max_tracks: u32,
    pub save_videos: bool,
    pub prefer_h264: bool,
    /// 1-based playlist indices chosen in the selection dialog.
    pub playlist_items: Option<Vec<usize>>,
}

pub enum Outcome {
    Success,
    Failed(String),
    Cancelled,
}

pub struct Finished {
    pub outcome: Outcome,
    pub audio: usize,
    pub videos: usize,
}

pub enum Event {
    /// Status label + log.
    Log(String),
    /// Log line "name: status".
    Item { name: String, status: String },
    /// Live progress text (status label only).
    Detail(String),
    CurrentFile(String),
    Percent(f32),
    Error(String),
    Finished(Finished),
}

type ChildSlot = Arc<Mutex<Option<Child>>>;

pub struct JobHandle {
    pub rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    slot: ChildSlot,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(child) = self.slot.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }
}

#[derive(Clone)]
struct Emitter {
    tx: Sender<Event>,
    ctx: egui::Context,
}

impl Emitter {
    fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ctx.request_repaint();
    }
    fn log(&self, s: impl Into<String>) {
        self.send(Event::Log(s.into()));
    }
    fn item(&self, name: &str, status: &str) {
        self.send(Event::Item {
            name: name.to_string(),
            status: status.to_string(),
        });
    }
}

#[allow(unused_mut)]
fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn for_each_line<R: Read>(reader: R, mut f: impl FnMut(&str)) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let s = String::from_utf8_lossy(&buf);
                f(s.trim_end_matches(['\r', '\n']));
            }
        }
    }
}

fn num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

fn fmt_speed(speed: f64) -> String {
    if speed > MB {
        format!("@ {:.1}MB/s", speed / MB)
    } else if speed > 1024.0 {
        format!("@ {:.1}KB/s", speed / 1024.0)
    } else {
        format!("@ {:.1}B/s", speed)
    }
}

fn fmt_eta(eta: f64) -> String {
    if eta > 3600.0 {
        format!("ETA: {}h {}m", (eta / 3600.0) as u64, ((eta % 3600.0) / 60.0) as u64)
    } else if eta > 60.0 {
        format!("ETA: {}m {}s", (eta / 60.0) as u64, (eta % 60.0) as u64)
    } else {
        format!("ETA: {}s", eta as u64)
    }
}

fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

// ---------------------------------------------------------------------------
// Playlist info (for the "Extended" video selection dialog)
// ---------------------------------------------------------------------------

pub struct PlaylistInfo {
    pub title: String,
    /// `None` means the URL is a single video, not a playlist/channel.
    pub entries: Option<Vec<String>>,
}

pub fn fetch_playlist(url: &str) -> Result<PlaylistInfo, String> {
    let output = command("yt-dlp")
        .args(["--flat-playlist", "--dump-single-json", "--no-warnings", "--"])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(spawn_error)?;

    if output.stdout.is_empty() {
        let err = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = err.lines().rev().take(3).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(tail.join(" "));
    }
    let v: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse yt-dlp output: {e}"))?;

    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Playlist")
        .to_string();
    let entries = v.get("entries").and_then(|e| e.as_array()).map(|arr| {
        arr.iter()
            .enumerate()
            .map(|(i, e)| {
                e.get("title")
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Video {}", i + 1))
            })
            .collect()
    });
    Ok(PlaylistInfo { title, entries })
}

fn spawn_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "yt-dlp was not found. Please install yt-dlp and make sure it is available on your PATH."
            .to_string()
    } else {
        format!("Failed to start yt-dlp: {e}")
    }
}

// ---------------------------------------------------------------------------
// Download job
// ---------------------------------------------------------------------------

pub fn spawn(job: Job, ctx: egui::Context) -> JobHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let slot: ChildSlot = Arc::new(Mutex::new(None));
    let em = Emitter { tx, ctx };
    let (c2, s2) = (cancel.clone(), slot.clone());
    thread::spawn(move || {
        let finished = run(&job, &em, &c2, &s2);
        em.send(Event::Finished(finished));
    });
    JobHandle { rx, cancel, slot }
}

#[derive(Default)]
struct Counts {
    audio: usize,
    videos: usize,
}

fn run(job: &Job, em: &Emitter, cancel: &AtomicBool, slot: &ChildSlot) -> Finished {
    let mut counts = Counts::default();
    let res = run_inner(job, em, cancel, slot, &mut counts);
    let outcome = if cancel.load(Ordering::Relaxed) {
        Outcome::Cancelled
    } else {
        match res {
            Ok(()) => Outcome::Success,
            Err(e) => Outcome::Failed(format!("Error during audio extraction: {e}")),
        }
    };
    Finished {
        outcome,
        audio: counts.audio,
        videos: counts.videos,
    }
}

fn output_template(dir: &str) -> String {
    // '%' in the directory must be escaped for yt-dlp's template engine.
    PathBuf::from(dir.replace('%', "%%"))
        .join(OUTPUT_NAME)
        .to_string_lossy()
        .into_owned()
}

fn run_inner(
    job: &Job,
    em: &Emitter,
    cancel: &AtomicBool,
    slot: &ChildSlot,
    counts: &mut Counts,
) -> Result<(), String> {
    let mut args: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(|s| s.to_string()));

    if job.save_videos {
        // Download + merge the video once; audio is extracted afterwards with a
        // direct ffmpeg call so the video file is left untouched.
        let fmt = if job.prefer_h264 {
            "bestvideo[vcodec^=avc1]+bestaudio[ext=m4a]/best[vcodec^=avc1]/bestvideo+bestaudio/best"
        } else {
            "bestvideo+bestaudio/best"
        };
        push(&["-f", fmt, "--merge-output-format", "mp4"]);
        if job.prefer_h264 {
            em.log("Video codec preference: H.264");
        }
        em.log("Mode: Saving both video and audio files");
        em.log("Downloading and merging video...");
    } else {
        let fmt = job.audio_format.trim().to_lowercase();
        push(&["-f", "bestaudio/best", "-x", "--audio-format", &fmt]);
        let (_, _, uses_bitrate) = codec_for(&fmt);
        let q = clean_quality(&job.audio_quality);
        if uses_bitrate && !q.is_empty() {
            push(&["--audio-quality", &format!("{q}K")]);
        }
        em.log("Mode: Audio only");
        em.log("Extracting video information...");
    }

    if let Some(items) = &job.playlist_items {
        let list = items.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        push(&["--playlist-items", &list]);
        em.log(format!("Downloading selected video(s): {list}"));
    } else if job.max_tracks > 0 {
        push(&["--playlist-end", &job.max_tracks.to_string()]);
        em.log(format!("Limiting download to {} track(s)", job.max_tracks));
    }

    let template = output_template(&job.download_path);
    push(&[
        "-o",
        &template,
        "--ignore-errors",
        "--no-quiet",
        "--no-simulate",
        "--newline",
        "--progress-template",
        DL_TEMPLATE,
        "--progress-template",
        PP_TEMPLATE,
        "--print",
        FILE_PRINT,
        "--",
        &job.url,
    ]);

    let result = run_ytdlp(&args, job.save_videos, em, cancel, slot)?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    if result.files.is_empty() && !result.success {
        return Err(match result.code {
            Some(c) => format!("yt-dlp exited with code {c} and produced no files - see the log for details"),
            None => "yt-dlp failed and produced no files - see the log for details".to_string(),
        });
    }

    if !job.save_videos {
        counts.audio = result.files.len();
        em.log("Audio extraction completed!");
        return Ok(());
    }

    em.log("Extracting audio from downloaded video file(s)...");
    for (path, title) in &result.files {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if !path.exists() {
            em.send(Event::Error(format!(
                "Could not locate the downloaded video file for '{title}' to extract audio from"
            )));
            continue;
        }
        counts.videos += 1;
        em.item(title, "Extracting audio from video...");
        match extract_audio_via_ffmpeg(job, path) {
            Ok(()) => {
                counts.audio += 1;
                em.item(title, "Audio extraction complete");
            }
            Err(msg) => {
                em.send(Event::Error(msg));
                em.item(title, "Audio extraction failed - see log");
            }
        }
    }
    Ok(())
}

fn clean_quality(q: &str) -> String {
    q.trim().trim_end_matches(['k', 'K']).to_string()
}

fn extract_audio_via_ffmpeg(job: &Job, video_path: &Path) -> Result<(), String> {
    let fmt = job.audio_format.trim().to_lowercase();
    let (codec, ext, uses_bitrate) = codec_for(&fmt);
    let out = video_path.with_extension(ext);
    let name = video_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut cmd = command("ffmpeg");
    cmd.args(["-y", "-nostdin", "-i"])
        .arg(video_path)
        .args(["-vn", "-acodec", codec]);
    if uses_bitrate {
        cmd.arg("-b:a")
            .arg(format!("{}k", clean_quality(&job.audio_quality)));
    }
    cmd.arg(&out);

    match cmd.output() {
        Ok(o) if o.status.success() => {
            if out.exists() {
                Ok(())
            } else {
                Err(format!("ffmpeg finished but '{}' was not created", out.display()))
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let chars: Vec<char> = stderr.trim().chars().collect();
            let tail: String = chars[chars.len().saturating_sub(500)..].iter().collect();
            Err(format!("ffmpeg audio extraction failed for '{name}': {tail}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(
            "ffmpeg was not found. Please make sure ffmpeg is installed and available on your PATH."
                .to_string(),
        ),
        Err(e) => Err(format!("Error running ffmpeg on '{name}': {e}")),
    }
}

struct RunResult {
    /// (final file path, title) for every item yt-dlp finished.
    files: Vec<(PathBuf, String)>,
    success: bool,
    code: Option<i32>,
}

#[derive(Default)]
struct ProgState {
    last_name: String,
    last_bucket: i32,
}

fn run_ytdlp(
    args: &[String],
    save_videos: bool,
    em: &Emitter,
    cancel: &AtomicBool,
    slot: &ChildSlot,
) -> Result<RunResult, String> {
    let mut child = command("yt-dlp")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .spawn()
        .map_err(spawn_error)?;
    let stdout = child.stdout.take().ok_or("failed to capture yt-dlp output")?;
    let stderr = child.stderr.take().ok_or("failed to capture yt-dlp errors")?;
    *slot.lock().unwrap() = Some(child);
    if cancel.load(Ordering::Relaxed) {
        if let Some(c) = slot.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    // yt-dlp's own warnings/errors arrive on stderr.
    let err_em = em.clone();
    let err_thread = thread::spawn(move || {
        for_each_line(stderr, |line| {
            if let Some(m) = line.strip_prefix("ERROR:") {
                err_em.send(Event::Error(format!("yt-dlp error: {}", m.trim())));
            } else if let Some(m) = line.strip_prefix("WARNING:") {
                err_em.log(format!("yt-dlp warning: {}", m.trim()));
            }
        });
    });

    let mut files: Vec<(PathBuf, String)> = Vec::new();
    let mut state = ProgState::default();
    for_each_line(stdout, |line| {
        if let Some(rest) = line.strip_prefix("RSDL|") {
            handle_download_progress(rest, save_videos, em, &mut state);
        } else if let Some(rest) = line.strip_prefix("RSPP|") {
            let f: Vec<&str> = rest.splitn(3, '|').collect();
            if f.len() == 3 && f[0] == "started" {
                match f[1] {
                    "ExtractAudio" => em.item(f[2], "Converting to audio format..."),
                    "Merger" => em.item(f[2], "Merging video and audio..."),
                    _ => {}
                }
            }
        } else if let Some(rest) = line.strip_prefix("RSFILE|") {
            let mut it = rest.splitn(2, '|');
            let path = it.next().unwrap_or("");
            let title = it.next().unwrap_or("Unknown video");
            if !path.is_empty() {
                files.push((PathBuf::from(path), title.to_string()));
            }
        } else if line.starts_with("[download] Downloading item")
            || line.starts_with("[download] Downloading playlist")
        {
            em.log(line.trim_start_matches("[download] ").to_string());
        }
    });
    let _ = err_thread.join();

    let status = loop {
        let polled = {
            let mut guard = slot.lock().unwrap();
            match guard.as_mut() {
                Some(c) => c.try_wait(),
                None => break None,
            }
        };
        match polled {
            Ok(Some(st)) => break Some(st),
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    *slot.lock().unwrap() = None;

    Ok(RunResult {
        files,
        success: status.map(|s| s.success()).unwrap_or(false),
        code: status.and_then(|s| s.code()),
    })
}

fn handle_download_progress(rest: &str, save_videos: bool, em: &Emitter, state: &mut ProgState) {
    let f: Vec<&str> = rest.splitn(7, '|').collect();
    if f.len() < 7 {
        return;
    }
    let filename = base_name(f[6]);
    match f[0] {
        "downloading" => {
            em.send(Event::CurrentFile(filename.clone()));
            let downloaded = num(f[1]);
            let total = num(f[2]).or_else(|| num(f[3]));
            let mut parts: Vec<String> = Vec::new();
            let mut pct: Option<f64> = None;
            match (downloaded, total) {
                (Some(d), Some(t)) => {
                    let p = if t > 0.0 { d / t * 100.0 } else { 0.0 };
                    pct = Some(p);
                    parts.push(format!("{p:.1}%"));
                    em.send(Event::Percent(p as f32));
                    parts.push(format!("{:.1}MB / {:.1}MB", d / MB, t / MB));
                }
                (Some(d), None) => parts.push(format!("{:.1}MB downloaded", d / MB)),
                _ => {}
            }
            if let Some(s) = num(f[4]) {
                parts.push(fmt_speed(s));
            }
            if let Some(e) = num(f[5]) {
                parts.push(fmt_eta(e));
            }
            let msg = if parts.is_empty() {
                "Downloading...".to_string()
            } else {
                parts.join(" | ")
            };
            em.send(Event::Detail(format!("{filename}: {msg}")));

            // Only write to the log on a new file or every 10% step.
            let bucket = pct.map(|p| (p / 10.0) as i32);
            if state.last_name != filename || bucket.is_some_and(|b| b != state.last_bucket) {
                state.last_name = filename.clone();
                state.last_bucket = bucket.unwrap_or(-1);
                em.item(&filename, &msg);
            }
        }
        "finished" => {
            em.send(Event::CurrentFile(filename.clone()));
            em.send(Event::Percent(100.0));
            if save_videos {
                em.item(&filename, "Download complete");
            } else {
                em.item(&filename, "Download complete - Processing audio...");
            }
        }
        "error" => {
            em.item(&filename, "Download failed - see log for details");
            em.send(Event::Error(format!("Failed while downloading '{filename}'")));
        }
        _ => {}
    }
}