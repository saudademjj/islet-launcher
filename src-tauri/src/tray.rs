//! Native tray and the lifetime of background work. Read current services
//! on every action so moving the data directory does not leave stale paths.
use crate::LauncherState;
use islet::core::i18n::{english, pick};
use islet::services::{Activity, UiEvent};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Manager, Wry,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "launcher-tray";
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static HIDE_TIP_SHOWN: AtomicBool = AtomicBool::new(false);
static STOP_CONFIRMING: AtomicBool = AtomicBool::new(false);

struct TrayState {
    icon: TrayIcon<Wry>,
    status: MenuItem<Wry>,
    show: MenuItem<Wry>,
    hide: MenuItem<Wry>,
    play: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    logs: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    last: Mutex<Option<View>>,
}

#[derive(PartialEq, Eq, Clone)]
struct View {
    english: bool,
    activity: Activity,
    games: usize,
    world: Option<String>,
    room: bool,
    preparing: bool,
    scheduled: bool,
    hidden: bool,
}

/// Queued tasks have short idle gaps; require two continuous idle seconds.
#[derive(Default)]
struct IdleExit {
    since: Option<Instant>,
}
impl IdleExit {
    fn ready(&mut self, scheduled: bool, idle: bool, now: Instant) -> bool {
        if !scheduled || !idle {
            self.since = None;
            return false;
        }
        now.duration_since(*self.since.get_or_insert(now)) >= Duration::from_secs(2)
    }
}

pub(crate) fn available() -> bool {
    AVAILABLE.load(Ordering::SeqCst)
}

pub(crate) fn install(app: &AppHandle) -> tauri::Result<()> {
    let item = |id, title, enabled| MenuItem::with_id(app, id, title, enabled, None::<&str>);
    let status = item("tray-status", pick("就绪", "Ready"), false)?;
    let show = item("tray-show", pick("显示启动器", "Show launcher"), true)?;
    let hide = item("tray-hide", pick("隐藏启动器", "Hide launcher"), true)?;
    let play = item(
        "tray-play",
        pick("开始当前世界", "Play current world"),
        false,
    )?;
    let stop = item(
        "tray-stop",
        pick("停止正在运行的游戏", "Stop running games"),
        false,
    )?;
    let logs = item(
        "tray-logs",
        pick("打开日志文件夹", "Open logs folder"),
        true,
    )?;
    let quit = item("tray-quit", pick("退出启动器", "Quit launcher"), true)?;
    let first = PredefinedMenuItem::separator(app)?;
    let last = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &status, &first, &show, &hide, &play, &stop, &logs, &last, &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Islet")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| dispatch(app, event.id.as_ref()))
        .on_tray_icon_event(|icon, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                crate::events::show_main_window(icon.app_handle(), true);
                refresh(icon.app_handle());
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let icon = builder.build(app)?;
    app.manage(TrayState {
        icon,
        status,
        show,
        hide,
        play,
        stop,
        logs,
        quit,
        last: Mutex::new(None),
    });
    AVAILABLE.store(true, Ordering::SeqCst);
    refresh(app);
    tracing::info!(id = TRAY_ID, "system tray ready");
    Ok(())
}

pub(crate) fn watch_background_work(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut exit = IdleExit::default();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let services = app.state::<LauncherState>().services();
            let idle = services.activity() == Activity::Idle
                && !app.state::<crate::p2p::P2p>().is_preparing();
            if exit.ready(crate::quit_is_scheduled(), idle, Instant::now()) {
                if !crate::has_unsaved_changes() {
                    crate::quit(&app);
                    break;
                }
                // Edits made after quitting was scheduled are asked about,
                // not dropped: the page shows the save question instead.
                crate::keep_running(&app);
                crate::request_quit(&app);
            }
            refresh(&app);
        }
    });
}

/// `explain` shows, once, how to get the window back; a scheduled quit is
/// kept either way (the status bar or the tray cancels it).
pub(crate) fn hide_main_window(app: &AppHandle, explain: bool) -> bool {
    if !available() {
        return false;
    }
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    crate::set_window_tray_hidden(true);
    match window.hide() {
        Ok(()) => {
            if explain
                && !crate::quit_is_scheduled()
                && !HIDE_TIP_SHOWN.swap(true, Ordering::SeqCst)
                && app
                    .state::<LauncherState>()
                    .services()
                    .settings
                    .get()
                    .system_notifications
            {
                use tauri_plugin_notification::NotificationExt;
                let _ = app
                    .notification()
                    .builder()
                    .title(pick(
                        "草岛仍在后台",
                        "Islet is running in the background",
                    ))
                    .body(pick(
                        "点击托盘图标恢复窗口；右键菜单可以退出启动器。",
                        "Click the tray icon to restore the window; use its menu to quit.",
                    ))
                    .show();
            }
            refresh(app);
            tracing::info!("main window hidden to tray");
            true
        }
        Err(error) => {
            crate::set_window_tray_hidden(false);
            tracing::warn!(%error, "could not hide the main window to tray");
            false
        }
    }
}

