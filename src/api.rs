//! HTTP API server — exposes grod controls to local-network clients (e.g. Flutter app).
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use crate::cast::Caster;
use crate::config::{Config, Encoder, Quality};
use crate::daemon::resolve_cast_url;
use crate::piped::{extract_video_id, PipedClient};
use crate::queue::{Queue, QueueEntry};
use crate::streamer::StreamServer;

pub struct AppState {
    pub caster: Caster,
    pub piped: PipedClient,
    pub queue: Queue,
    pub streamer: StreamServer,
    pub public_host: String,
    /// Default cast quality, mutable at runtime via `POST /quality`.
    /// Shared with the daemon poll loop so changes apply to the next cast
    /// without restarting the daemon.
    pub default_quality: Arc<Mutex<Quality>>,
    /// Quality label of the most recently cast video. Updated on every
    /// successful cast (both HLS-mux and fallback paths) so /status can
    /// show it even when no streamer session is active (e.g. fallback cast).
    pub last_cast_quality: Arc<Mutex<String>>,
    /// Resolved encoder backend (`auto` already collapsed to concrete value
    /// at daemon start). Used for every HLS muxer session.
    pub encoder: Encoder,
    /// Timestamp of the most recent confirmed activity (poll saw the device
    /// playing, OR a fresh cast was just dispatched). The poll loop uses
    /// this to decide whether the device is in a transient idle window
    /// (recent activity) or genuinely idle (long enough to clear state).
    pub last_active: Arc<Mutex<Instant>>,
    pub pin: String,
}

type SharedState = Arc<AppState>;

async fn pin_auth(State(s): State<SharedState>, req: Request, next: Next) -> impl IntoResponse {
    if s.pin.is_empty() {
        return next.run(req).await.into_response();
    }
    let provided = req
        .headers()
        .get("X-Grod-Pin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided != s.pin {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "invalid PIN"}))).into_response();
    }
    next.run(req).await.into_response()
}

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/cast", post(cast))
        .route("/queue", post(queue_add))
        .route("/queue/{pos}", delete(queue_remove))
        .route("/queue", delete(queue_clear))
        .route("/skip", post(skip))
        .route("/play-pause", post(play_pause))
        .route("/volume-up", post(volume_up))
        .route("/volume-down", post(volume_down))
        .route("/volume", post(set_volume))
        .route("/mute", post(mute))
        .route("/unmute", post(unmute))
        .route("/forward", post(forward))
        .route("/back", post(back))
        .route("/search", get(search))
        .route("/quality", post(set_quality))
        .layer(middleware::from_fn_with_state(state.clone(), pin_auth))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// --- response types ---

#[derive(Serialize)]
struct StatusResponse {
    state: &'static str,
    now_playing: Option<QueueEntry>,
    queue: Vec<QueueEntryWithPos>,
    daemon: bool,
    /// Current cast quality (e.g. "1080p", "720p"). Reflects the active session
    /// when one is set, otherwise the configured default.
    quality: String,
    /// Playback position in seconds. Null when no media is playing or when
    /// duration is unknown (live streams, our event-type HLS playlists).
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<u64>,
    /// Total media duration in seconds. Null in the same cases as `position`.
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u64>,
    /// Current device volume in `[0.0, 1.0]`. Null when the device is
    /// unreachable. Lets clients render an absolute volume slider.
    #[serde(skip_serializing_if = "Option::is_none")]
    volume: Option<f64>,
    /// Whether the device is muted. Null when unreachable.
    #[serde(skip_serializing_if = "Option::is_none")]
    muted: Option<bool>,
}

#[derive(Serialize)]
struct QueueEntryWithPos {
    pos: usize,
    id: String,
    title: String,
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct QueuedResponse {
    pos: usize,
    title: String,
}

#[derive(Deserialize)]
struct UrlBody {
    url: String,
    /// When true, stop the current cast (if any) and play this URL immediately
    /// instead of queueing it. Defaults to false so the queue-on-busy behavior
    /// is preserved for clients that don't know about this field.
    #[serde(default)]
    force: bool,
}

#[derive(Deserialize)]
struct SeekBody {
    seconds: Option<u32>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Deserialize)]
