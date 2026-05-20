//! Background daemon — polls device every 10s and auto-advances the queue when idle.
//! Also runs the HTTP API server on `api_port` (default 7878) and a muxing stream
//! server on `stream_port` (default 7879) that serves remuxed video+audio to the Chromecast.
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::api::{router, AppState};
use crate::cast::Caster;
use crate::config::{data_path, Quality};
use crate::discovery;
use crate::piped::PipedClient;
use crate::queue::{Queue, QueueEntry};
use crate::streamer::StreamServer;

const PID_FILENAME: &str = "daemon.pid";
const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub fn pid_path() -> Result<PathBuf> {
    Ok(data_path()?.join(PID_FILENAME))
}

pub fn is_running() -> bool {
    let Ok(path) = pid_path() else { return false };
    let Ok(raw) = std::fs::read_to_string(&path) else { return false };
    let Ok(pid) = raw.trim().parse::<u32>() else { return false };
    // Check if process exists by sending signal 0
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub fn write_pid() -> Result<()> {
    let path = pid_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(())
}

pub fn stop() -> Result<()> {
    let path = pid_path()?;
    if !path.exists() {
        println!("Daemon not running");
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let pid: u32 = raw.trim().parse()?;
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    std::fs::remove_file(&path)?;
    println!("Daemon stopped (PID {pid})");
    Ok(())
}

/// `grod daemon status` — local-side status (no Chromecast roundtrip required).
/// Shows pid, configured ports, PIN-protected flag, and if the daemon is
/// reachable on its API port, the live `/status` summary (now-playing + queue).
pub async fn print_status(cfg: &crate::config::Config) -> Result<()> {
    let running = is_running();
    let pid = pid_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<u32>().ok());

    println!(
        "Daemon:      {}",
        if running { "running" } else { "stopped" }
    );
    if let Some(pid) = pid {
        println!("PID:         {pid}");
    }
    println!("API port:    {} (http://127.0.0.1:{}/status)", cfg.api_port, cfg.api_port);
    println!("Stream port: {}", cfg.stream_port);
    println!(
        "PIN auth:    {}",
        if cfg.api_pin.is_empty() { "disabled" } else { "enabled" }
    );
    println!("Quality:     {} (default)", cfg.default_quality.label());

    if !running {
        return Ok(());
    }

    // Try the local API for live state. Short timeout: daemon should answer fast,
    // and we don't want `grod daemon status` to hang if it's wedged.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .context("building reqwest client")?;
    let url = format!("http://127.0.0.1:{}/status", cfg.api_port);
    let mut req = client.get(&url);
    if !cfg.api_pin.is_empty() {
        req = req.header("X-Grod-Pin", &cfg.api_pin);
    }
    match req.send().await {
        Err(e) => {
            println!("\nAPI:         unreachable ({e})");
        }
        Ok(resp) => {
            let status = resp.status();
            match resp.json::<serde_json::Value>().await {
                Err(e) => println!("\nAPI:         {status} (could not parse: {e})"),
                Ok(v) => {
                    println!("\nState:       {}", v.get("state").and_then(|s| s.as_str()).unwrap_or("?"));
                    if let Some(np) = v.get("now_playing").and_then(|n| n.as_object()) {
                        let title = np.get("title").and_then(|t| t.as_str()).unwrap_or("?");
                        println!("Now playing: {title}");
                        if let (Some(p), Some(d)) = (
                            v.get("position").and_then(|p| p.as_u64()),
                            v.get("duration").and_then(|d| d.as_u64()),
                        ) {
                            println!("Position:    {p}s / {d}s");
                        }
                        if let Some(q) = v.get("quality").and_then(|q| q.as_str()) {
                            println!("Cast quality: {q}");
                        }
                    }
                    if let Some(q) = v.get("queue").and_then(|q| q.as_array()) {
                        println!("Queue:       {} video(s) waiting", q.len());
                    }
                }
            }
        }
    }
    Ok(())
}

pub struct DaemonConfig {
    pub piped_api: String,
    pub device_addr: String,
    pub device_port: u16,
    pub api_port: u16,
    pub stream_port: u16,
    pub api_pin: String,
    pub default_quality: Quality,
}

/// Run daemon loop + HTTP API server + stream server in current process.
pub async fn run_loop(cfg: DaemonConfig) -> Result<()> {
    write_pid()?;

    let queue = Queue::open()?;
    let caster = Caster::new(&cfg.device_addr, cfg.device_port);
    let piped = PipedClient::new(&cfg.piped_api);

    // Public host for stream URLs — the Chromecast pulls from this address.
    // Use the configured device subnet's matching local IP (resolved at runtime).
    let public_host = resolve_lan_host(&cfg.device_addr).unwrap_or_else(|| cfg.device_addr.clone());

    let stream_server = StreamServer::new("0.0.0.0", cfg.stream_port);

    let quality = Arc::new(Mutex::new(cfg.default_quality));
    let last_cast_quality = Arc::new(Mutex::new(String::new()));

    let state = Arc::new(AppState {
        caster: caster.clone(),
        piped: piped.clone(),
        queue: queue.clone(),
        streamer: stream_server.clone(),
        public_host: public_host.clone(),
        default_quality: quality.clone(),
        last_cast_quality: last_cast_quality.clone(),
        pin: cfg.api_pin,
    });

    let pin_required = !state.pin.is_empty();
    let app = router(state);
    let bind_addr = format!("0.0.0.0:{}", cfg.api_port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    eprintln!("[daemon] HTTP API listening on {bind_addr}");
    eprintln!("[daemon] Stream server listening on 0.0.0.0:{}", cfg.stream_port);
    eprintln!("[daemon] Stream public host: {public_host}");
    eprintln!("[daemon] Default quality: {}", cfg.default_quality.label());

    // Firewall hint: warn if the required ports look closed.
    check_firewall_hint(cfg.api_port, cfg.stream_port);

    // mDNS service publish — let LAN clients (Flutter app) auto-discover us.
    // Hold the handle so the service stays registered for the daemon's lifetime.
    // Failure is non-fatal — manual config still works.
    let _mdns_handle = match discovery::publish(&public_host, cfg.api_port, pin_required) {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("[daemon] mDNS publish failed (non-fatal): {e}");
            None
        }
    };

    // Keep a clone for explicit cleanup on shutdown — the streamer owns the
    // ffmpeg Child, and dropping a tokio::process::Child during runtime
    // teardown doesn't always reap the subprocess. Explicit `.clear().await`
    // on SIGTERM/Ctrl-C avoids orphan ffmpegs.
    let shutdown_streamer = stream_server.clone();

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { eprintln!("[daemon] HTTP server error: {e}"); }
        }
        res = stream_server.clone().run() => {
            if let Err(e) = res { eprintln!("[daemon] Stream server error: {e}"); }
        }
        _ = poll_loop(caster, piped, queue, stream_server, public_host, quality, last_cast_quality) => {}
        _ = shutdown_signal() => {
            eprintln!("[daemon] shutdown signal received");
        }
    }

    // Best-effort cleanup: kill any active ffmpeg, clean its tempdir.
    shutdown_streamer.clear().await;
    let _ = std::fs::remove_file(pid_path()?);
    eprintln!("[daemon] stopped");

    Ok(())
}

