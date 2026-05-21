//! Local muxing HTTP server (HLS).
//!
//! Resolves Piped video-only + audio-only stream pairs via ffmpeg (`-c copy`)
//! into HLS — an .m3u8 playlist + .ts segments — served over HTTP on a fixed
//! port the Chromecast can pull from.
//!
//! Why HLS:
//! - Chromecast's default media receiver app accepts HLS reliably; fragmented
//!   mp4 and raw MPEG-TS are silently rejected on Nvidia Shield.
//! - ffmpeg can write a sliding-window live HLS playlist (`hls_flags delete_segments+append_list`)
//!   so the on-disk footprint stays bounded.
//!
//! Flow:
//!   1. API/cast layer calls [`StreamServer::set_session`] with (video_url, audio_url).
//!   2. Previous session's ffmpeg + temp dir are torn down.
//!   3. A new temp dir is created and ffmpeg is spawned, writing segments into it.
//!   4. The Chromecast loads `http://<bind>:<port>/stream/<token>/playlist.m3u8`.
//!   5. Chromecast fetches the playlist + segment files in turn.

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use crate::config::{Encoder, Quality};
use rand::Rng;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::File;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;

const MASTER_FILENAME: &str = "master.m3u8";
const PLAYLIST_FILENAME: &str = "playlist.m3u8";
const SEGMENT_DURATION_SECS: u32 = 4;
#[allow(dead_code)]
const PLAYLIST_SIZE: u32 = 12;

/// Active muxing session.
#[derive(Debug, Clone)]
pub struct Session {
    pub token: String,
    pub video_url: String,
    pub audio_url: String,
    pub quality_label: String,
    pub work_dir: PathBuf,
    /// Total media duration in seconds. Used by the status endpoint to feed
    /// the client's progress bar — the muxed HLS playlist itself reports
    /// duration=-1 to Chromecast, so we have to surface it from Piped.
    pub duration_secs: u64,
}

#[derive(Clone)]
pub struct StreamServer {
    bind_addr: String,
    port: u16,
    inner: Arc<Inner>,
}

struct Inner {
    session: Mutex<Option<Session>>,
    /// Currently running ffmpeg child (killed on new session).
    active: Mutex<Option<Child>>,
}

