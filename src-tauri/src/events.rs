//! Forwards service events to the web UI. Game output arrives one line at a
//! time and can run to thousands of lines a second while mods load, so lines
//! are batched to keep the UI at a handful of updates per second.

use std::collections::HashSet;
use std::time::Duration;

use async_channel::Receiver;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{Instant, timeout_at};

use islet::services::UiEvent;

use crate::LauncherState;

pub const EVENT: &str = "launcher-event";

/// How long the first line of a burst waits for company before it is sent.
const LOG_BATCH_WINDOW: Duration = Duration::from_millis(80);

/// Log lines that mean the game's window is up, or about to be. The launcher
/// steps aside only then: a big modpack can take a minute to get there, and
/// an empty screen meanwhile looks like nothing started.
const WINDOW_MARKERS: &[&str] = &[
    // 1.13 and newer, as the window is created.
    "Backend library: LWJGL",
    // 1.12 and older.
    "LWJGL Version:",
    // Forge and NeoForge show their own loading window first.
    "EARLYDISPLAY",
    // Any version, a little later; in case the lines above are missing.
    "Sound engine started",
];

/// Games that want the launcher out of the way, by stage.
#[derive(Default)]
struct Aside {
    /// Started, window not up yet.
    waiting: HashSet<String>,
    /// The launcher stepped aside for these. It only comes back when the
    /// last of them exits, so two running games cannot reveal it early.
    hidden: HashSet<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogLine {
    line: String,
    stream: String,
}

#[derive(Serialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
enum Batched<'a> {
    #[serde(rename_all = "camelCase")]
    LaunchLogs {
        instance_id: &'a str,
        lines: &'a [LogLine],
    },
}

pub fn forward(app: AppHandle, events: Receiver<UiEvent>) {
    tauri::async_runtime::spawn(async move {
        let mut next = None;
        let mut aside = Aside::default();
        loop {
            let event = match next.take() {
                Some(event) => event,
                None => match events.recv().await {
                    Ok(event) => event,
                    Err(_) => break,
                },
            };
            let UiEvent::LaunchLog {
                instance_id,
                line,
                stream,
            } = event
            else {
                handle(&app, &mut aside, event);
                continue;
            };

            let mut lines = vec![LogLine { line, stream }];
            let deadline = Instant::now() + LOG_BATCH_WINDOW;
            while let Ok(Ok(event)) = timeout_at(deadline, events.recv()).await {
                match event {
                    UiEvent::LaunchLog {
                        instance_id: ref other,
                        line,
                        stream,
                    } if *other == instance_id => lines.push(LogLine { line, stream }),
                    // Anything else (an exit, another instance) goes out after
                    // the lines that preceded it.
                    other => {
                        next = Some(other);
                        break;
                    }
                }
            }
            if aside.waiting.contains(&instance_id)
                && lines.iter().any(|line| WINDOW_MARKERS.iter().any(|marker| line.line.contains(marker)))
            {
                aside.waiting.remove(&instance_id);
                step_aside(&app, &mut aside, instance_id.clone());
            }
            emit(
                &app,
                &Batched::LaunchLogs {
                    instance_id: &instance_id,
                    lines: &lines,
                },
            );
        }
    });
}

/// Move aside for `instance_id`, retaining a tray or taskbar/Dock way back.
fn step_aside(app: &AppHandle, aside: &mut Aside, instance_id: String) {
    aside.hidden.insert(instance_id);
    if crate::window_tray_hidden() { return; }
    // Publish the intent before hiding: the startup fallback must never
    // observe "not hidden" between the hide and this flag.
    crate::set_window_auto_hidden(true);
    #[cfg(target_os = "macos")]
    let moved = show_main_window(app, false);
    #[cfg(not(target_os = "macos"))]
    let moved = match app.get_webview_window("main").map(|window| {
        if crate::tray::available() { window.hide() } else { window.minimize() }
    }) {
        Some(Ok(())) => true,
        Some(Err(error)) => {
            tracing::warn!(error = %error, "failed to minimize the launcher window");
            false
        }
        None => false,
    };
    if !moved {
        // The window never actually hid, so let the fallback keep working
        // for a page that fails to reveal itself.
        crate::set_window_auto_hidden(false);
    }
}

