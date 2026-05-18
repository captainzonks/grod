//! # grod — Google Fishing Rod
//!
//! Cast YouTube and [Piped](https://github.com/TeamPiped/Piped) videos to any
//! Chromecast device from the command line.
//!
//! ## Features
//!
//! - Cast by YouTube URL, Piped URL, or video ID
//! - Queue with auto-advance via background daemon
//! - Interactive TUI queue manager ([`tui`])
//! - Full playback controls: pause, seek, volume, mute
//! - Stream resolution via self-hosted Piped API ([`piped`])
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
//! ```

pub mod cast;
pub mod cli;
pub mod config;
pub mod daemon;
pub mod piped;
pub mod queue;
pub mod tui;
