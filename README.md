<div align="center">

<img src="assets/grod-logo-01.webp" alt="grod logo" width="280"/>

# grod

**Cast YouTube and [Piped](https://github.com/TeamPiped/Piped) videos to any Chromecast device from the command line.**

[![crates.io](https://img.shields.io/crates/v/grod.svg?logo=rust)](https://crates.io/crates/grod)
[![docs.rs](https://docs.rs/grod/badge.svg)](https://docs.rs/grod)
[![GitHub release](https://img.shields.io/github/v/release/captainzonks/grod?logo=github)](https://github.com/captainzonks/grod/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust)](https://www.rust-lang.org/)

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/E1E21U3S1R)

> Single-binary Chromecast remote: one-shot casting, auto-advancing queue, TUI, LAN HTTP API, and 1080p HLS muxing — all wrapped around a self-hosted [Piped](https://github.com/TeamPiped/Piped) instance.

</div>

---

## Overview

`grod` ("Google Fishing Rod") is a Rust CLI for casting YouTube and Piped videos to any Chromecast device — Chromecast Ultra, Nvidia Shield TV, Google TV, anything that speaks the Chromecast media protocol. It pairs with a self-hosted Piped instance to deliver ad-free playback, runs as a background daemon for queue auto-advance, exposes an HTTP API for companion apps, and ships a [ratatui](https://github.com/ratatui-org/ratatui) TUI for interactive control.

### Why grod?

- **Ad-free YouTube** via your own Piped instance — no SponsorBlock workarounds, no rotating CDN tokens.
- **Terminal-first** — single static binary, no Electron, no browser, scriptable end-to-end.
- **High-quality casting** — local ffmpeg HLS muxer pulls separate video-only mp4 + audio-only m4a streams from Piped and re-muxes them in flight, so 1080p+ casts work on devices that don't accept Piped's adaptive streams directly (e.g. Nvidia Shield).
- **Multi-frontend** — same daemon serves the CLI, the TUI, and the `grod_remote` Flutter companion app over a documented LAN HTTP API.

---

## Key Features

### Casting

- Cast by YouTube URL, Piped URL, or raw 11-char video ID
- Local HLS muxer for high-resolution casting (libx264 with `repeat-headers`, forced IDR per segment for Chromecast decoder compatibility)
- Hardware-accelerated encoding via **VAAPI** (AMD/Intel iGPU), **NVENC** (NVIDIA), or **QSV** (Intel) — auto-detected, with libx264 CPU fallback
- Configurable quality preference: `best | 1080p | 720p | 480p | 360p`
- Automatic fallback to muxed mp4 (~360p) when a target quality is unavailable
- Original-audio-track preference (skips auto-dubs on multi-language videos)
- HTTP reconnect flags weather googlevideo CDN drops mid-stream

### Queue & Daemon

- Background daemon polls the device every 10s and auto-advances the queue on idle
- Persistent JSON queue at `~/.local/share/grod/queue.json`
- `grod daemon start | stop | status` with PID file tracking and graceful SIGTERM shutdown (kills the ffmpeg child, cleans the HLS tempdir)
- Optional systemd user-mode service in [`contrib/grod.service`](contrib/grod.service)

### LAN HTTP API + Discovery

- axum-based HTTP API on configurable port (default `7878`)
- Optional `X-Grod-Pin` header authentication
- Endpoints: `/status`, `/cast`, `/queue`, `/quality`, `/play-pause`, `/skip`, `/volume-up`, `/volume-down`, `/search`, and more
- mDNS service advertisement (`_grod._tcp.local.`) for zero-config LAN discovery
- `grod firewall` subcommand prints LAN-scoped allow rules for `ufw`, `firewalld`, `nftables`, and `iptables`

### Interactive TUI

- Live now-playing status with elapsed/remaining timestamps
- Queue navigation, playback controls, volume, mute
- Dismissible error popup overlay with full anyhow trace

---

## Architecture

```text
┌──────────────┐    HTTP+PIN     ┌────────────────────────────────┐
│  grod CLI    │ ──────────────▶ │           grod daemon          │
│  grod TUI    │    mDNS         │  ┌──────────────────────────┐  │
│  Flutter app │ ◀────────────── │  │  axum HTTP API (7878)    │  │
└──────────────┘                 │  │  queue + poll loop       │  │
                                 │  │  HLS muxer  (7879)       │  │
                                 │  └──────────────────────────┘  │
                                 └──────────┬──────┬──────────────┘
                                            │      │
                                  HTTPS GET │      │ go-chromecast
                                  /streams  │      │ (Cast protocol)
                                            ▼      ▼
                                     ┌──────────┐  ┌──────────────┐
                                     │  Piped   │  │  Chromecast  │
                                     │   API    │  │    device    │
                                     └──────────┘  └──────────────┘
```

The daemon resolves stream URLs from Piped, picks the best video-only + audio-only stream pair for the configured quality, spawns `ffmpeg` to re-mux them into HLS on the stream port, and hands the resulting `master.m3u8` URL to `go-chromecast` for playback on the target device.

---

## Dependencies

- **[go-chromecast](https://github.com/vishen/go-chromecast)** — must be on `$PATH`. Handles the Cast protocol.
- **A self-hosted [Piped](https://github.com/TeamPiped/Piped) instance** — `grod` does not embed a YouTube resolver; ad-free playback is Piped's job.
- **`ffmpeg`** — only required when using the daemon's HLS muxer (i.e. anything above 360p). One-shot CLI casts at the muxed-mp4 fallback don't need it.

---

## Installation

### Cargo (recommended)

```bash
cargo install grod
```

### Pre-built binary (Linux / macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/captainzonks/grod/main/install.sh | sh
```

Installs to `~/.local/bin/grod`. Override with `INSTALL_DIR`:

```bash
INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/captainzonks/grod/main/install.sh | sh
```

### From source

```bash
git clone https://github.com/captainzonks/grod
cd grod
cargo install --path .
```

---

## Quick Start

```bash
# 1. Discover Chromecasts on your network
grod config discover

# 2. Point grod at your Piped instance
grod config set-api https://piped.example.com

# 3. Optional: default to 1080p casts (requires ffmpeg + daemon)
grod config set-quality 1080p

# 4. Cast a video
grod cast "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
```

For queue + auto-advance + the HTTP API, start the daemon:

```bash
grod daemon            # foreground; use systemd or & for backgrounding
grod queue <url>       # queue a video
grod daemon status     # check PID, ports, live now-playing
```

---

## Configuration

Config is stored at `~/.local/share/grod/config.toml`:

```toml
piped_api       = "https://your-piped-instance.example.com"
device_addr     = "192.168.1.100"  # your Chromecast's LAN address — yours will differ
device_port     = 8009
api_port        = 7878             # HTTP API port (used by mobile remotes)
stream_port     = 7879             # local HLS muxer port (Chromecast pulls from here)
api_pin         = ""               # optional PIN; empty disables auth
default_quality = "1080p"          # best | 1080p | 720p | 480p | 360p
encoder         = "auto"           # auto | cpu | vaapi | nvenc | qsv
```

The example addresses above are placeholders — `grod config discover` and `grod firewall` print real values for your network.

### Configuration commands

```bash
grod config show                       # show current config (auto-resolved encoder shown as "auto → vaapi")
grod config set-api <url>              # set Piped API base URL
grod config set-device <addr> [port]   # set device address (default port: 8009)
grod config discover                   # discover Chromecast devices on LAN
grod config set-pin <pin>              # set API PIN (empty string disables auth)
grod config set-quality <quality>      # default cast quality
grod config set-encoder <encoder>      # auto | cpu | vaapi | nvenc | qsv
```

### Hardware encoding

The HLS muxer can transcode video on the GPU instead of libx264 on the CPU. On a laptop iGPU (tested AMD Ryzen 7 5700U / Lucienne) VAAPI sustains **12×+ realtime at 1080p High@4.0 with ~150% CPU** — vs. libx264 `ultrafast` at ~2× realtime, Constrained Baseline only.

| Encoder  | When to use                                    | Profile        | ffmpeg encoder |
| -------- | ---------------------------------------------- | -------------- | -------------- |
| `auto`   | Default — probes for the best available HW    | varies         | varies         |
| `vaapi`  | AMD or Intel iGPU on Linux                     | High@4.0       | `h264_vaapi`   |
| `nvenc`  | NVIDIA GPU                                     | High@4.0       | `h264_nvenc`   |
| `qsv`    | Intel CPU with Quick Sync Video                | High@4.0       | `h264_qsv`     |
| `cpu`    | Forced libx264 fallback (no GPU available)     | Constrained Baseline@4.0 | `libx264 -preset ultrafast` |

Auto-detection probes `/dev/dri/renderD128` (VAAPI/QSV) and `/dev/nvidia*` (NVENC), plus checks `ffmpeg -encoders` for the relevant encoder symbol. Auto preference order: NVENC > QSV > VAAPI > CPU. Restart the daemon after `set-encoder` for it to take effect.

```bash
grod config set-encoder auto      # let grod pick best HW backend
grod config set-encoder vaapi     # force VAAPI even if NVENC available
grod config show                  # prints "Encoder : auto → vaapi" when resolved
```

> **Note:** the `CODECS=` hint in the master playlist is profile-aware — HW encoders emit `avc1.640028` (High@4.0), CPU emits `avc1.42e028` (Constrained Baseline@4.0). Mismatching the hint causes Chromecast to reject the LOAD silently.

---

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

The daemon polls the device every 10 seconds and automatically casts the next queued video when the device goes idle. It also runs the HTTP API (port `7878` by default) and the local muxing stream server (port `7879` by default).

```bash
grod daemon              # start (foreground; use & or systemd)  (alias: d)
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
systemctl --user status grod.service          # is it up?
journalctl --user -u grod.service -f          # live log
systemctl --user restart grod.service         # bounce it
systemctl --user disable --now grod.service   # uninstall
```

The unit looks for `grod` on `$PATH` first, then `~/.local/bin/grod`. Override the binary path via `systemctl --user edit grod.service` if you installed it elsewhere.

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

It prints LAN-scoped `allow` commands for `ufw`, `firewalld`, `nftables`, and `iptables` based on the firewall tools it finds on `$PATH`. Pick the one matching your setup and run it as root. Examples are scoped to your detected LAN subnet (so the ports aren't exposed to the WAN); a broader fallback is shown as a comment in case you need it.

### TUI

Interactive queue manager with live now-playing status:

```bash
grod tui
```

| Key           | Action                       |
| ------------- | ---------------------------- |
| `space`       | Play / pause                 |
| `s`           | Skip current                 |
| `d` / `Del`   | Remove selected from queue   |
| `→` / `l`     | Seek forward 10s             |
| `←` / `h`     | Seek backward 10s            |
| `+` / `-`     | Volume up / down             |
| `m`           | Toggle mute                  |
| `j` / `k`     | Navigate queue               |
| `q` / `Esc`   | Quit                         |

Errors surface as a dismissible popup overlay with the full anyhow chain. Press any key to close.

---

## Companion App

A Flutter remote app for Android (`grod_remote`) is in development. It speaks the same HTTP API + mDNS surface as the CLI, so the daemon doesn't care which client is driving it. Release planned alongside the public Android TV port.

---

## Roadmap

- **`grod_tv`** — Android TV port using Media3 ExoPlayer's `MergingMediaSource`, eliminating the need for ffmpeg-side HLS muxing. Design doc lives in the `grod_tv` workspace.
- Search-results pagination + filters in the TUI
- Per-device quality profiles (Shield 1080p, Ultra 4K, etc.)

---

## Acknowledgments

<div align="center">

<img src="assets/grod-mascot-01.webp" alt="grod mascot" width="160"/>

</div>

- **[Piped](https://github.com/TeamPiped/Piped)** — for delivering ad-free YouTube playback that this entire project is built around.
- **[go-chromecast](https://github.com/vishen/go-chromecast)** — for the Cast protocol heavy lifting.
- **[ratatui](https://github.com/ratatui-org/ratatui)** + **[axum](https://github.com/tokio-rs/axum)** + **[ffmpeg](https://ffmpeg.org/)** — the Rust + media plumbing that makes the whole thing tick.

---

## Support

If `grod` saves you from a single YouTube ad, consider buying me a coffee:

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/E1E21U3S1R)

---

## License

[MIT](LICENSE) © captainzonks