impl StreamServer {
    pub fn new(bind_addr: impl Into<String>, port: u16) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            port,
            inner: Arc::new(Inner {
                session: Mutex::new(None),
                active: Mutex::new(None),
            }),
        }
    }

    /// Replace the current session. Kills the previous ffmpeg + tempdir; spawns
    /// a new ffmpeg writing HLS segments. Returns the master playlist URL.
    ///
    /// `quality` is used to emit a CODECS hint in the master playlist —
    /// Chromecast's media receiver uses this to bind a hardware decoder
    /// before any segment fetch, which is required for reliable playback.
    pub async fn set_session(
        &self,
        video_url: String,
        audio_url: String,
        quality_label: String,
        quality: Quality,
        encoder: Encoder,
        duration_secs: u64,
        public_host: &str,
    ) -> Result<String> {
        // Kill previous ffmpeg + clean its tempdir
        if let Some(mut child) = self.inner.active.lock().await.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        if let Some(prev) = self.inner.session.lock().await.take() {
            let _ = std::fs::remove_dir_all(&prev.work_dir);
        }

        let token = random_token();
        let work_dir = std::env::temp_dir().join(format!("grod-stream-{token}"));
        std::fs::create_dir_all(&work_dir)
            .with_context(|| format!("creating stream work dir {}", work_dir.display()))?;

        let playlist_path = work_dir.join(PLAYLIST_FILENAME);
        let master_path = work_dir.join(MASTER_FILENAME);

        // Master playlist: tells Chromecast the codec + resolution up front so it
        // can pick a hardware decoder before fetching the media playlist.
        // Without this, Shield silently rejects the stream.
        let (codecs, resolution, bandwidth) = master_hints(quality, encoder);
        let master_body = format!(
            "#EXTM3U\n\
             #EXT-X-VERSION:6\n\
             #EXT-X-INDEPENDENT-SEGMENTS\n\
             #EXT-X-STREAM-INF:BANDWIDTH={bandwidth},CODECS=\"{codecs}\",RESOLUTION={resolution}\n\
             {PLAYLIST_FILENAME}\n"
        );
        std::fs::write(&master_path, master_body)
            .with_context(|| format!("writing master playlist {}", master_path.display()))?;

        // ffmpeg HLS settings:
        //   -c copy                                 : no transcoding
        //   -hls_time N                             : ~N-second segments
        //   -hls_list_size M                        : keep last M segments in playlist
        //   -hls_flags delete_segments+append_list+independent_segments
        //                                            : sliding window, append safely, each seg standalone
        //   -hls_segment_type mpegts                : .ts segments (Chromecast supports)
        //   -hls_segment_filename ...               : segment naming
        //   -f hls                                  : output format
        let mut cmd = Command::new("ffmpeg");
        // -re on inputs throttles read rate to realtime — keeps ffmpeg in sync with
        // playback so segments don't race ahead and get deleted before Chromecast fetches them.
        // Transcode video + copy audio.
        //
        // Why transcode video (not -c copy):
        //   Piped's mp4 video segments only carry SPS/PPS in extradata (avcC) — h264
        //   keyframes mid-stream don't repeat them. MPEG-TS HLS segments must be
        //   independently decodable, so each segment's first IDR needs in-band
        //   SPS/PPS. Without them, Chromecast plays a few seconds then aborts with
        //   "non-existing PPS" decoder errors (verified via ffprobe on seg_00002+).
        //
        // libx264 with `repeat-headers=1` emits SPS/PPS before every IDR keyframe,
        // and we force IDRs every SEGMENT_DURATION_SECS so segment boundaries align
        // with keyframes. CPU cost: ~50-70% of one Ryzen core for 1080p veryfast.
        //
        // -c:a copy keeps audio bit-exact (already AAC).
        //
        // Other flags:
        //   -fflags +genpts      : regen missing PTS if any
        //   -copyts -start_at_zero: shared t=0 origin for v+a (no leading discontinuity)
        //
        // NOT using -re (realtime input throttle): we want ffmpeg to run as fast as
        // CPU/network allow so segments accumulate ahead of Chromecast playback.
        // This builds a lead that absorbs googlevideo CDN hiccups (frequent ~3s
        // disconnects) without stalling Shield's playback into BUFFERING.
        // Disk cost: ~95MB for a 3-min 1080p video — trivial.
        let keyint = SEGMENT_DURATION_SECS * 24; // assume ~24fps source — IDR every seg
        let x264_params = format!(
            "keyint={keyint}:min-keyint={keyint}:scenecut=0:repeat-headers=1"
        );
        // HTTP reconnect flags must precede the `-i` they apply to.
        //   -reconnect 1                : reconnect on disconnect
        //   -reconnect_at_eof 1         : also on premature EOF (googlevideo CDN drops)
        //   -reconnect_streamed 1       : reconnect even for streamed (non-seekable) inputs
        //   -reconnect_on_network_error 1: reconnect on net errors (timeout, RST)
        //   -reconnect_on_http_error 4xx,5xx: retry on HTTP errors
        //   -reconnect_delay_max 5      : cap backoff at 5s between retries
        // ffmpeg 8.1 logs "Late SEI is not implemented" on recent YouTube
        // uploads — this is cosmetic, the decoder continues fine. The
        // dominant throughput cost is the libx264 encode itself, so we
        // optimize the encoder (see -preset/profile below) rather than
        // try to strip SEI NALUs pre-decode (a `-bsf:v filter_units`
        // pass *adds* serializing overhead that's worse than the warning).
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("warning")
            .arg("-fflags")
            .arg("+genpts");

        // HW encoder init: must come before the first -i so the device
        // context is available when the encoder is configured downstream.
        // VAAPI/QSV need an explicit hwdevice; NVENC does not.
        match encoder {
            Encoder::Vaapi => {
                cmd.arg("-init_hw_device")
                    .arg("vaapi=va:/dev/dri/renderD128")
                    .arg("-filter_hw_device").arg("va");
            }
            Encoder::Qsv => {
                cmd.arg("-init_hw_device").arg("qsv=qs")
                    .arg("-filter_hw_device").arg("qs");
            }
            _ => {}
        }

        cmd
            // --- input 0: video ---
            .arg("-reconnect").arg("1")
            .arg("-reconnect_at_eof").arg("1")
            .arg("-reconnect_streamed").arg("1")
            .arg("-reconnect_on_network_error").arg("1")
            .arg("-reconnect_on_http_error").arg("4xx,5xx")
            .arg("-reconnect_delay_max").arg("5")
            .arg("-i")
            .arg(&video_url)
            // --- input 1: audio ---
            .arg("-reconnect").arg("1")
            .arg("-reconnect_at_eof").arg("1")
            .arg("-reconnect_streamed").arg("1")
            .arg("-reconnect_on_network_error").arg("1")
            .arg("-reconnect_on_http_error").arg("4xx,5xx")
            .arg("-reconnect_delay_max").arg("5")
            .arg("-i")
            .arg(&audio_url)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("1:a:0");

        // Per-encoder video output args. Each branch must produce H.264 with
        // a profile/level matching the CODECS hint emitted by master_hints().
        match encoder {
            Encoder::Cpu | Encoder::Auto => {
                // ultrafast+baseline: ~2.2x realtime at 1080p on Ryzen U-series.
                // veryfast+High drops to ~0.2x and starves the Chromecast buffer.
                cmd.arg("-c:v").arg("libx264")
                    .arg("-preset").arg("ultrafast")
                    .arg("-profile:v").arg("baseline")
                    .arg("-level:v").arg("4.0")
                    .arg("-threads").arg("0")
                    .arg("-crf").arg("20")
                    .arg("-x264-params").arg(&x264_params);
            }
            Encoder::Vaapi => {
                // VAAPI needs frames uploaded onto a hwaccel surface. nv12
                // is what every AMD/Intel iGPU encoder accepts; format
                // converts the decoded sw frame, hwupload moves it to GPU.
                let gop = SEGMENT_DURATION_SECS * 24;
                cmd.arg("-vf").arg("format=nv12,hwupload")
                    .arg("-c:v").arg("h264_vaapi")
                    .arg("-profile:v").arg("high")
                    .arg("-level").arg("40")
                    .arg("-rc_mode").arg("CQP")
                    .arg("-qp").arg("22")
                    .arg("-g").arg(gop.to_string())
                    .arg("-bf").arg("0");
            }
            Encoder::Nvenc => {
                let gop = SEGMENT_DURATION_SECS * 24;
                cmd.arg("-c:v").arg("h264_nvenc")
                    .arg("-preset").arg("p4")  // p1=fastest, p7=slowest; p4 ≈ medium
                    .arg("-profile:v").arg("high")
                    .arg("-level").arg("4.0")
                    .arg("-rc").arg("vbr")
                    .arg("-cq").arg("22")
                    .arg("-g").arg(gop.to_string())
                    .arg("-bf").arg("0");
            }
            Encoder::Qsv => {
                let gop = SEGMENT_DURATION_SECS * 24;
                cmd.arg("-vf").arg("format=nv12,hwupload=extra_hw_frames=64")
                    .arg("-c:v").arg("h264_qsv")
                    .arg("-preset").arg("veryfast")
                    .arg("-profile:v").arg("high")
                    .arg("-level").arg("40")
                    .arg("-g").arg(gop.to_string())
                    .arg("-bf").arg("0");
            }
        }

        cmd.arg("-c:a").arg("copy")
            .arg("-copyts")
            .arg("-start_at_zero")
            .arg("-muxdelay")
            .arg("0")
            .arg("-muxpreload")
            .arg("0")
            .arg("-hls_time")
            .arg(SEGMENT_DURATION_SECS.to_string())
            .arg("-hls_list_size")
            .arg("0") // keep all segments
            .arg("-hls_flags")
            .arg("independent_segments")
            .arg("-hls_segment_type")
            .arg("mpegts")
            .arg("-hls_playlist_type")
            .arg("event")
            .arg("-hls_segment_filename")
            .arg(work_dir.join("seg_%05d.ts").to_string_lossy().to_string())
            .arg("-f")
            .arg("hls")
            .arg(playlist_path.to_string_lossy().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .context("failed to spawn ffmpeg — is ffmpeg installed?")?;

        let session = Session {
            token: token.clone(),
            video_url,
            audio_url,
            quality_label,
            work_dir,
            duration_secs,
        };

        *self.inner.session.lock().await = Some(session);
        *self.inner.active.lock().await = Some(child);

        // Spawn a reaper: wait for ffmpeg exit and clean its slot.
        let inner_clone = self.inner.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let mut guard = inner_clone.active.lock().await;
                let exited = match guard.as_mut() {
                    Some(c) => matches!(c.try_wait(), Ok(Some(_))),
                    None => true,
                };
                if exited {
                    if let Some(mut c) = guard.take() {
                        let _ = c.wait().await;
                    }
                    break;
                }
            }
        });

        Ok(format!(
            "http://{}:{}/stream/{}/{}",
            public_host, self.port, token, MASTER_FILENAME
        ))
    }

    /// Get the current session (for status reporting).
    pub async fn current(&self) -> Option<Session> {
        self.inner.session.lock().await.clone()
    }

    /// Clear session, kill ffmpeg, remove tempdir.
    pub async fn clear(&self) {
        if let Some(mut child) = self.inner.active.lock().await.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        if let Some(prev) = self.inner.session.lock().await.take() {
            let _ = std::fs::remove_dir_all(&prev.work_dir);
        }
    }

    pub async fn run(self) -> Result<()> {
        let app = Router::new()
            .route("/stream/{token}/{file}", get(file_handler))
            .with_state(self.inner.clone());

        let bind = format!("{}:{}", self.bind_addr, self.port);
        let listener = tokio::net::TcpListener::bind(&bind)
            .await
            .with_context(|| format!("binding stream server on {bind}"))?;
        eprintln!("Stream server listening on {bind}");
        axum::serve(listener, app)
            .await
            .context("stream server failed")?;
        Ok(())
    }
}