struct QualityBody {
    quality: String,
}

#[derive(Deserialize)]
struct VolumeBody {
    /// Absolute volume level in `[0.0, 1.0]`. Clamped in `Caster::set_volume`.
    level: f64,
}

fn ok() -> Json<OkResponse> {
    Json(OkResponse { ok: true })
}

fn err(msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

// --- handlers ---

async fn status(State(s): State<SharedState>) -> impl IntoResponse {
    let raw = s.caster.status_raw().unwrap_or_default();
    let state = if raw.contains("PLAYING") {
        "playing"
    } else if raw.contains("PAUSED") {
        "paused"
    } else if raw.contains("BUFFERING") {
        "buffering"
    } else {
        "idle"
    };

    let now_playing = s.queue.now_playing().unwrap_or(None);
    let entries = s.queue.load().unwrap_or_default();
    let queue = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| QueueEntryWithPos { pos: i + 1, id: e.id, title: e.title })
        .collect();

    let session = s.streamer.current().await;
    let quality = match &session {
        Some(sess) => sess.quality_label.clone(),
        None => {
            let last = s.last_cast_quality.lock().unwrap().clone();
            if last.is_empty() {
                s.default_quality.lock().unwrap().label().to_string()
            } else {
                last
            }
        }
    };

    // Position + duration:
    //   Chromecast's `time remaining=X/Y` field is misleadingly named:
    //   X actually counts UP from cast start (elapsed), not down (remaining).
    //   Verified on both HLS muxer casts and mp4 fallback casts on Nvidia
    //   Shield. Y is unreliable for muxer casts (often -1 or matches X)
    //   but real for mp4 fallback. So always trust X as elapsed and prefer
    //   Piped's Session duration as the source of truth for Y, falling
    //   back to Chromecast's Y only when no Session exists (mp4 fallback).
    let session_duration =
        session.as_ref().map(|s| s.duration_secs).filter(|&d| d > 0);
    let elapsed = s.caster.time_remaining();
    let chromecast_dur = s.caster.position_duration().map(|(_, d)| d);
    let duration = session_duration.or(chromecast_dur);
    let position = match (elapsed, duration) {
        (Some(e), Some(d)) => Some(e.min(d)),
        (Some(e), None) => Some(e),
        _ => None,
    };

    // Volume + muted come from the same status line already fetched into
    // `raw` — no extra go-chromecast invocation.
    let volume = crate::cast::Caster::parse_volume(&raw);
    let muted = crate::cast::Caster::parse_muted(&raw);

    Json(StatusResponse {
        state,
        now_playing,
        queue,
        daemon: crate::daemon::is_running(),
        quality,
        position,
        duration,
        volume,
        muted,
    })
}

async fn cast(State(s): State<SharedState>, Json(body): Json<UrlBody>) -> impl IntoResponse {
    let id = match extract_video_id(&body.url) {
        Some(id) => id,
        None => return err("could not extract video ID").into_response(),
    };

    let occupied = s.caster.is_playing();

    // Busy + no force flag → queue it. With force=true, fall through and
    // interrupt the current cast.
    if occupied && !body.force {
        match s.piped.title(&id).await {
            Err(e) => return err(e).into_response(),
            Ok(title) => match s.queue.push(QueueEntry { id, title: title.clone() }) {
                Err(e) => return err(e).into_response(),
                Ok(pos) => return Json(serde_json::json!({ "queued": true, "pos": pos, "title": title })).into_response(),
            },
        }
    }

    // force=true and something is playing — stop it first so the new cast
    // doesn't race the old one. Errors here are non-fatal: the new load
    // will overwrite the receiver state anyway.
    if occupied && body.force {
        let _ = s.caster.stop();
        s.streamer.clear().await;
    }

    let q = *s.default_quality.lock().unwrap();
    match s.piped.resolve(&id, q).await {
        Err(e) => err(e).into_response(),
        Ok(video) => {
            let (url, ct) = match resolve_cast_url(&s.streamer, &video, q, s.encoder, &s.public_host).await {
                Some(u) => u,
                None => return err("no playable stream URL").into_response(),
            };
            eprintln!("[api] cast url ({ct}): {url}");
            match s.caster.load(&url, Some(ct)) {
                Err(e) => err(e).into_response(),
                Ok(_) => {
                    if let Err(e) = s.queue.set_now_playing(&QueueEntry {
                        id: video.id.clone(),
                        title: video.title.clone(),
                    }) {
                        eprintln!("[api] set_now_playing failed: {e}");
                    }
                    *s.last_cast_quality.lock().unwrap() = video.quality_label.clone();
                    *s.last_active.lock().unwrap() = Instant::now();
                    Json(serde_json::json!({
                        "casting": true,
                        "title": video.title,
                        "quality": video.quality_label,
                    })).into_response()
                }
            }
        }
    }
}

