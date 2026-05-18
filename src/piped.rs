use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StreamsResponse {
    pub title: String,
    #[serde(rename = "videoStreams")]
    pub video_streams: Vec<VideoStream>,
}

#[derive(Debug, Deserialize)]
pub struct VideoStream {
    pub url: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "videoOnly", default)]
    pub video_only: bool,
    #[allow(dead_code)]
    pub quality: Option<String>,
}

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

    pub async fn resolve(&self, video_id: &str) -> Result<ResolvedVideo> {
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

        let stream_url = pick_stream(&resp.video_streams)
            .context("no suitable stream found")?;

        Ok(ResolvedVideo {
            id: video_id.to_string(),
            title: resp.title,
            stream_url,
        })
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
    pub stream_url: String,
}

fn pick_stream(streams: &[VideoStream]) -> Option<String> {
    // Prefer HLS — muxed, adaptive, Chromecast handles natively
    for s in streams {
        if s.mime_type == "application/x-mpegurl" {
            return Some(s.url.clone());
        }
    }
    // Fallback: muxed mp4
    for s in streams {
        if !s.video_only && s.mime_type.contains("mp4") {
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
        assert_eq!(extract_video_id("not-a-video"), None);
    }
}
