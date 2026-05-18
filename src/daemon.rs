use anyhow::Result;
use std::path::PathBuf;
use std::time::Duration;

use crate::cast::Caster;
use crate::config::data_path;
use crate::piped::PipedClient;
use crate::queue::{Queue, QueueEntry};

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

/// Run daemon loop in current process (caller should have forked/spawned).
pub async fn run_loop(piped_api: String, addr: String, port: u16) -> Result<()> {
    write_pid()?;

    let queue = Queue::open()?;
    let caster = Caster::new(&addr, port);
    let piped = PipedClient::new(&piped_api);

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        if caster.is_playing() {
            continue;
        }

        // Device idle — clear now_playing, try to advance queue
        let _ = queue.clear_now_playing();

        match queue.pop()? {
            None => continue,
            Some(entry) => {
                eprintln!("[daemon] casting: {}", entry.title);
                match piped.resolve(&entry.id).await {
                    Err(e) => eprintln!("[daemon] resolve failed for {}: {e}", entry.id),
                    Ok(video) => {
                        if let Err(e) = caster.load(&video.stream_url) {
                            eprintln!("[daemon] cast failed: {e}");
                        } else {
                            let _ = queue.set_now_playing(&QueueEntry {
                                id: entry.id,
                                title: entry.title,
                            });
                        }
                    }
                }
            }
        }
    }
}
