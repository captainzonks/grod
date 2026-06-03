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

    /// Set the device volume to an absolute level in `[0.0, 1.0]`.
    /// go-chromecast accepts a float arg to `volume`; values outside the
    /// range are clamped here so a malformed client request can't push the
    /// receiver into undefined behavior.
    pub fn set_volume(&self, level: f64) -> Result<()> {
        let clamped = level.clamp(0.0, 1.0);
        // Two decimals matches go-chromecast's own display granularity and
        // avoids sending noise like 0.4700000001.
        self.gc(&["volume", &format!("{clamped:.2}")])
    }

    /// Current device volume in `[0.0, 1.0]`, parsed from the status line
    /// (`"...volume=0.47 muted=false"`). None when the device is unreachable
    /// or the field is absent.
    pub fn volume(&self) -> Option<f64> {
        Self::parse_volume(&self.status_raw().ok()?)
    }

    /// True if the device is muted, parsed from the status line
    /// (`"...muted=false"`). None when unreachable or the field is absent.
    pub fn muted(&self) -> Option<bool> {
        Self::parse_muted(&self.status_raw().ok()?)
    }

    /// Pull the `volume=<float>` field out of a go-chromecast status line.
    /// Split out so it can be unit-tested without a live device and reused by
    /// the API `/status` handler, which already holds a `status_raw()` string.
    pub(crate) fn parse_volume(raw: &str) -> Option<f64> {
        let marker = "volume=";
        let pos = raw.find(marker)?;
        let after = &raw[pos + marker.len()..];
        let field: String = after
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ',')
            .collect();
        field.parse().ok()
    }

    /// Pull the `muted=<bool>` field out of a go-chromecast status line.
    pub(crate) fn parse_muted(raw: &str) -> Option<bool> {
        let marker = "muted=";
        let pos = raw.find(marker)?;
        let after = &raw[pos + marker.len()..];
        let field: String = after
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ',')
            .collect();
        field.parse().ok()
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

#[cfg(test)]
mod tests {
    use super::Caster;

    #[test]
    fn parses_volume_from_idle_status() {
        assert_eq!(
            Caster::parse_volume("Idle, volume=0.47 muted=false"),
            Some(0.47)
        );
    }

    #[test]
    fn parses_volume_when_followed_by_comma() {
        assert_eq!(
            Caster::parse_volume("volume=1.00, something else"),
            Some(1.0)
        );
    }

    #[test]
    fn volume_absent_returns_none() {
        assert_eq!(Caster::parse_volume("PLAYING, muted=false"), None);
    }

    #[test]
    fn parses_muted_flag() {
        assert_eq!(
            Caster::parse_muted("Idle, volume=0.47 muted=true"),
            Some(true)
        );
        assert_eq!(
            Caster::parse_muted("Idle, volume=0.47 muted=false"),
            Some(false)
        );
    }
}
