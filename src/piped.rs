//! Piped API client — resolves YouTube video IDs to stream URLs and titles.
use crate::config::Quality;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub uploader: String,
    pub duration: i64,
    pub thumbnail: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum SearchItem {
    Stream {
        #[serde(default)]
        url: String,
        #[serde(default)]
        title: String,
        #[serde(rename = "uploaderName", default)]
        uploader_name: String,
        #[serde(default)]
        duration: i64,
        #[serde(default)]
        thumbnail: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct StreamsResponse {
    pub title: String,
    #[serde(rename = "videoStreams")]
    pub video_streams: Vec<VideoStream>,
    #[serde(rename = "audioStreams", default)]
    pub audio_streams: Vec<AudioStream>,
    /// Total video length in seconds. Used to feed the progress bar in
    /// clients when the Chromecast can't infer duration from the stream
    /// itself (event-type HLS playlists report -1 for duration).
    #[serde(default)]
    pub duration: u64,
}

#[derive(Debug, Deserialize)]
pub struct VideoStream {
    pub url: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "videoOnly", default)]
    pub video_only: bool,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AudioStream {
    pub url: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(default)]
    pub bitrate: u64,
    #[serde(default)]
    pub format: Option<String>,
    /// "ORIGINAL", "DUBBED", "DESCRIPTIVE", etc. None on single-track videos.
    /// We prefer ORIGINAL over dubbed tracks for videos with YouTube's
    /// multi-language audio — without this filter, Dimension 20 episodes
    /// (for example) come back as German dubs because they appear first in
    /// Piped's audioStreams list.
    #[serde(default, rename = "audioTrackType")]
    pub track_type: Option<String>,
    /// e.g. "en-US", "de-DE". For diagnostic logging only — track_type does
    /// the real selection work.
    #[serde(default, rename = "audioTrackLocale")]
    pub track_locale: Option<String>,
}

#[derive(Clone)]
pub struct PipedClient {
    base_url: String,
    client: reqwest::Client,
}