async fn file_handler(
    Path((token, file)): Path<(String, String)>,
    State(inner): State<Arc<Inner>>,
) -> Response {
    // Path traversal guard
    if file.contains('/') || file.contains("..") {
        return (StatusCode::BAD_REQUEST, "invalid filename").into_response();
    }

    let session = {
        let guard = inner.session.lock().await;
        match guard.as_ref() {
            Some(s) if s.token == token => s.clone(),
            _ => return (StatusCode::NOT_FOUND, "no session for token").into_response(),
        }
    };

    let path = session.work_dir.join(&file);

    // Playlist may not exist yet (ffmpeg still warming up) — poll briefly.
    // For the media playlist (`playlist.m3u8`), also wait until at least 3 segments
    // are on disk so Chromecast doesn't immediately hit a "future" segment that 404s.
    // The master playlist is static and never requires segment readiness.
    let is_media_playlist = file == PLAYLIST_FILENAME;
    let mut tries = 0;
    let file_ready = loop {
        let exists = path.exists();
        if exists && !is_media_playlist {
            break true;
        }
        if exists && is_media_playlist {
            let seg_count = std::fs::read_dir(&session.work_dir)
                .map(|d| d.filter_map(|e| e.ok()).filter(|e| {
                    e.file_name().to_string_lossy().ends_with(".ts")
                }).count())
                .unwrap_or(0);
            if seg_count >= 3 {
                break true;
            }
        }
        if tries >= 50 {
            break false;
        }
        tries += 1;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };

    if !file_ready {
        return (StatusCode::NOT_FOUND, "file not ready").into_response();
    }

    let f = match File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("open {}: {e}", path.display()),
            )
                .into_response();
        }
    };

    let content_type = if file.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if file.ends_with(".ts") {
        "video/mp2t"
    } else {
        "application/octet-stream"
    };

    let stream = ReaderStream::new(f);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());

    (StatusCode::OK, headers, body).into_response()
}

