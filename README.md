# AudioDownloader

A cross-platform GUI application for downloading audio from YouTube videos and playlists, built with Rust and egui. Supports multiple audio formats and qualities, playlist selection, and clipboard monitoring.

## Features

- **YouTube Audio Download**: Download audio from YouTube videos and playlists
- **Multiple Formats**: Support for MP3, AAC, M4A, Opus, Vorbis, WAV, and FLAC
- **Quality Selection**: Configurable audio quality in kbps (e.g., 128, 192, 256)
- **Playlist Support**: Download entire playlists or select specific tracks
- **Clipboard Monitoring**: Automatically detect and download YouTube URLs from clipboard
- **Video Saving**: Option to save both video and audio files
- **Progress Tracking**: Real-time download progress with speed and ETA
- **Cross-Platform**: Works on Windows, macOS, and Linux

## Requirements

- **yt-dlp**: Must be installed and available on your PATH
- **ffmpeg**: Required for audio format conversion (when saving videos)
- **Rust**: Edition 2021 or later
- **Cargo**: Package manager

### Installing Dependencies

**yt-dlp**:
```bash
# Windows
pip install yt-dlp

# macOS
brew install yt-dlp

# Linux
sudo apt install yt-dlp
```

**ffmpeg**:
```bash
# Windows
choco install ffmpeg

# macOS
brew install ffmpeg

# Linux
sudo apt install ffmpeg
```

## Installation

### Clone the repository
```bash
git clone <repository-url>
cd AudiodownLoader
```

### Using Makefile (Recommended)

The project includes a Makefile with convenient targets:

```bash
# Build in release mode
make build

# Run the application
make run

# Clean build artifacts
make clean

# Run tests
make test

# Create macOS .app and .dmg bundles
make dmg

# Install to /Applications (macOS only)
make install

# Show help
make help
```

### Manual Build

If you prefer to use Cargo directly:

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run the application
cargo run
cargo run --release

# Install to ~/.cargo/bin
cargo install --path .
```

## Usage

### Command Line Interface

The application supports command-line arguments:

```bash
# Download audio from a URL
AudioDownloader --url "https://youtube.com/watch?v=..."

# Specify download path
AudioDownloader --download-path "/path/to/downloads"

# Choose audio format and quality
AudioDownloader --format mp3 --quality 256

# Limit number of tracks
AudioDownloader --max-tracks 5

# Save video files as well
AudioDownloader --save-videos
```

### GUI Usage

1. **Enter URL**: Paste or type a YouTube URL in the URL field
2. **Select Format**: Choose from MP3, AAC, M4A, Opus, Vorbis, WAV, or FLAC
3. **Set Quality**: Specify audio quality in kbps (default: 192)
4. **Choose Path**: Select download directory (or use default)
5. **Playlist Selection**: For playlists, select which tracks to download
6. **Download**: Click the Download button to start

### Clipboard Monitoring

Enable clipboard monitoring to automatically detect YouTube URLs copied to your clipboard. The app will show a confirmation dialog before downloading.

## Configuration

The application stores settings in a JSON config file located in the platform config directory:

- **Windows**: `%APPDATA%/ytaudiodownloader/config.json`
- **macOS**: `~/Library/Application Support/ytaudiodownloader/config.json`
- **Linux**: `~/.config/ytaudiodownloader/config.json`

### Configuration Fields

```json
{
  "url": "",
  "download_path": "",
  "audio_format": "mp3",
  "audio_quality": "192",
  "max_tracks": 0,
  "save_videos": true,
  "prefer_h264": true,
  "debug_mode": false,
  "extended_mode": true,
  "clipboard_monitoring": false,
  "autodownload_enabled": false
}
```

### Configuration Options

- `url`: Default YouTube URL
- `download_path`: Default download directory
- `audio_format`: Audio format (mp3, aac, m4a, opus, vorbis, wav, flac)
- `audio_quality`: Audio quality in kbps
- `max_tracks`: Maximum number of tracks to download (0 = unlimited)
- `save_videos`: Save video files in addition to audio
- `prefer_h264`: Prefer H.264 codec for video
- `debug_mode`: Enable debug logging
- `extended_mode`: Enable extended features (playlist selection)
- `clipboard_monitoring`: Monitor clipboard for YouTube URLs
- `autodownload_enabled`: Automatically download detected URLs

## Audio Formats

| Format | Codec | Extension | Supports Bitrate |
|--------|-------|-----------|------------------|
| MP3 | libmp3lame | .mp3 | Yes |
| AAC | aac | .aac | Yes |
| M4A | aac | .m4a | Yes |
| Opus | libopus | .opus | Yes |
| Vorbis | libvorbis | .ogg | Yes |
| WAV | pcm_s16le | .wav | No |
| FLAC | flac | .flac | No |

## How It Works

1. **URL Detection**: The app accepts YouTube URLs from user input or clipboard
2. **Playlist Processing**: For playlists, it fetches video list using yt-dlp
3. **Track Selection**: Users can select specific tracks from playlists
4. **Download**: Uses yt-dlp to download audio/video with progress tracking
5. **Audio Extraction**: If saving videos, uses ffmpeg to extract audio
6. **File Naming**: Files are named with playlist index, title, and video ID

## Troubleshooting

### yt-dlp not found
Ensure yt-dlp is installed and on your PATH:
```bash
which yt-dlp
```

### ffmpeg not found
If you need video saving, ensure ffmpeg is installed:
```bash
which ffmpeg
```

### Download failures
- Check your internet connection
- Verify the YouTube URL is valid and accessible
- Try a different audio format or quality
- Check yt-dlp version and update if needed

## Development

### Project Structure

```
AudiodownLoader/
├── src/
│   ├── main.rs          # Entry point and CLI
│   ├── app.rs           # GUI application logic
│   ├── config.rs        # Configuration management
│   ├── clipboard.rs     # Clipboard monitoring
│   └── downloader.rs    # Download engine
├── Cargo.toml           # Dependencies
├── Makefile             # Build and installation targets
└── README.md
```

### Building from Source

```bash
# Using Makefile (recommended)
make build      # Release build
make run        # Run in debug mode
make test       # Run tests

# Manual Cargo commands
cargo build                    # Debug build
cargo build --release          # Release build
cargo run                      # Run in debug mode
cargo run --release            # Run in release mode
cargo install --path .         # Install to ~/.cargo/bin
```

### macOS Distribution

To create a distributable macOS application:

```bash
make dmg        # Create .app and .dmg bundles
make install    # Install to /Applications
```

This will create:
- `target/release/bundle/osx/AudioDownloader.app`
- `target/release/bundle/dmg/AudioDownloader.dmg`

### Dependencies

- `eframe` - GUI framework
- `egui` - Immediate mode GUI library
- `rfd` - File dialogs
- `arboard` - Clipboard access
- `clap` - Command-line argument parsing
- `serde` & `serde_json` - Configuration serialization
- `directories` - Platform-specific config paths

## License

[Add your license here]

## Contributing

[Add contribution guidelines here]
