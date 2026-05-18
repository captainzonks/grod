# grod

[![crates.io](https://img.shields.io/crates/v/grod.svg)](https://crates.io/crates/grod)
[![docs.rs](https://docs.rs/grod/badge.svg)](https://docs.rs/grod)

**grod** (Google Fishing Rod) — cast YouTube and [Piped](https://github.com/TeamPiped/Piped) videos to any Chromecast device from the command line.

## Features

- Cast by YouTube URL, Piped URL, or video ID
- Queue management with auto-advance
- Background daemon watches device and plays next in queue
- Interactive TUI with live now-playing status and playback controls
- Full playback controls: play/pause, seek, volume, mute
- Resolves streams via a self-hosted Piped API instance

## Dependencies

- [go-chromecast](https://github.com/vishen/go-chromecast) — must be in `$PATH`
- A self-hosted [Piped](https://github.com/TeamPiped/Piped) instance (API backend)

## Installation

### cargo (recommended)

```bash
cargo install grod
```

### Binary (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/captainzonks/grod/main/install.sh | sh
```

Installs to `~/.local/bin/grod`. Set `INSTALL_DIR` to override:

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/captainzonks/grod/main/install.sh | sh
```

### From source

```bash
git clone https://github.com/captainzonks/grod
cd grod
cargo install --path .
```

## Setup

Discover devices on your network and select one:

```bash
grod config discover
```

Set your Piped API URL:

```bash
grod config set-api https://your-piped-instance.example.com
```

Verify:

```bash
grod config show
```

## Usage

### Cast a video

```bash
grod cast "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
grod cast dQw4w9WgXcQ
```

If the device is busy, the video is queued automatically. Use `-q` to always queue:

```bash
grod cast -q "https://youtu.be/dQw4w9WgXcQ"
```

Alias: `c`

### Queue management

```bash
grod queue <url>       # always add to queue           (alias: q)
grod list              # show queue with titles         (alias: l)
grod remove <pos>      # remove entry at position       (alias: rm)
grod clear             # clear entire queue             (alias: cl)
grod status            # show now playing + queue       (alias: s)
```

### Playback controls

```bash
grod play/pause        # toggle play/pause  (aliases: pp, p, pause, play, toggle, t)
grod skip              # stop current, play next        (alias: sk)
grod forward [secs]    # seek forward (default 10s)     (alias: f)
grod back [secs]       # seek backward (default 10s)    (alias: b)
grod volume-up                                          (alias: vu)
grod volume-down                                        (alias: vd)
grod mute                                               (alias: m)
grod unmute                                             (alias: um)
```

### Background daemon

The daemon polls the device every 10 seconds and automatically casts the next queued video when the device goes idle.

```bash
grod daemon            # start (runs in foreground, use & or a service)  (alias: d)
grod stop-daemon       # stop                                             (alias: sd)
```

### TUI

Interactive queue manager with live now-playing status:

```bash
grod tui
```

| Key | Action |
|-----|--------|
| `space` | Play / pause |
| `s` | Skip current |
| `d` / `Del` | Remove selected from queue |
| `→` / `l` | Seek forward 10s |
| `←` / `h` | Seek backward 10s |
| `+` / `-` | Volume up / down |
| `m` | Toggle mute |
| `j` / `k` | Navigate queue |
| `q` / `Esc` | Quit |

Errors are shown as a dismissible popup overlay with full trace information. Press any key to close.

## Configuration

Config is stored at `~/.local/share/grod/config.toml`:

```toml
piped_api = "https://your-piped-instance.example.com"
device_addr = "192.168.1.100"
device_port = 8009
```

## License

MIT
