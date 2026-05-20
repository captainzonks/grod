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
- Local HLS muxing for 1080p casting (uses your video + audio streams via Piped, muxes locally with ffmpeg)
- HTTP API on the LAN with optional PIN auth — for mobile remotes or other clients
- mDNS service discovery — LAN clients auto-find the daemon (no manual IP entry)
- Resolves streams via a self-hosted Piped API instance

## Dependencies

- [go-chromecast](https://github.com/vishen/go-chromecast) — must be in `$PATH`
- A self-hosted [Piped](https://github.com/TeamPiped/Piped) instance (API backend)
- `ffmpeg` — only required if you use the daemon's HLS muxer for 1080p casting (one-shot CLI casts without the daemon don't need it)

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

### Configuration commands

```bash
grod config show                       # show current config
grod config set-api <url>              # set Piped API base URL
grod config set-device <addr> [port]   # set device address (default port: 8009)
grod config discover                   # discover Chromecast devices on LAN
grod config set-pin <pin>              # set API PIN (empty string disables auth)
grod config set-quality <quality>      # default cast quality: best | 1080p | 720p | 480p | 360p
```

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
grod queue <url>       # always add to queue        (alias: q)
grod list              # show queue with titles      (alias: l)
grod remove <pos>      # remove entry at position    (alias: rm)
grod clear             # clear entire queue          (alias: cl)
grod status            # show now playing + queue    (alias: s)
```

### Playback controls

```bash
grod play/pause        # toggle play/pause            (aliases: pp, p, pause, play, toggle, t)
grod skip              # stop current, play next       (alias: sk)
grod forward [secs]    # seek forward (default 10s)    (alias: f)
grod back [secs]       # seek backward (default 10s)   (alias: b)
grod volume-up         # increase volume               (alias: vu)
grod volume-down       # decrease volume               (alias: vd)
grod mute              # mute device                   (alias: m)
grod unmute            # unmute device                 (alias: um)
```

### Background daemon

The daemon polls the device every 10 seconds and automatically casts the next queued video when the device goes idle. It also runs the HTTP API (port `7878` by default) and the local muxing stream server (port `7879` by default) used for high-quality casting.

```bash
grod daemon              # start (runs in foreground, use & or systemd)  (alias: d)
grod daemon start        # explicit form
grod daemon stop         # stop running daemon
grod daemon status       # pid, ports, live now-playing + queue
```

#### systemd user service

To have the daemon start automatically on login and restart on failure, install the user-mode service:

```bash
# 1. Copy the unit into your user systemd directory
mkdir -p ~/.config/systemd/user
cp contrib/grod.service ~/.config/systemd/user/

# 2. Enable + start it (no sudo needed — runs as your user)
systemctl --user daemon-reload
systemctl --user enable --now grod.service

# 3. Optional: keep running across logout (otherwise it stops with your session)
sudo loginctl enable-linger "$USER"
```

Useful follow-up commands:

```bash
systemctl --user status grod.service     # is it up?
journalctl --user -u grod.service -f     # live log
systemctl --user restart grod.service    # bounce it
systemctl --user disable --now grod.service   # uninstall
```

The unit looks for `grod` on `$PATH` first, then `~/.local/bin/grod`. If you installed it elsewhere, override with `systemctl --user edit grod.service` and set a custom `ExecStart=`.

### HTTP API + remote control

While the daemon is running it exposes an HTTP API for LAN clients (e.g. a phone remote). All control endpoints (`/cast`, `/skip`, `/play-pause`, `/volume-up`, `/queue`, ...) are POST, plus `GET /status` and `GET /search?q=...`. With a PIN configured, requests must include the `X-Grod-Pin: <pin>` header. The daemon also advertises itself on the LAN via mDNS as `_grod._tcp.local.` so clients can auto-discover it.

```bash
grod config set-pin 1234       # require this PIN on every request (omit to disable)
curl -X POST http://<your-laptop-lan-ip>:7878/cast \
     -H 'Content-Type: application/json' \
     -H 'X-Grod-Pin: 1234' \
     -d '{"url":"https://www.youtube.com/watch?v=dQw4w9WgXcQ"}'
```

### Firewall

The daemon needs `7878/tcp` (API) and `7879/tcp` (stream server) open for LAN traffic. If LAN clients can't reach the daemon, run:

```bash
grod firewall
```

It prints LAN-scoped `allow` commands for ufw, firewalld, nftables, and iptables based on the firewall tools it finds on `$PATH`. Pick the one matching your setup and run it as root. Examples are scoped to your detected LAN subnet (so the ports aren't exposed to the WAN); a broader fallback is shown as a comment in case you need it.

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
device_addr = "192.168.1.100"     # your Chromecast's LAN address — yours will differ
device_port = 8009
api_port = 7878                   # HTTP API port (used by mobile remotes)
stream_port = 7879                # local muxing stream server port (Chromecast pulls from here)
api_pin = ""                      # optional PIN; empty disables auth
default_quality = "1080p"         # best | 1080p | 720p | 480p | 360p
```

The example addresses (`192.168.1.100`, `<your-laptop-lan-ip>`, etc.) are placeholders. Use your own LAN addresses; `grod config discover` and `grod firewall` print real values for your network.

## License

MIT
