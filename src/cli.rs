use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "toss", about = "Cast YouTube/Piped videos to Chromecast devices")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Cast a video; queue if device is busy
    Cast {
        /// YouTube URL, Piped URL, or video ID
        url: String,
        /// Always queue, never interrupt current video
        #[arg(short = 'q', long)]
        queue: bool,
    },
    /// Add a video to the queue without casting immediately
    Queue {
        /// YouTube URL, Piped URL, or video ID
        url: String,
    },
    /// Skip current video and play next in queue
    Skip,
    /// Pause playback
    Pause,
    /// Resume playback
    Play,
    /// Toggle pause/play
    Toggle,
    /// Mute device
    Mute,
    /// Unmute device
    Unmute,
    /// Increase volume
    VolumeUp,
    /// Decrease volume
    VolumeDown,
    /// Seek forward by seconds (default 10)
    Forward {
        #[arg(default_value = "10")]
        seconds: u32,
    },
    /// Seek backward by seconds (default 10)
    Back {
        #[arg(default_value = "10")]
        seconds: u32,
    },
    /// List queued videos
    List,
    /// Remove a video from the queue by position
    Remove {
        /// 1-based position in queue
        position: usize,
    },
    /// Clear the entire queue
    Clear,
    /// Show current device status and now playing
    Status,
    /// Start the background queue daemon
    Daemon,
    /// Stop the background queue daemon
    StopDaemon,
    /// Interactive TUI queue manager
    Tui,
    /// Configure tosser (Piped API URL, device address)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
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
}
