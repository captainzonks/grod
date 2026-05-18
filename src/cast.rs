use anyhow::{bail, Context, Result};
use std::process::Command;

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
    pub fn load(&self, stream_url: &str) -> Result<()> {
        let status = Command::new("go-chromecast")
            .args([
                "load",
                stream_url,
                "--addr",
                &self.addr,
                "--port",
                &self.port.to_string(),
                "--detach",
            ])
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

    pub fn pause(&self) -> Result<()> {
        self.gc(&["pause"])
    }

    pub fn unpause(&self) -> Result<()> {
        self.gc(&["unpause"])
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

    /// True if device is actively playing OR paused (i.e. occupied with media).
    pub fn is_playing(&self) -> bool {
        self.status_raw()
            .map(|s| s.contains("PLAYING") || s.contains("PAUSED"))
            .unwrap_or(false)
    }

    /// Parse time remaining in seconds from status output.
    /// Format: "time remaining=Xs/Ys"
    pub fn time_remaining(&self) -> Option<u64> {
        let raw = self.status_raw().ok()?;
        let marker = "time remaining=";
        let pos = raw.find(marker)?;
        let after = &raw[pos + marker.len()..];
        let remaining_str: String = after.chars().take_while(|&c| c != 's').collect();
        remaining_str.parse().ok()
    }
}
