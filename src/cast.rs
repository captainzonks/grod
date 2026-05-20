//! Chromecast control — wraps `go-chromecast` for load, stop, and playback commands.
use anyhow::{bail, Context, Result};
use std::process::Command;

#[derive(Clone)]
pub struct Caster {
    pub addr: String,
    pub port: u16,
}

impl Caster {
    pub fn new(addr: impl Into<String>, port: u16) -> Self {
        Self {
            addr: addr.into(),
            port,
        }
    }

    /// Cast a stream URL to the device. Returns immediately (detached).
    /// `content_type` overrides Chromecast's autodetection (e.g. `video/mp2t` for MPEG-TS).
    /// Pass `None` to let go-chromecast guess.
    pub fn load(&self, stream_url: &str, content_type: Option<&str>) -> Result<()> {
        let port_str = self.port.to_string();
        let mut args: Vec<&str> = vec![
            "load",
            stream_url,
            "--addr",
            &self.addr,
            "--port",
            &port_str,
            "--detach",
        ];
        if let Some(ct) = content_type {
            args.push("-c");
            args.push(ct);
        }
        let status = Command::new("go-chromecast")
            .args(&args)
            .status()
            .context("failed to run go-chromecast — is it installed?")?;

        if !status.success() {
            bail!("go-chromecast exited with status {status}");
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.gc(&["stop"])
    }

    pub fn toggle_pause(&self) -> Result<()> {
        self.gc(&["togglepause"])
    }

    pub fn mute(&self) -> Result<()> {
        self.gc(&["mute"])
    }

    pub fn unmute(&self) -> Result<()> {
        self.gc(&["unmute"])
    }

    pub fn volume_up(&self) -> Result<()> {
        self.gc(&["volume-up"])
    }

    pub fn volume_down(&self) -> Result<()> {
        self.gc(&["volume-down"])
    }

    pub fn seek_forward(&self, seconds: u32) -> Result<()> {
        self.gc(&["seek", &seconds.to_string()])
    }

    pub fn seek_back(&self, seconds: u32) -> Result<()> {
        self.gc(&["rewind", &seconds.to_string()])
    }

    fn gc(&self, args: &[&str]) -> Result<()> {
        let status = Command::new("go-chromecast")
            .args(args)
            .args(["--addr", &self.addr, "--port", &self.port.to_string()])
            .status()
            .with_context(|| format!("go-chromecast {} failed", args[0]))?;
        if !status.success() {
            bail!("go-chromecast {} exited with {status}", args[0]);
        }
        Ok(())
    }

    /// Returns raw status string from go-chromecast.
    pub fn status_raw(&self) -> Result<String> {
        let out = Command::new("go-chromecast")
            .args([
                "status",
                "--addr",
                &self.addr,
                "--port",
                &self.port.to_string(),
            ])
            .output()
            .context("go-chromecast status failed")?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// True if device is occupied with media — PLAYING, PAUSED, or BUFFERING.
    /// BUFFERING must count as occupied so the daemon's poll loop doesn't clear
    /// `now_playing` while Chromecast is still loading a freshly issued stream.
    pub fn is_playing(&self) -> bool {
        self.status_raw()
            .map(|s| {
                s.contains("PLAYING") || s.contains("PAUSED") || s.contains("BUFFERING")
            })
            .unwrap_or(false)
    }

    /// Parse time remaining in seconds from status output.
    /// Format: "time remaining=Xs/Ys" (where Ys may be "-1s" for live/HLS streams —
    /// remaining is still valid in that case).
    pub fn time_remaining(&self) -> Option<u64> {
        let raw = self.status_raw().ok()?;
        let marker = "time remaining=";
        let pos = raw.find(marker)?;
        let after = &raw[pos + marker.len()..];
        let field: String = after.chars().take_while(|&c| c != ',').collect();
        let left = field.split('/').next()?.trim_end_matches('s');
        left.parse().ok()
    }

    /// Returns `(position_secs, duration_secs)` for the active cast.
    /// The Chromecast reports `time remaining=Xs/Ys` where Y is the total
    /// duration. Position is computed as `Y - X`, clamped to [0, Y].
    /// Returns None if no media is playing, or if the receiver reports a
    /// duration of -1 (live streams and our event-type HLS playlists
    /// produce this — there's no progress to show).
    pub fn position_duration(&self) -> Option<(u64, u64)> {
        let (remaining, duration) = self.parse_time_field()?;
        let pos = duration.saturating_sub(remaining);
        Some((pos, duration))
    }

    /// Parses `time remaining=<remaining>s/<duration>s` into `(remaining, duration)`.
    /// Returns None when the duration is unknown (e.g. live streams report -1).
    fn parse_time_field(&self) -> Option<(u64, u64)> {
        let raw = self.status_raw().ok()?;
        let marker = "time remaining=";
        let pos = raw.find(marker)?;
        let after = &raw[pos + marker.len()..];
        // Pull up to the comma — "Xs/Ys"
        let field: String = after.chars().take_while(|&c| c != ',').collect();
        let (left, right) = field.split_once('/')?;
        let remaining: u64 = left.trim_end_matches('s').parse().ok()?;
        // Duration "-1s" → unknown; bail.
        let right = right.trim_end_matches('s');
        if right.starts_with('-') {
            return None;
        }
        let duration: u64 = right.parse().ok()?;
        Some((remaining, duration))
    }
}
