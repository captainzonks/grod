//! Persistent queue and now-playing state, stored as JSON in the XDG data dir.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::data_path;

const QUEUE_FILENAME: &str = "queue.json";
const NOW_PLAYING_FILENAME: &str = "now_playing.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: String,
    pub title: String,
}

#[derive(Clone)]
pub struct Queue {
    path: PathBuf,
    now_playing_path: PathBuf,
}

impl Queue {
    pub fn open() -> Result<Self> {
        let dir = data_path()?;
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join(QUEUE_FILENAME),
            now_playing_path: dir.join(NOW_PLAYING_FILENAME),
        })
    }

    pub fn load(&self) -> Result<Vec<QueueEntry>> {
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let raw = std::fs::read_to_string(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        serde_json::from_str(&raw).context("parsing queue.json")
    }

    fn save(&self, entries: &[QueueEntry]) -> Result<()> {
        let raw = serde_json::to_string_pretty(entries)?;
        std::fs::write(&self.path, raw)
            .with_context(|| format!("writing {}", self.path.display()))
    }

    pub fn push(&self, entry: QueueEntry) -> Result<usize> {
        let mut entries = self.load()?;
        entries.push(entry);
        let pos = entries.len();
        self.save(&entries)?;
        Ok(pos)
    }

    /// Remove and return the first entry.
    pub fn pop(&self) -> Result<Option<QueueEntry>> {
        let mut entries = self.load()?;
        if entries.is_empty() {
            return Ok(None);
        }
        let entry = entries.remove(0);
        self.save(&entries)?;
        Ok(Some(entry))
    }

    /// Remove entry at 1-based position. Returns removed entry.
    pub fn remove(&self, pos: usize) -> Result<QueueEntry> {
        let mut entries = self.load()?;
        if pos < 1 || pos > entries.len() {
            anyhow::bail!("position {pos} out of range (queue has {} entries)", entries.len());
        }
        let entry = entries.remove(pos - 1);
        self.save(&entries)?;
        Ok(entry)
    }

    pub fn clear(&self) -> Result<()> {
        self.save(&[])
    }

    pub fn len(&self) -> Result<usize> {
        Ok(self.load()?.len())
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    // --- now playing ---

    pub fn set_now_playing(&self, entry: &QueueEntry) -> Result<()> {
        let raw = serde_json::to_string(entry)?;
        std::fs::write(&self.now_playing_path, raw)?;
        Ok(())
    }

    pub fn clear_now_playing(&self) -> Result<()> {
        if self.now_playing_path.exists() {
            std::fs::remove_file(&self.now_playing_path)?;
        }
        Ok(())
    }

    pub fn now_playing(&self) -> Result<Option<QueueEntry>> {
        if !self.now_playing_path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&self.now_playing_path)?;
        Ok(serde_json::from_str(&raw).ok())
    }
}