impl PipedClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Resolve at requested quality. Returns a muxed pair (video_url + audio_url)
    /// when a suitable video-only mp4 + audio-only m4a are available; falls back
    /// to a single muxed `stream_url` (low quality) otherwise.
    pub async fn resolve(&self, video_id: &str, quality: Quality) -> Result<ResolvedVideo> {
        let url = format!("{}/streams/{}", self.base_url, video_id);
        let resp: StreamsResponse = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .context("Piped API returned error status")?
            .json()
            .await
            .context("parsing Piped API response")?;

        // Try high-quality video-only + audio-only pair (for ffmpeg muxing)
        let pair = pick_streams_for_quality(&resp.video_streams, &resp.audio_streams, quality);

        // Fallback single muxed URL (used when muxing not possible or as last resort)
        let fallback = pick_muxed_fallback(&resp.video_streams);

        if pair.is_none() && fallback.is_none() {
            anyhow::bail!("no suitable stream found");
        }

        Ok(ResolvedVideo {
            id: video_id.to_string(),
            title: resp.title,
            video_url: pair.as_ref().map(|p| p.0.clone()),
            audio_url: pair.as_ref().map(|p| p.1.clone()),
            quality_label: pair.as_ref().map(|p| p.2.clone()).unwrap_or_else(|| "360p".to_string()),
            stream_url: fallback,
            duration_secs: resp.duration,
        })
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let url = format!("{}/search", self.base_url);
        let resp: SearchResponse = self
            .client
            .get(&url)
            .query(&[("q", query), ("filter", "videos")])
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .context("Piped search API returned error status")?
            .json()
            .await
            .context("parsing Piped search response")?;

        let results = resp
            .items
            .into_iter()
            .filter_map(|item| match item {
                SearchItem::Stream { url, title, uploader_name, duration, thumbnail } => {
                    Some(SearchResult { url, title, uploader: uploader_name, duration, thumbnail })
                }
                SearchItem::Other => None,
            })
            .collect();

        Ok(results)
    }

    pub async fn title(&self, video_id: &str) -> Result<String> {
        let url = format!("{}/streams/{}", self.base_url, video_id);
        let resp: StreamsResponse = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .context("Piped API returned error status")?
            .json()
            .await
            .context("parsing Piped API response")?;
        Ok(resp.title)
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedVideo {
    pub id: String,
    pub title: String,
    /// Video-only mp4 stream URL (for ffmpeg muxing). Present when muxing pair found.
    pub video_url: Option<String>,
    /// Audio-only m4a stream URL (for ffmpeg muxing). Present when muxing pair found.
    pub audio_url: Option<String>,
    /// Actual quality label resolved (e.g. "1080p"). Falls back to "360p" if muxing unavailable.
    pub quality_label: String,
    /// Single muxed stream URL used as fallback when muxing not possible. Low quality (≤360p on YouTube).
    pub stream_url: Option<String>,
    /// Total video duration in seconds, straight from Piped.
    pub duration_secs: u64,
}

/// Pick (video_url, audio_url, quality_label) for muxing.
/// Strategy: best mp4 video-only stream ≤ target height, plus highest-bitrate m4a audio-only.
fn pick_streams_for_quality(
    videos: &[VideoStream],
    audios: &[AudioStream],
    quality: Quality,
) -> Option<(String, String, String)> {
    let target = quality.target_height();

    // Find best video-only mp4 ≤ target height
    let video = videos
        .iter()
        .filter(|s| {
            s.video_only
                && s.height > 0
                && s.height <= target
                && s.mime_type == "video/mp4"
                && s.format.as_deref() == Some("MPEG_4")
        })
        .max_by_key(|s| s.height)?;

    // Find highest-bitrate audio-only m4a, preferring the original-language
    // track when YouTube exposes multiple (e.g. Dimension 20 episodes carry
    // German/Spanish/etc. dubs alongside English). Order of preference:
    //   1. track_type == "ORIGINAL"
    //   2. track_type missing (single-track videos report no track_type)
    //   3. anything else (dubs, descriptions)
    // Tie-break within each bucket by bitrate, descending.
    let m4a: Vec<&AudioStream> = audios
        .iter()
        .filter(|a| a.mime_type == "audio/mp4" && a.format.as_deref() == Some("M4A"))
        .collect();
    let audio = m4a
        .iter()
        .copied()
        .max_by_key(|a| {
            let tier: u8 = match a.track_type.as_deref() {
                Some("ORIGINAL") => 2,
                None => 1,
                _ => 0,
            };
            (tier, a.bitrate)
        })?;

    let label = video
        .quality
        .clone()
        .unwrap_or_else(|| format!("{}p", video.height));

    Some((video.url.clone(), audio.url.clone(), label))
}

/// Fallback: a single muxed stream URL (low-res, no ffmpeg needed).
fn pick_muxed_fallback(streams: &[VideoStream]) -> Option<String> {
    // Prefer muxed mp4 (itag 18, ~360p)
    for s in streams {
        if !s.video_only && s.mime_type == "video/mp4" {
            return Some(s.url.clone());
        }
    }
    // Then HLS
    for s in streams {
        if s.mime_type == "application/x-mpegurl" {
            return Some(s.url.clone());
        }
    }
    None
}

/// Extract YouTube video ID from various URL forms or raw ID.
pub fn extract_video_id(input: &str) -> Option<String> {
    let input = input.trim();

    // Raw 11-char ID
    if input.len() == 11 && input.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Some(input.to_string());
    }

    // ?v= or &v= param
    if let Some(pos) = input.find("v=") {
        let after = &input[pos + 2..];
        let id: String = after.chars().take(11).collect();
        if id.len() == 11 {
            return Some(id);
        }
    }

    // youtu.be/<id>
    if let Some(pos) = input.find("youtu.be/") {
        let after = &input[pos + 9..];
        let id: String = after.chars().take(11).collect();
        if id.len() == 11 {
            return Some(id);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_video_id() {
        assert_eq!(extract_video_id("dQw4w9WgXcQ"), Some("dQw4w9WgXcQ".into()));
        assert_eq!(
            extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            extract_video_id("https://piped.example.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        // Anything that is not 11 chars of [A-Za-z0-9_-] and is not embedded
        // in a recognised URL form should fail. "not-a-video" *is* 11 chars
        // of the allowed charset, so it deliberately matches the raw-ID path
        // — pick inputs that can't collide.
        assert_eq!(extract_video_id("not a video"), None);
        assert_eq!(extract_video_id("too-many-chars-here"), None);
        assert_eq!(extract_video_id(""), None);
    }
}