/// CODECS string + resolution + bandwidth hint per target quality + encoder.
///
/// avc1 codec strings follow ISO/IEC 14496-15: `avc1.PPCCLL` where PP=profile,
/// CC=constraint flags, LL=level. AAC-LC is mp4a.40.2.
/// Bandwidth is a rough VBR ceiling — Chromecast uses it for buffer sizing, not
/// gating, so over-estimating slightly is safe.
///
/// CODECS hint MUST match the actual stream profile or Chromecast rejects
/// LOAD. CPU path uses Constrained Baseline for speed; HW paths produce
/// High profile at no CPU cost so we advertise it accordingly.
fn master_hints(q: Quality, encoder: Encoder) -> (&'static str, &'static str, u32) {
    let hw = !matches!(encoder, Encoder::Cpu | Encoder::Auto);
    // (codecs, resolution, bandwidth_bps)
    match (q, hw) {
        // --- HW encoders: High profile ---
        (Quality::Best | Quality::P1080, true) => ("avc1.640028,mp4a.40.2", "1920x1080", 6_000_000),
        (Quality::P720, true) => ("avc1.64001f,mp4a.40.2", "1280x720", 3_000_000),
        (Quality::P480, true) => ("avc1.64001e,mp4a.40.2", "854x480", 1_500_000),
        (Quality::P360, true) => ("avc1.64001e,mp4a.40.2", "640x360", 800_000),
        // --- CPU encoder: Constrained Baseline ---
        (Quality::Best | Quality::P1080, false) => ("avc1.42e028,mp4a.40.2", "1920x1080", 6_000_000),
        (Quality::P720, false) => ("avc1.42e01f,mp4a.40.2", "1280x720", 3_000_000),
        (Quality::P480, false) => ("avc1.42e01e,mp4a.40.2", "854x480", 1_500_000),
        (Quality::P360, false) => ("avc1.42e01e,mp4a.40.2", "640x360", 800_000),
    }
}