fn handle(app: &AppHandle, aside: &mut Aside, event: UiEvent) {
    use tauri_plugin_notification::NotificationExt;
    let settings = app.state::<LauncherState>().services().settings.get();
    let notification = match &event {
        UiEvent::InstallComplete { .. } => Some(islet::core::i18n::pick("游戏安装完成", "Game installation completed").to_string()),
        UiEvent::OperationComplete(Ok(message)) => Some(message.clone()),
        UiEvent::OperationComplete(Err(error)) | UiEvent::InstallError { error, .. } => Some(error.message.clone()),
        UiEvent::LaunchExit { success: false, .. } => Some(islet::core::i18n::pick("游戏异常退出，打开启动器查看崩溃分析", "The game crashed. Open the launcher for the crash analysis.").to_string()),
        _ => None,
    };
    if settings.system_notifications {
        if let Some(body) = notification {
            if let Err(error) = app.notification().builder().title("Islet").body(body).show() {
                tracing::warn!(%error, "system notification failed");
            }
        }
    }
    if matches!(&event, UiEvent::LaunchStarted { .. } | UiEvent::LaunchExit { .. }) {
        let services = app.state::<LauncherState>().services();
        let playing = services.running_instance_ids().first().and_then(|id| services.instances.get(id).ok());
        crate::presence::set(&settings.discord_application_id, playing.as_ref().map(|i| (i.name.as_str(), i.version_id.as_str())));
    }

    // LaunchStarted is only emitted after the Java process spawned, so a
    // failed start never hides anything; the window itself waits for the
    // game's (see WINDOW_MARKERS). LaunchExit covers normal exits and
    // crashes alike.
    match &event {
        UiEvent::LaunchStarted { instance_id, .. } => {
            if settings.hide_on_launch {
                aside.waiting.insert(instance_id.clone());
            }
        }
        UiEvent::LaunchExit { instance_id, .. } => {
            aside.waiting.remove(instance_id);
            if aside.hidden.remove(instance_id) && aside.hidden.is_empty()
                && crate::window_auto_hidden()
                && !crate::window_tray_hidden() && !crate::quit_is_scheduled()
            {
                crate::set_window_auto_hidden(false);
                show_main_window(app, true);
            }
        }
        _ => {}
    }

    // Moving the data directory finishes by reopening the services on the new
    // root; the UI then reloads everything with `bootstrap`.
    if let UiEvent::StorageChanged(Ok(root)) = &event {
        let state = app.state::<LauncherState>();
        match state.services().reopen(root.clone()) {
            Ok(services) => state.replace(services),
            Err(error) => {
                emit(app, &UiEvent::StorageChanged(Err(error.to_payload())));
                return;
            }
        }
    }
    emit(app, &event);
}

/// Hides or shows the main window, reporting whether the requested visibility
/// change actually succeeded. Showing also restores a minimized window.
pub(crate) fn show_main_window(app: &AppHandle, visible: bool) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    if visible {
        crate::set_window_tray_hidden(false);
        crate::set_window_auto_hidden(false);
    } else {
        return match window.hide() {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(error = %error, "failed to hide the launcher window");
                false
            }
        };
    }
    if window.is_minimized().unwrap_or(false)
        && let Err(error) = window.unminimize()
    {
        tracing::warn!(error = %error, "failed to restore the launcher window");
    }
    if let Err(error) = window.show() {
        tracing::warn!(error = %error, "failed to show the launcher window");
        return false;
    }
    if let Err(error) = window.set_focus() {
        tracing::warn!(error = %error, "failed to focus the launcher window");
    }
    true
}

fn emit(app: &AppHandle, payload: &impl Serialize) {
    if let Err(error) = app.emit(EVENT, payload) {
        tracing::warn!(error = %error, "failed to deliver event to the window");
    }
}