/// Resolves when SIGTERM (systemd, `kill <pid>`) or Ctrl-C arrives.
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let term = async {
        if let Ok(mut s) = signal(SignalKind::terminate()) {
            s.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

async fn poll_loop(
    caster: Caster,
    piped: PipedClient,
    queue: Queue,
    streamer: StreamServer,
    public_host: String,
    quality: Arc<Mutex<Quality>>,
    last_cast_quality: Arc<Mutex<String>>,
) {
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        if caster.is_playing() {
            continue;
        }

        let _ = queue.clear_now_playing();

        match queue.pop() {
            Err(e) => eprintln!("[daemon] queue error: {e}"),
            Ok(None) => continue,
            Ok(Some(entry)) => {
                let q = *quality.lock().unwrap();
                eprintln!("[daemon] casting: {} (quality: {})", entry.title, q.label());
                match piped.resolve(&entry.id, q).await {
                    Err(e) => eprintln!("[daemon] resolve failed for {}: {e}", entry.id),
                    Ok(video) => {
                        let (url, ct) = match resolve_cast_url(&streamer, &video, q, &public_host).await {
                            Some(u) => u,
                            None => {
                                eprintln!("[daemon] no playable URL for {}", entry.id);
                                continue;
                            }
                        };
                        eprintln!("[daemon] cast url ({ct}): {url}");
                        if let Err(e) = caster.load(&url, Some(ct)) {
                            eprintln!("[daemon] cast failed: {e}");
                        } else {
                            let _ = queue.set_now_playing(&QueueEntry {
                                id: entry.id,
                                title: entry.title,
                            });
                            *last_cast_quality.lock().unwrap() = video.quality_label.clone();
                        }
                    }
                }
            }
        }
    }
}

/// Choose between the muxed pair (via streamer) and the low-res fallback URL.
/// Returns (cast_url, content_type) for go-chromecast.
pub async fn resolve_cast_url(
    streamer: &StreamServer,
    video: &crate::piped::ResolvedVideo,
    quality: Quality,
    public_host: &str,
) -> Option<(String, &'static str)> {
    if let (Some(v), Some(a)) = (video.video_url.clone(), video.audio_url.clone()) {
        match streamer
            .set_session(v, a, video.quality_label.clone(), quality, video.duration_secs, public_host)
            .await
        {
            Ok(url) => Some((url, "application/x-mpegurl")),
            Err(e) => {
                eprintln!("[daemon] streamer setup failed: {e}");
                // Fall through to fallback
                let url = video.stream_url.clone()?;
                let ct = if url.contains(".m3u8") || url.contains("/hls/") {
                    "application/x-mpegurl"
                } else {
                    "video/mp4"
                };
                Some((url, ct))
            }
        }
    } else {
        let url = video.stream_url.clone()?;
        let ct = if url.contains(".m3u8") || url.contains("/hls/") {
            "application/x-mpegurl"
        } else {
            "video/mp4"
        };
        Some((url, ct))
    }
}

/// One-line nudge on daemon startup pointing at `grod firewall`. We don't
/// try to *detect* whether the firewall is actually filtering — that's
/// unreliable from userspace (firewall CLIs need root; TCP self-probe goes
/// through loopback shortcut and bypasses inbound rules). Better to leave
/// detection to the user and offer easy access to the canonical commands.
fn check_firewall_hint(api_port: u16, stream_port: u16) {
    eprintln!(
        "[daemon] If LAN clients can't reach the daemon, run `grod firewall` to see \
         commands for opening ports {api_port}/tcp and {stream_port}/tcp."
    );
}

/// Print firewall-tool-specific commands to open the configured API and
/// stream ports. Suggests LAN-scoped rules when a local /24 can be detected
/// (preferred — doesn't expose ports to the WAN), and a broader rule as a
/// fallback for unusual setups.
pub fn print_firewall_commands(api_port: u16, stream_port: u16) {
    let ports = [api_port, stream_port];
    let port_tcp: Vec<String> = ports.iter().map(|p| format!("{p}/tcp")).collect();
    let lan_cidr = detect_lan_cidr();

    println!("grod uses two LAN ports:");
    println!("  - {api_port}/tcp : HTTP API (Flutter app, curl)");
    println!("  - {stream_port}/tcp : stream server (Chromecast pulls video)");
    println!();
    match &lan_cidr {
        Some(cidr) => {
            println!("Detected LAN subnet: {cidr}");
            println!("LAN-scoped rules (safer — block WAN, allow LAN only):");
        }
        None => {
            println!("Could not detect LAN subnet — only broad rules shown.");
            println!("Open rules (allow from anywhere):");
        }
    }
    println!();

    let mut printed_any = false;

    if which("ufw") {
        println!("  ufw:");
        if let Some(cidr) = &lan_cidr {
            for p in &ports {
                println!(
                    "    sudo ufw allow from {cidr} to any port {p} proto tcp"
                );
            }
            println!("    # Fallback (allows from anywhere):");
            println!("    #   sudo ufw allow {}", port_tcp.join(" "));
        } else {
            println!("    sudo ufw allow {}", port_tcp.join(" "));
        }
        println!();
        printed_any = true;
    }
    if which("firewall-cmd") {
        println!("  firewalld:");
        if let Some(cidr) = &lan_cidr {
            // Rich rules for source-restricted access.
            for p in &ports {
                println!(
                    "    sudo firewall-cmd --permanent --zone=public \\\n      \
                     --add-rich-rule='rule family=\"ipv4\" source address=\"{cidr}\" \
                     port protocol=\"tcp\" port=\"{p}\" accept'"
                );
            }
            println!("    sudo firewall-cmd --reload");
            println!("    # Fallback (allows from anywhere):");
            let adds: Vec<String> = ports.iter().map(|p| format!("--add-port={p}/tcp")).collect();
            println!(
                "    #   sudo firewall-cmd --permanent --zone=public {} && sudo firewall-cmd --reload",
                adds.join(" ")
            );
        } else {
            let adds: Vec<String> = ports.iter().map(|p| format!("--add-port={p}/tcp")).collect();
            println!(
                "    sudo firewall-cmd --permanent --zone=public {} && sudo firewall-cmd --reload",
                adds.join(" ")
            );
        }
        println!();
        printed_any = true;
    }
    if which("nft") {
        println!("  nftables (transient — add to your ruleset file to persist):");
        if let Some(cidr) = &lan_cidr {
            for p in &ports {
                println!(
                    "    sudo nft add rule inet filter input ip saddr {cidr} \
                     tcp dport {p} accept"
                );
            }
            println!("    # Fallback (allows from anywhere):");
            for p in &ports {
                println!("    #   sudo nft add rule inet filter input tcp dport {p} accept");
            }
        } else {
            for p in &ports {
                println!("    sudo nft add rule inet filter input tcp dport {p} accept");
            }
        }
        println!();
        printed_any = true;
    }
    if which("iptables") {
        println!("  iptables (transient — use iptables-save to persist):");
        if let Some(cidr) = &lan_cidr {
            for p in &ports {
                println!(
                    "    sudo iptables -I INPUT -p tcp -s {cidr} --dport {p} -j ACCEPT"
                );
            }
            println!("    # Fallback (allows from anywhere):");
            for p in &ports {
                println!("    #   sudo iptables -I INPUT -p tcp --dport {p} -j ACCEPT");
            }
        } else {
            for p in &ports {
                println!("    sudo iptables -I INPUT -p tcp --dport {p} -j ACCEPT");
            }
        }
        println!();
        printed_any = true;
    }

    if !printed_any {
        println!("  No known firewall tool found on PATH (ufw, firewall-cmd, nft, iptables).");
        println!("  Consult your host firewall's documentation to allow inbound TCP {api_port} and {stream_port}.");
    }
}

/// Find this host's LAN subnet as a CIDR string (e.g. "192.168.1.0/24") by
/// parsing the first non-loopback IPv4 from `ip -4 -o addr show`. Returns
/// None if no LAN IP found or the prefix length can't be parsed.
///
/// We compute the network address from the prefix mask rather than just
/// stripping the last octet so that /16, /22, etc. networks also work.
fn detect_lan_cidr() -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .ok()?;
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        let inet_pos = line.find("inet ")?;
        let after = &line[inet_pos + 5..];
        let addr_with_mask = after.split_whitespace().next()?;
        let (addr, mask) = addr_with_mask.split_once('/')?;
        if addr.starts_with("127.") {
            continue;
        }
        let prefix: u8 = mask.parse().ok()?;
        let octets: Vec<u8> = addr.split('.').filter_map(|o| o.parse().ok()).collect();
        if octets.len() != 4 {
            continue;
        }
        let ip_u32 = u32::from_be_bytes([octets[0], octets[1], octets[2], octets[3]]);
        let mask_u32 = if prefix == 0 {
            0u32
        } else {
            u32::MAX << (32 - prefix)
        };
        let net = ip_u32 & mask_u32;
        let b = net.to_be_bytes();
        return Some(format!("{}.{}.{}.{}/{prefix}", b[0], b[1], b[2], b[3]));
    }
    None
}

/// Cheap PATH lookup: does `name` exist as an executable?
fn which(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in path.split(':') {
        let candidate = std::path::Path::new(dir).join(name);
        if candidate.is_file() {
            return true;
        }
    }
    false
}

/// Find a local IP on the same /24 as the Chromecast so the device can reach us.
/// Falls back to None if no match — caller uses configured device addr as best-effort.
fn resolve_lan_host(device_addr: &str) -> Option<String> {
    let device_prefix = device_addr.rsplitn(2, '.').nth(1)?.to_string();
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .ok()?;
    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        // line example: "2: wlan0    inet 192.168.1.42/24 brd ..."
        let inet_pos = line.find("inet ")?;
        let after = &line[inet_pos + 5..];
        let addr_with_mask = after.split_whitespace().next()?;
        let addr = addr_with_mask.split('/').next()?;
        if addr.starts_with("127.") {
            continue;
        }
        if let Some(prefix) = addr.rsplitn(2, '.').nth(1) {
            if prefix == device_prefix {
                return Some(addr.to_string());
            }
        }
    }
    None
}
