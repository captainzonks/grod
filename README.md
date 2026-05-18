# tosser

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
curl -fsSL https://raw.githubusercontent.com/OWNER/tosser/main/install.sh | sh
```

Installs to `~/.local/bin/toss`. Set `INSTALL_DIR` to override:

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/OWNER/tosser/main/install.sh | sh
```

### From source

```bash
git clone https://github.com/OWNER/tosser
cd tosser
cargo install --path .
```

## Setup

Discover devices on your network and select one:

```bash
toss config discover
```

Set your Piped API URL:

```bash
toss config set-api https://your-piped-instance.example.com
```

Verify:

```bash
toss config show
```

## Usage

### Cast a video

```bash
toss cast "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
toss cast dQw4w9WgXcQ
```

If the device is busy, the video is queued automatically. Use `-q` to always queue:

```bash
toss cast -q "https://youtu.be/dQw4w9WgXcQ"
```

### Queue management

```bash
toss queue <url>       # always add to queue
toss list              # show queue with titles
toss remove <pos>      # remove entry at position
toss clear             # clear entire queue
```

### Playback controls

```bash
toss pause
toss play
toss toggle            # pause/play toggle
toss skip              # stop current, play next in queue
toss forward [secs]    # seek forward (default 10s)
toss back [secs]       # seek backward (default 10s)
toss volume-up
toss volume-down
toss mute
toss unmute
```

### Status

```bash
toss status
```

### Background daemon

The daemon polls the device every 10 seconds and automatically casts the next queued video when the device goes idle.

```bash
toss daemon            # start (runs in foreground, use & or a service)
toss stop-daemon       # stop
```

### TUI

Interactive queue manager with live now-playing status:

```bash
toss tui
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

Config is stored at `~/.local/share/tosser/config.toml`:

```toml
piped_api = "https://your-piped-instance.example.com"
device_addr = "192.168.1.100"
device_port = 8009
```

## License

MIT
