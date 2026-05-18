# grod

Cast YouTube and [Piped](https://github.com/TeamPiped/Piped) videos to any Chromecast device from the command line.

## Features

- Cast videos by YouTube URL, Piped URL, or video ID
- Queue management with auto-advance
- Background daemon watches device and plays next in queue
- Interactive TUI with playback controls
- Resolves streams via a self-hosted Piped API instance

## Dependencies

- [go-chromecast](https://github.com/vishen/go-chromecast) — must be in `$PATH`
- A self-hosted [Piped](https://github.com/TeamPiped/Piped) instance (API backend)

## Installation

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

### Queue management

```bash
grod queue <url>       # always add to queue
grod list              # show queue with titles
grod remove <pos>      # remove entry at position
grod clear             # clear entire queue
```

### Playback controls

```bash
grod play/pause        # toggle play/pause  (aliases: pp, p, pause, play, toggle, t)
grod skip              # stop current, play next in queue  (alias: sk)
grod forward [secs]    # seek forward (default 10s)  (alias: f)
grod back [secs]       # seek backward (default 10s)  (alias: b)
grod volume-up         # (alias: vu)
grod volume-down       # (alias: vd)
grod mute              # (alias: m)
grod unmute            # (alias: um)
```

### Status

```bash
grod status
```

### Background daemon

The daemon polls the device every 10 seconds and automatically casts the next queued video when the device goes idle.

```bash
grod daemon            # start (runs in foreground, use & or a service)
grod stop-daemon       # stop
```

### TUI

Interactive queue manager with live now-playing status:

```bash
grod tui
```

| Key | Action |
|-----|--------|
| `space` | Pause / play |
| `s` | Skip current |
| `d` / `Del` | Remove selected from queue |
| `→` / `l` | Seek forward 10s |
| `←` / `h` | Seek backward 10s |
| `+` / `-` | Volume up / down |
| `m` | Toggle mute |
| `j` / `k` | Navigate queue |
| `q` / `Esc` | Quit |

## Configuration

Config is stored at `~/.local/share/grod/config.toml`:

```toml
piped_api = "https://your-piped-instance.example.com"
device_addr = "192.168.1.100"
device_port = 8009
```

## License

MIT
