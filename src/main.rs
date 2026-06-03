use grod::cast;
use grod::cli;
use grod::config;
use grod::daemon;
use grod::piped;
use grod::queue;
use grod::streamer;
use grod::tui;

use anyhow::{bail, Context, Result};
use clap::Parser;

use cast::Caster;
use cli::{Cli, Commands, ConfigAction, DaemonAction};
use config::{Config, Quality};
use piped::{extract_video_id, PipedClient};
use queue::{Queue, QueueEntry};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;

    match cli.command {
        Commands::Config { action } => handle_config(action, cfg).await,
        Commands::Daemon { action } => match action.unwrap_or_default() {
            DaemonAction::Start => handle_daemon(cfg).await,
            DaemonAction::Stop => daemon::stop(),
            DaemonAction::Status => daemon::print_status(&cfg).await,
        },
        Commands::Firewall => {
            daemon::print_firewall_commands(cfg.api_port, cfg.stream_port);
            Ok(())
        }
        Commands::Tui => {
            let caster = require_caster(&cfg)?;
            tui::TuiApp::new(caster)?.run()
        }
        cmd => {
            let caster = require_caster(&cfg)?;
            let piped = require_piped(&cfg)?;
            let queue = Queue::open()?;
            let quality = cfg.default_quality;
            handle_command(cmd, caster, piped, queue, quality).await
        }
    }
}

fn require_caster(cfg: &Config) -> Result<Caster> {
    if cfg.device_addr.is_empty() {
        bail!("No device configured. Run: grod config set-device <addr> [port]");
    }
    Ok(Caster::new(&cfg.device_addr, cfg.device_port))
}

fn require_piped(cfg: &Config) -> Result<PipedClient> {
    if cfg.piped_api.is_empty() {
        bail!("No Piped API configured. Run: grod config set-api <url>");
    }
    Ok(PipedClient::new(&cfg.piped_api))
}

async fn handle_command(
    cmd: Commands,
    caster: Caster,
    piped: PipedClient,
    queue: Queue,
    quality: Quality,
) -> Result<()> {
    match cmd {
        Commands::Cast { url, queue: force_queue } => {
            let id = extract_video_id(&url)
                .with_context(|| format!("Could not extract video ID from: {url}"))?;

            if force_queue || caster.is_playing() {
                let title = piped.title(&id).await?;
                let pos = queue.push(QueueEntry { id, title: title.clone() })?;
                println!("Queued at position {pos}: {title}");
                ensure_daemon(&caster);
            } else {
                if let Some(next) = queue.pop()? {
                    cast_entry(&caster, &piped, &queue, next, quality).await?;
                    let title = piped.title(&id).await?;
                    let pos = queue.push(QueueEntry { id, title: title.clone() })?;
                    println!("Playing queued video first; yours queued at position {pos}: {title}");
                    ensure_daemon(&caster);
                } else {
                    cast_entry(
                        &caster,
                        &piped,
                        &queue,
                        QueueEntry { id: id.clone(), title: String::new() },
                        quality,
                    )
                    .await?;
                }
            }
        }

        Commands::Queue { url } => {
            let id = extract_video_id(&url)
                .with_context(|| format!("Could not extract video ID from: {url}"))?;
            let title = piped.title(&id).await?;
            let pos = queue.push(QueueEntry { id, title: title.clone() })?;
            println!("Queued at position {pos}: {title}");
            ensure_daemon(&caster);
        }

        Commands::Skip => {
            caster.stop()?;
            match queue.pop()? {
                Some(entry) => cast_entry(&caster, &piped, &queue, entry, quality).await?,
                None => println!("Queue empty"),
            }
        }

        Commands::PlayPause => caster.toggle_pause()?,
        Commands::Mute => caster.mute()?,
        Commands::Unmute => caster.unmute()?,
        Commands::VolumeUp => caster.volume_up()?,
        Commands::VolumeDown => caster.volume_down()?,
        Commands::Volume { level } => caster.set_volume(level)?,
        Commands::Forward { seconds } => caster.seek_forward(seconds)?,
        Commands::Back { seconds } => caster.seek_back(seconds)?,

        Commands::List => {
            let entries = queue.load()?;
            if entries.is_empty() {
                println!("Queue empty");
            } else {
                println!("Queue ({} videos):", entries.len());
                for (i, e) in entries.iter().enumerate() {
                    println!("  {:>3}. {}", i + 1, e.title);
                }
            }
        }

        Commands::Remove { position } => {
            let removed = queue.remove(position)?;
            println!("Removed: {}", removed.title);
        }

        Commands::Clear => {
            queue.clear()?;
            println!("Queue cleared");
        }

        Commands::Status => {
            let raw = caster.status_raw()?;
            let occupied = raw.contains("PLAYING")
                || raw.contains("PAUSED")
                || raw.contains("BUFFERING");
            let state_label = if raw.contains("PAUSED") {
                "Paused"
            } else if raw.contains("BUFFERING") {
                "Buffering"
            } else {
                "Now playing"
            };

            if occupied {
                let time = caster.time_remaining()
                    .map(|s| format!("{s}s remaining"))
                    .unwrap_or_default();
                match queue.now_playing()? {
                    Some(e) => println!("{state_label}: {}\n  ID: {} | {time}", e.title, e.id),
                    None => println!("{state_label}: (cast outside grod) | {time}"),
                }
            } else {
                println!("Idle");
                queue.clear_now_playing()?;
            }
            let count = queue.len()?;
            println!("Queue: {count} video(s) waiting");
            println!("Daemon: {}", if daemon::is_running() { "running" } else { "stopped" });
        }

        _ => unreachable!(),
    }
    Ok(())
}

