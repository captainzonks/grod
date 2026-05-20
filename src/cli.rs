use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "grod", about = "Cast YouTube/Piped videos to Chromecast devices")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Cast a video; queue if device is busy
    #[command(alias = "c")]
    Cast {
        /// YouTube URL, Piped URL, or video ID
        url: String,
        /// Always queue, never interrupt current video
        #[arg(short = 'q', long)]
        queue: bool,
    },
    /// Add a video to the queue without casting immediately
    #[command(alias = "q")]
    Queue {
        /// YouTube URL, Piped URL, or video ID
        url: String,
    },
    /// Skip current video and play next in queue
    #[command(alias = "sk")]
    Skip,
    /// Toggle play/pause
    #[command(name = "play/pause", alias = "pp", alias = "p", alias = "pause", alias = "play", alias = "t", alias = "toggle")]
    PlayPause,
    /// Mute device
    #[command(alias = "m")]
    Mute,
    /// Unmute device
    #[command(alias = "um")]
    Unmute,
    /// Increase volume
    #[command(alias = "vu")]
    VolumeUp,
    /// Decrease volume
    #[command(alias = "vd")]
    VolumeDown,
    /// Seek forward by seconds (default 10)
    #[command(alias = "f")]
    Forward {
        #[arg(default_value = "10")]
        seconds: u32,
    },
    /// Seek backward by seconds (default 10)
    #[command(alias = "b")]
    Back {
        #[arg(default_value = "10")]
        seconds: u32,
    },
    /// List queued videos
    #[command(alias = "l")]
    List,
    /// Remove a video from the queue by position
    #[command(alias = "rm")]
    Remove {
        /// 1-based position in queue
        position: usize,
    },
    /// Clear the entire queue
    #[command(alias = "cl")]
    Clear,
    /// Show current device status and now playing
    #[command(alias = "s")]
    Status,
    /// Manage the background queue daemon (start | stop | status)
    #[command(alias = "d")]
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },
    /// Interactive TUI queue manager
    Tui,
    /// Configure grod (Piped API URL, device address)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Print firewall commands to allow the API + stream ports on the LAN
    Firewall,
}

#[derive(Subcommand, Debug, Default)]
pub enum DaemonAction {
    /// Start the daemon (foreground; use systemd or `&` for backgrounding)
    #[default]
    Start,
    /// Stop the running daemon
    Stop,
    /// Show daemon status (running, ports, live now-playing if reachable)
    Status,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current config
    Show,
    /// Set Piped API base URL
    SetApi {
        url: String,
    },
    /// Set Chromecast device address
    SetDevice {
        addr: String,
        #[arg(default_value = "8009")]
        port: u16,
    },
    /// Discover Chromecast devices on the network
    Discover,
    /// Set API PIN (empty string to disable)
    SetPin {
        pin: String,
    },
    /// Set default cast quality (best | 1080p | 720p | 480p | 360p)
    SetQuality {
        quality: String,
    },
}