/// Resolve `Encoder::Auto` to a concrete backend by probing `ffmpeg -encoders`
/// and the DRI render node. Non-Auto values pass through unchanged so the user
/// can force a backend even if probing would have rejected it (useful for
/// debugging or non-standard installs).
///
/// Order of preference for Auto: NVENC > VAAPI > QSV > CPU.
/// NVENC is fastest when present; VAAPI covers most Linux iGPUs; QSV is
/// Intel-specific. CPU is the universal fallback.
pub fn resolve_encoder(requested: Encoder) -> Encoder {
    if !matches!(requested, Encoder::Auto) {
        return requested;
    }
    let encoders = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let has_enc = |name: &str| encoders.contains(name);
    let has_dri = std::path::Path::new("/dev/dri/renderD128").exists();
    // NVENC requires actual NVIDIA hardware. ffmpeg ships the encoder
    // unconditionally, so check for the kernel module's device node.
    let has_nvidia = std::path::Path::new("/dev/nvidia0").exists()
        || std::path::Path::new("/dev/nvidiactl").exists();
    // QSV requires Intel iGPU. AMD's /dev/dri/renderD128 exists too, so
    // distinguish via vendor: only enable QSV if i915 driver is loaded.
    let has_intel_gpu = std::fs::read_to_string("/sys/class/drm/card0/device/vendor")
        .map(|v| v.trim() == "0x8086")
        .unwrap_or(false);

    if has_enc("h264_nvenc") && has_nvidia {
        Encoder::Nvenc
    } else if has_enc("h264_qsv") && has_dri && has_intel_gpu {
        Encoder::Qsv
    } else if has_enc("h264_vaapi") && has_dri {
        // VAAPI is the universal Linux iGPU path — works on AMD, Intel
        // (with i965/iHD driver), and even some NVIDIA via nvidia-vaapi.
        Encoder::Vaapi
    } else {
        Encoder::Cpu
    }
}

fn random_token() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 12];
    rng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
