//! # grod — Google Fishing Rod
//!
//! Cast YouTube and [Piped](https://github.com/TeamPiped/Piped) videos to any
//! Chromecast device from the command line.
//!
//! ## Features
//!
//! - Cast by YouTube URL, Piped URL, or video ID
//! - Queue with auto-advance via background daemon ([`daemon`])
//! - Interactive TUI queue manager with error popup overlay ([`tui`])
//! - Full playback controls: pause, seek, volume, mute
//! - Stream resolution via self-hosted Piped API ([`piped`])
//! - Local HLS muxer for 1080p casting (ffmpeg pulls video-only mp4 +
//!   audio-only m4a from Piped, transcodes with libx264, serves over HTTP — [`streamer`])
//! - LAN HTTP API with optional PIN auth for companion apps ([`api`])
//! - mDNS service advertisement (`_grod._tcp.local.`) for LAN auto-discovery ([`discovery`])
//! - `grod firewall` subcommand prints LAN-scoped allow rules for
//!   ufw/firewalld/nftables/iptables
//!
//! ## Quick start
//!
//! ```text
//! grod config discover          # find devices on LAN
//! grod config set-api <url>     # set Piped API base URL
//! grod cast <youtube-url>       # cast immediately or queue  (alias: c)
//! grod play/pause               # toggle play/pause          (alias: pp)
//! grod status                   # now playing + queue        (alias: s)
//! grod tui                      # open interactive TUI
//! grod daemon [start|stop|status]  # manage background daemon            (alias: d)
//! grod firewall                 # print LAN allow rules for required ports
//! ```

pub mod api;
pub mod cast;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod discovery;
pub mod piped;
pub mod queue;
pub mod streamer;
pub mod tui;