async fn queue_add(State(s): State<SharedState>, Json(body): Json<UrlBody>) -> impl IntoResponse {
    let id = match extract_video_id(&body.url) {
        Some(id) => id,
        None => return err("could not extract video ID").into_response(),
    };
    match s.piped.title(&id).await {
        Err(e) => err(e).into_response(),
        Ok(title) => match s.queue.push(QueueEntry { id, title: title.clone() }) {
            Err(e) => err(e).into_response(),
            Ok(pos) => Json(QueuedResponse { pos, title }).into_response(),
        },
    }
}

async fn queue_remove(State(s): State<SharedState>, Path(pos): Path<usize>) -> impl IntoResponse {
    match s.queue.remove(pos) {
        Ok(e) => Json(serde_json::json!({ "removed": e.title })).into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn queue_clear(State(s): State<SharedState>) -> impl IntoResponse {
    match s.queue.clear() {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn skip(State(s): State<SharedState>) -> impl IntoResponse {
    if let Err(e) = s.caster.stop() {
        return err(e).into_response();
    }
    s.streamer.clear().await;
    ok().into_response()
}

async fn play_pause(State(s): State<SharedState>) -> impl IntoResponse {
    match s.caster.toggle_pause() {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn volume_up(State(s): State<SharedState>) -> impl IntoResponse {
    match s.caster.volume_up() {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn volume_down(State(s): State<SharedState>) -> impl IntoResponse {
    match s.caster.volume_down() {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn set_volume(State(s): State<SharedState>, Json(body): Json<VolumeBody>) -> impl IntoResponse {
    match s.caster.set_volume(body.level) {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn mute(State(s): State<SharedState>) -> impl IntoResponse {
    match s.caster.mute() {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn unmute(State(s): State<SharedState>) -> impl IntoResponse {
    match s.caster.unmute() {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn forward(State(s): State<SharedState>, body: Option<Json<SeekBody>>) -> impl IntoResponse {
    let secs = body.and_then(|b| b.seconds).unwrap_or(10);
    match s.caster.seek_forward(secs) {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn back(State(s): State<SharedState>, body: Option<Json<SeekBody>>) -> impl IntoResponse {
    let secs = body.and_then(|b| b.seconds).unwrap_or(10);
    match s.caster.seek_back(secs) {
        Ok(_) => ok().into_response(),
        Err(e) => err(e).into_response(),
    }
}

async fn search(State(s): State<SharedState>, Query(params): Query<SearchQuery>) -> impl IntoResponse {
    match s.piped.search(&params.q).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => err(e).into_response(),
    }
}

/// Set the default cast quality. Persists to config.toml and applies to the
/// next cast immediately (no daemon restart).
async fn set_quality(State(s): State<SharedState>, Json(body): Json<QualityBody>) -> impl IntoResponse {
    let q = match Quality::parse(&body.quality) {
        Some(q) => q,
        None => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("invalid quality '{}': use best|1080p|720p|480p|360p", body.quality)})),
        ).into_response(),
    };

    // Persist to config so daemon restarts pick it up.
    match Config::load() {
        Ok(mut cfg) => {
            cfg.default_quality = q;
            if let Err(e) = cfg.save() {
                return err(format!("failed to save config: {e}")).into_response();
            }
        }
        Err(e) => return err(format!("failed to load config: {e}")).into_response(),
    }

    *s.default_quality.lock().unwrap() = q;
    Json(serde_json::json!({"quality": q.label()})).into_response()
}