async fn cast_entry(
    caster: &Caster,
    piped: &PipedClient,
    queue: &Queue,
    entry: QueueEntry,
    quality: Quality,
) -> Result<()> {
    let video = piped.resolve(&entry.id, quality).await?;
    let title = if entry.title.is_empty() { video.title.clone() } else { entry.title };
    println!("Casting: {}", title);

    // CLI path has no local muxing streamer running — fall back to direct stream_url.
    // For high-quality (1080p) playback, run `grod daemon` which hosts the muxer.
    let url = match video.stream_url.as_deref() {
        Some(u) => u.to_string(),
        None => anyhow::bail!(
            "no muxed fallback available for this video; start `grod daemon` for high-quality casting"
        ),
    };

    if video.video_url.is_some() && !daemon::is_running() {
        eprintln!(
            "Note: streaming at ~360p (muxed fallback). Run `grod daemon` for {} muxed playback.",
            quality.label()
        );
    }

    let ct = if url.contains(".m3u8") || url.contains("/hls/") {
        "application/x-mpegurl"
    } else {
        "video/mp4"
    };
    caster.load(&url, Some(ct))?;
    queue.set_now_playing(&QueueEntry {
        id: entry.id,
        title,
    })?;
    Ok(())
}

fn ensure_daemon(_caster: &Caster) {
    if !daemon::is_running() {
        eprintln!("Hint: run `grod daemon` in background to auto-advance queue");
    }
}

async fn handle_daemon(cfg: Config) -> Result<()> {
    if cfg.device_addr.is_empty() {
        bail!("No device configured. Run: grod config set-device <addr> [port]");
    }
    if cfg.piped_api.is_empty() {
        bail!("No Piped API configured. Run: grod config set-api <url>");
    }
    if daemon::is_running() {
        println!("Daemon already running");
        return Ok(());
    }
    println!("Starting daemon (Ctrl-C to stop, or `grod daemon stop` from another shell)...");
    println!("HTTP API listening on 0.0.0.0:{}", cfg.api_port);
    if !cfg.api_pin.is_empty() {
        println!("API PIN protection enabled");
    }
    daemon::run_loop(daemon::DaemonConfig {
        piped_api: cfg.piped_api,
        device_addr: cfg.device_addr,
        device_port: cfg.device_port,
        api_port: cfg.api_port,
        stream_port: cfg.stream_port,
        api_pin: cfg.api_pin,
        default_quality: cfg.default_quality,
        encoder: cfg.encoder,
    }).await
}

