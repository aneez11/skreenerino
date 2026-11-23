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
                let sink_for_join = sink;

                while let Some(command) = cmd_rx.recv().await {
                    match command {
                        UiCommand::Rescan => {
                            let proxy = proxy.clone();
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
                        UiCommand::Join { target } => {
                            if let Some(task) = current.take() {
                                task.abort();
                            }
                            let sink = sink_for_join.clone();
                            let proxy = proxy.clone();
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
                        UiCommand::StartShare { .. } => {
                            let _ = proxy.send_event(UiEvent::Sharing(true));
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
