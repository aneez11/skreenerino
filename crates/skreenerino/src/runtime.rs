//! Session runtime: owns the tokio runtime and translates UI commands into
//! skreen-app calls. Runs off the UI thread so the window never blocks.

use crate::app::{UiCommand, UiEvent, UiProxy, UiSink};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tokio::sync::mpsc as async_mpsc;

/// Spawn the session runtime on its own thread and return immediately.
///
/// The UI sends commands over a std channel (so it never blocks); a small thread
/// forwards them into a tokio channel. Without this split, a blocking `recv()`
/// inside `block_on` would starve the very tasks it just spawned.
pub fn spawn_runtime(
    _handle: crate::app::AppHandle,
    commands: Receiver<UiCommand>,
    sink: Arc<UiSink>,
    proxy: UiProxy,
) {
    std::thread::Builder::new()
        .name("skreenerino-runtime".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = proxy.send_event(UiEvent::Warning(format!("runtime init failed: {e}")));
                    return;
                }
            };

            let (cmd_tx, mut cmd_rx) = async_mpsc::channel::<UiCommand>(32);
            std::thread::Builder::new()
                .name("skreenerino-cmd-bridge".into())
                .spawn(move || {
                    while let Ok(command) = commands.recv() {
                        if cmd_tx.blocking_send(command).is_err() {
                            break;
                        }
                    }
                })
                .ok();

            runtime.block_on(async move {
                let mut current: Option<tokio::task::JoinHandle<()>> = None;
                let proxy_only = proxy.clone();
                let sink_for_join = sink;

                while let Some(command) = cmd_rx.recv().await {
                    match command {
                        UiCommand::Rescan => {
                            let proxy = proxy_only.clone();
                            tokio::task::spawn_blocking(move || {
                                match skreen_app::discovery::browse_peers() {
                                    Ok(peers) => {
                                        let _ = proxy.send_event(UiEvent::Peers(peers));
                                    }
                                    Err(e) => {
                                        let _ = proxy.send_event(UiEvent::Warning(format!(
                                            "discovery: {e}"
                                        )));
                                    }
                                }
                            });
                        }
                        UiCommand::ScanMonitors => {
                            let proxy = proxy_only.clone();
                            tokio::task::spawn_blocking(move || match scan_monitors() {
                                Ok(list) => {
                                    let _ = proxy.send_event(UiEvent::Monitors(list));
                                }
                                Err(e) => {
                                    let _ = proxy.send_event(UiEvent::Warning(format!(
                                        "monitor scan failed: {e}"
                                    )));
                                }
                            });
                        }
                        UiCommand::AnswerConsent {
                            requester,
                            accepted,
                        } => {
                            if let Ok(slot) = ACTIVE_SHARE.lock() {
                                if let Some(shared) = slot.as_ref() {
                                    if accepted {
                                        shared.approve(&requester);
                                    } else {
                                        shared.deny(&requester);
                                    }
                                }
                            }
                        }
                        UiCommand::Join { target } => {
                            if let Some(task) = current.take() {
                                task.abort();
                            }
                            let sink = sink_for_join.clone();
                            let proxy = proxy_only.clone();
                            current = Some(tokio::spawn(async move {
                                match skreen_app::discovery::resolve_target(&target) {
                                    Ok(addr) => {
                                        let opts = skreen_app::viewer::ViewerOptions::new(addr);
                                        let proxy_stats = proxy.clone();
                                        let stats_seen =
                                            Arc::new(std::sync::atomic::AtomicU32::new(0));
                                        let result = skreen_app::viewer::run_viewer(
                                            opts,
                                            sink,
                                            move |stats| {
                                                stats_seen.store(
                                                    stats.frames,
                                                    std::sync::atomic::Ordering::Relaxed,
                                                );
                                                let _ =
                                                    proxy_stats.send_event(UiEvent::Stats(stats));
                                            },
                                        )
                                        .await;
                                        if let Err(e) = result {
                                            let _ = proxy
                                                .send_event(UiEvent::Warning(format!("{e:#}")));
                                        }
                                        let _ = proxy.send_event(UiEvent::Ended);
                                    }
                                    Err(e) => {
                                        let _ =
                                            proxy.send_event(UiEvent::Warning(format!("{e:#}")));
                                    }
                                }
                            }));
                        }
                        UiCommand::StartShare {
                            room,
                            name,
                            fps,
                            bitrate,
                            scale,
                            audio,
                            monitor,
                        } => {
                            if let Some(task) = current.take() {
                                task.abort();
                            }
                            let proxy_task = proxy_only.clone();
                            let proxy_done = proxy_only.clone();
                            current = Some(tokio::spawn(async move {
                                match start_share(
                                    room,
                                    name,
                                    fps,
                                    bitrate,
                                    scale,
                                    audio,
                                    monitor,
                                    proxy_task.clone(),
                                )
                                .await
                                {
                                    Ok(()) => {
                                        let _ = proxy_task.send_event(UiEvent::Ended);
                                    }
                                    Err(e) => {
                                        let _ = proxy_task.send_event(UiEvent::Warning(format!(
                                            "sharing failed: {e:#}"
                                        )));
                                        let _ = proxy_task.send_event(UiEvent::Ended);
                                    }
                                }
                            }));
                            let _ = proxy_done.send_event(UiEvent::Sharing(true));
                        }
                        UiCommand::StopShare | UiCommand::Leave => {
                            if let Some(task) = current.take() {
                                task.abort();
                            }
                            let _ = proxy.send_event(UiEvent::Ended);
                        }
                    }
                }
            });
        })
        .ok();
}
/// Enumerate displays and grab a small preview of each.
fn scan_monitors() -> anyhow::Result<Vec<crate::app::MonitorChoice>> {
    let infos = skreen_capture::list_monitors()?;
    let mut out = Vec::with_capacity(infos.len());
    for (index, info) in infos.into_iter().enumerate() {
        let preview = skreen_capture::monitor_thumbnail(index, 320)
            .ok()
            .map(|(bytes, width, height)| (bytes.to_vec(), width, height));
        out.push(crate::app::MonitorChoice {
            info,
            preview,
            texture: None,
        });
    }
    Ok(out)
}
/// Bind a room and present the chosen monitor until the share is stopped.
async fn start_share(
    room: Option<String>,
    name: Option<String>,
    fps: u32,
    bitrate: u32,
    scale: f32,
    audio: bool,
    monitor: Option<u32>,
    proxy: UiProxy,
) -> anyhow::Result<()> {
    let bind: std::net::SocketAddr = "0.0.0.0:7654".parse()?;
    let server = skreen_transport::quic::QuicServer::bind(bind).await?;
    let cfg = skreen_app::presenter::RoomConfig {
        bitrate,
        fps,
        name: name.unwrap_or_else(skreen_app::discovery::default_device_name),
        audio,
        scale,
        monitor,
        require_approval: true,
    };
    let shared = std::sync::Arc::new(skreen_app::presenter::RoomShared::new());
    let room_name = room.unwrap_or_else(|| "default".into());
    tracing::info!(room = %room_name, "sharing started");

    // Surface pending join requests to the UI so the host can approve them.
    let watch = shared.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            for peer in watch.take_join_requests() {
                let _ = proxy.send_event(UiEvent::ConsentRequested {
                    requester: peer,
                    kind: skreen_security::ConsentKind::Pull,
                });
            }
        }
    });

    let stop = std::future::pending::<()>();
    skreen_app::presenter::run_room(server, cfg, shared, stop).await
}
/// The room currently being presented, if any, so consent decisions can reach it.
static ACTIVE_SHARE: std::sync::Mutex<Option<Arc<skreen_app::presenter::RoomShared>>> =
    std::sync::Mutex::new(None);