async fn handle_config(action: ConfigAction, mut cfg: Config) -> Result<()> {
    match action {
        ConfigAction::Show => {
            println!("Piped API : {}", if cfg.piped_api.is_empty() { "(not set)" } else { &cfg.piped_api });
            println!("Device    : {}:{}", if cfg.device_addr.is_empty() { "(not set)" } else { &cfg.device_addr }, cfg.device_port);
            println!("API port  : {}", cfg.api_port);
            println!("Stream port: {}", cfg.stream_port);
            println!("API PIN   : {}", if cfg.api_pin.is_empty() { "(not set)" } else { "****" });
            println!("Quality   : {}", cfg.default_quality.label());
            let resolved = streamer::resolve_encoder(cfg.encoder);
            if matches!(cfg.encoder, config::Encoder::Auto) {
                println!("Encoder   : {} (auto \u{2192} {})", cfg.encoder.label(), resolved.label());
            } else {
                println!("Encoder   : {}", cfg.encoder.label());
            }
        }
        ConfigAction::SetApi { url } => {
            cfg.piped_api = url.clone();
            cfg.save()?;
            println!("Piped API set to: {url}");
        }
        ConfigAction::SetDevice { addr, port } => {
            cfg.device_addr = addr.clone();
            cfg.device_port = port;
            cfg.save()?;
            println!("Device set to: {addr}:{port}");
        }
        ConfigAction::SetPin { pin } => {
            cfg.api_pin = pin.clone();
            cfg.save()?;
            if pin.is_empty() {
                println!("API PIN disabled");
            } else {
                println!("API PIN set");
            }
        }
        ConfigAction::SetQuality { quality } => {
            let q = Quality::parse(&quality)
                .with_context(|| format!("invalid quality '{quality}' (use best|1080p|720p|480p|360p)"))?;
            cfg.default_quality = q;
            cfg.save()?;
            println!("Default quality set to: {}", q.label());
        }
        ConfigAction::SetEncoder { encoder } => {
            let e = config::Encoder::parse(&encoder)
                .with_context(|| format!("invalid encoder '{encoder}' (use auto|cpu|vaapi|nvenc|qsv)"))?;
            cfg.encoder = e;
            cfg.save()?;
            println!("Encoder set to: {}", e.label());
            if matches!(e, config::Encoder::Auto) {
                let resolved = streamer::resolve_encoder(e);
                println!("  Auto resolves on this host to: {}", resolved.label());
            }
        }
        ConfigAction::Discover => {
            let out = std::process::Command::new("go-chromecast")
                .arg("ls")
                .output()
                .context("go-chromecast ls failed — is go-chromecast installed?")?;
            let raw = String::from_utf8_lossy(&out.stdout);

            let mut devices: Vec<(String, String, u16)> = Vec::new();
            for line in raw.lines() {
                if let Some(addr_start) = line.find("address=\"") {
                    let after = &line[addr_start + 9..];
                    if let Some(end) = after.find('"') {
                        let addr_port = &after[..end];
                        let mut parts = addr_port.rsplitn(2, ':');
                        let port: u16 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(8009);
                        let ip = parts.next().unwrap_or("").to_string();

                        let name = if let Some(ns) = line.find("device_name=\"") {
                            let n = &line[ns + 13..];
                            n[..n.find('"').unwrap_or(n.len())].to_string()
                        } else {
                            ip.clone()
                        };

                        devices.push((name, ip, port));
                    }
                }
            }

            if devices.is_empty() {
                println!("No devices found");
                return Ok(());
            }

            for (i, (name, ip, port)) in devices.iter().enumerate() {
                println!("  {}) {} — {}:{}", i + 1, name, ip, port);
            }

            print!("Select device [1-{}]: ", devices.len());
            use std::io::{BufRead, Write};
            std::io::stdout().flush()?;
            let stdin = std::io::stdin();
            let line = stdin.lock().lines().next()
                .context("no input")??;
            let idx: usize = line.trim().parse().context("enter a number")?;
            if idx < 1 || idx > devices.len() {
                bail!("invalid selection");
            }
            let (name, ip, port) = &devices[idx - 1];
            cfg.device_addr = ip.clone();
            cfg.device_port = *port;
            cfg.save()?;
            println!("Device set to: {name} ({ip}:{port})");
        }
    }
    Ok(())
}