pub(crate) fn refresh(app: &AppHandle) {
    let Some(tray) = app.try_state::<TrayState>() else {
        return;
    };
    let services = app.state::<LauncherState>().services();
    let settings = services.settings.get();
    let view = View {
        english: english(),
        activity: services.activity(),
        games: services.running_instance_ids().len(),
        world: settings
            .active_instance_id
            .and_then(|id| services.instances.get(&id).ok())
            .map(|i| i.name.chars().take(48).collect()),
        room: app.state::<crate::p2p::P2p>().has_activity()
            || app.state::<crate::tunnel::Tunnels>().has_activity(),
        preparing: app.state::<crate::p2p::P2p>().is_preparing(),
        scheduled: crate::quit_is_scheduled(),
        hidden: app
            .get_webview_window("main")
            .is_some_and(|w| !w.is_visible().unwrap_or(true)),
    };
    {
        let mut previous = tray.last.lock().unwrap_or_else(|e| e.into_inner());
        if previous.as_ref() == Some(&view) {
            return;
        }
        *previous = Some(view.clone());
    }
    let status = if view.scheduled {
        pick("任务完成后退出", "Quitting when work finishes").into()
    } else {
        match view.activity {
            Activity::Working => {
                pick("正在下载或处理文件", "Downloading or processing files").into()
            }
            Activity::Playing => {
                islet::tr!("游戏运行中（{}）", "Games running ({})", view.games)
            }
            Activity::Idle if view.preparing => {
                pick("正在准备联机", "Preparing multiplayer").into()
            }
            Activity::Idle if view.room => {
                pick("联机连接运行中", "Multiplayer connection active").into()
            }
            Activity::Idle => pick("就绪", "Ready").into(),
        }
    };
    let play = view
        .world
        .as_deref()
        .map(|name| {
            let name = name.replace('&', "&&");
            islet::tr!("开始：{name}", "Play: {name}")
        })
        .unwrap_or_else(|| pick("开始游戏（尚未选择世界）", "Play (no world selected)").into());
    let updates = [
        tray.status.set_text(&status),
        tray.show.set_text(pick("显示启动器", "Show launcher")),
        tray.hide.set_text(pick("隐藏启动器", "Hide launcher")),
        tray.hide.set_enabled(!view.hidden),
        tray.play.set_text(play),
        tray.play.set_enabled(
            view.world.is_some() && view.activity == Activity::Idle && !view.scheduled,
        ),
        tray.stop
            .set_text(pick("停止正在运行的游戏", "Stop running games")),
        tray.stop.set_enabled(view.games > 0),
        tray.logs
            .set_text(pick("打开日志文件夹", "Open logs folder")),
        tray.quit.set_text(if view.scheduled {
            pick("取消自动退出", "Cancel scheduled quit")
        } else {
            pick("退出启动器", "Quit launcher")
        }),
        tray.icon
            .set_tooltip(Some(format!("Islet · {status}"))),
    ];
    for error in updates.into_iter().filter_map(Result::err) {
        tracing::warn!(%error, "could not refresh tray item");
    }
}

fn dispatch(app: &AppHandle, id: &str) {
    tracing::info!(action = id, "tray menu action");
    let services = app.state::<LauncherState>().services();
    match id {
        "tray-show" => {
            crate::events::show_main_window(app, true);
        }
        "tray-hide" => {
            hide_main_window(app, true);
        }
        "tray-play" => {
            if services.activity() == Activity::Idle && !crate::quit_is_scheduled() {
                if let Some(instance_id) = services.settings.get().active_instance_id {
                    crate::events::show_main_window(app, true);
                    services.send(UiEvent::LaunchRequest { instance_id });
                }
            }
        }
        "tray-stop" => {
            if !services.running_instance_ids().is_empty()
                && !STOP_CONFIRMING.swap(true, Ordering::SeqCst)
            {
                app.dialog().message(pick(
                    "请先在游戏中保存世界。启动器会请求关闭正在运行的游戏；30 秒后仍未退出时会强制结束，未保存的进度可能丢失。",
                    "Save your worlds in the games first. The launcher will request shutdown; games still running after 30 seconds will be forced to close and may lose unsaved progress.",
                )).title(pick("停止正在运行的游戏", "Stop running games"))
                    .kind(MessageDialogKind::Warning)
                    .buttons(MessageDialogButtons::OkCancelCustom(pick("停止游戏", "Stop games").into(), pick("取消", "Cancel").into()))
                    .show(move |confirmed| {
                        STOP_CONFIRMING.store(false, Ordering::SeqCst);
                        if confirmed {
                            for id in services.running_instance_ids() {
                                if let Err(error) = services.stop_instance(&id, false) {
                                    tracing::warn!(%error, "could not request game shutdown from tray");
                                }
                            }
                        }
                    });
            }
        }
        "tray-logs" => {
            if let Err(error) = app
                .opener()
                .open_path(services.paths.logs.to_string_lossy(), None::<&str>)
            {
                tracing::warn!(%error, "could not open logs folder from tray");
            }
        }
        "tray-quit" if crate::quit_is_scheduled() => crate::keep_running(app),
        "tray-quit" => crate::request_quit(app),
        _ => {}
    }
    refresh(app);
}

pub(crate) fn shutdown(app: &AppHandle) {
    AVAILABLE.store(false, Ordering::SeqCst);
    if let Some(tray) = app.try_state::<TrayState>() {
        let _ = tray.icon.set_visible(false);
    }
    tracing::info!("system tray removed");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_work_resets_the_idle_exit_deadline() {
        let mut exit = IdleExit::default();
        let now = Instant::now();
        assert!(!exit.ready(true, true, now));
        assert!(!exit.ready(true, false, now + Duration::from_secs(1)));
        assert!(!exit.ready(true, true, now + Duration::from_secs(5)));
        assert!(exit.ready(true, true, now + Duration::from_secs(7)));
    }
    #[test]
    fn cancelling_and_rescheduling_does_not_reuse_an_old_deadline() {
        let mut exit = IdleExit::default();
        let now = Instant::now();
        assert!(!exit.ready(true, true, now));
        assert!(!exit.ready(false, true, now + Duration::from_secs(5)));
        assert!(!exit.ready(true, true, now + Duration::from_secs(6)));
        assert!(exit.ready(true, true, now + Duration::from_secs(8)));
    }
}
