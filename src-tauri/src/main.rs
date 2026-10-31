// Release builds on Windows have no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod shortcut;
mod open_files;
mod p2p;
mod presence;
mod tunnel;
mod self_update;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use islet::core::logging;
use islet::core::paths::AppPaths;
use islet::services::AppServices;

/// Moving the data directory swaps in a new service instance, so commands
/// read the current one through this lock instead of holding it.
pub struct LauncherState {
    services: RwLock<Arc<AppServices>>,
}

impl LauncherState {
    pub fn services(&self) -> Arc<AppServices> {
        self.services
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn replace(&self, services: Arc<AppServices>) {
        *self
            .services
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = services;
    }
}

/// Whether the launcher window is currently hidden because a game is running.
/// Kept separate from the `hide_on_launch` setting: the setting may change
/// while a game runs, but the window must stay hidden until the last launch
/// that hid it exits. The startup fallback reads this so it cannot undo an
/// automatic hide when a game starts within its delay.
static WINDOW_AUTO_HIDDEN: AtomicBool = AtomicBool::new(false);
static WINDOW_TRAY_HIDDEN: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_window_tray_hidden(hidden: bool) {
    WINDOW_TRAY_HIDDEN.store(hidden, Ordering::SeqCst);
}

pub(crate) fn window_tray_hidden() -> bool {
    WINDOW_TRAY_HIDDEN.load(Ordering::SeqCst)
}

pub(crate) fn set_window_auto_hidden(hidden: bool) {
    WINDOW_AUTO_HIDDEN.store(hidden, Ordering::SeqCst);
}

pub(crate) fn window_auto_hidden() -> bool {
    WINDOW_AUTO_HIDDEN.load(Ordering::SeqCst)
}

/// Set once the user confirmed quitting, so the close checks let it through.
static QUITTING: AtomicBool = AtomicBool::new(false);
/// Explicitly wait for games and background work to finish before quitting.
static QUIT_WHEN_IDLE: AtomicBool = AtomicBool::new(false);
/// The page holds edits that aren't saved yet (it reports them); quitting
/// asks to save or discard them first.
static UNSAVED: AtomicBool = AtomicBool::new(false);

pub(crate) fn quit(app: &tauri::AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
}

pub(crate) fn set_unsaved_changes(unsaved: bool) {
    UNSAVED.store(unsaved, Ordering::SeqCst);
}

pub(crate) fn has_unsaved_changes() -> bool {
    UNSAVED.load(Ordering::SeqCst)
}

pub(crate) fn quit_is_scheduled() -> bool {
    QUIT_WHEN_IDLE.load(Ordering::SeqCst)
}

/// Cancels a scheduled quit. Only an explicit choice does this (the status
/// bar or the tray); showing the window to watch the progress keeps it.
pub(crate) fn keep_running(app: &tauri::AppHandle) {
    if QUIT_WHEN_IDLE.swap(false, Ordering::SeqCst) {
        quit_schedule_changed(app, false);
    }
}

pub(crate) fn schedule_quit(app: &tauri::AppHandle) {
    use tauri::Manager;
    QUIT_WHEN_IDLE.store(true, Ordering::SeqCst);
    if tray::available() {
        tray::hide_main_window(app, false);
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.minimize();
    }
    quit_schedule_changed(app, true);
}

/// The page shows a scheduled quit with a way to cancel it.
fn quit_schedule_changed(app: &tauri::AppHandle, scheduled: bool) {
    use tauri::Manager;
    app.state::<LauncherState>()
        .services()
        .send(islet::services::UiEvent::QuitScheduled { scheduled });
    tray::refresh(app);
}

pub(crate) fn request_quit(app: &tauri::AppHandle) {
    if allow_close(app) { quit(app); }
}

/// Closing the window (or quitting) must not silently cut a download short,
/// lose the log and play time of a game that is still running, or throw away
/// edits the page hasn't saved. Returns true when the close should go ahead;
/// otherwise the page asks about the edits first, then about the work.
fn allow_close(app: &tauri::AppHandle) -> bool {
    use islet::services::Activity;
    use tauri::Manager;
    if QUITTING.load(Ordering::SeqCst) {
        return true;
    }
    let services = app.state::<LauncherState>().services();
    if !has_unsaved_changes()
        && services.activity() == Activity::Idle
        && !app.state::<p2p::P2p>().has_activity()
        && !app.state::<tunnel::Tunnels>().has_activity()
    {
        return true;
    }
    events::show_main_window(app, true);
    services.send(islet::services::UiEvent::QuitRequested);
    false
}

fn main() {
    // A separate data root keeps tests and UI previews away from real accounts.
    let paths = match std::env::var_os("MC_LAUNCHER_DATA_DIR")
        .map(AppPaths::from_root)
        .unwrap_or_else(AppPaths::new)
    {
        Ok(paths) => paths,
        Err(error) => fatal(&islet::tr!("无法初始化数据目录：{error}", "Couldn't set up the data folder: {error}")),
    };
    logging::init(&paths);
    shortcut::remember_startup(&std::env::args().collect::<Vec<_>>());
    open_files::argv(&std::env::args().collect::<Vec<_>>(), &std::env::current_dir().unwrap_or_default().to_string_lossy(), None);
    install_panic_hook();
    tracing::info!(data_dir = %paths.root.display(), "starting Tauri launcher");

    let (events_tx, events_rx) = async_channel::unbounded();
    let services = match AppServices::new(paths, events_tx) {
        Ok(services) => services,
        Err(error) => {
            tracing::error!(error = %error, "failed to initialize services");
            fatal(&islet::tr!("启动器初始化失败：{error}", "The launcher failed to start: {error}"))
        }
    };

    let result = tauri::Builder::default()
        // First, so a second start exits before opening a window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            open_files::argv(&argv, &cwd, Some(app));
            if let Some(id) = shortcut::launch_argument(&argv) {
                tauri::Manager::state::<LauncherState>(app)
                    .services()
                    .send(islet::services::UiEvent::LaunchRequest { instance_id: id });
            }
            events::show_main_window(app, true);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(tunnel::Tunnels::default())
        .manage(p2p::P2p::default())
        .manage(LauncherState {
            services: RwLock::new(services),
        })
        .setup(move |app| {
            self_update::clean_up_later(tauri::Manager::state::<LauncherState>(app).services().paths.cache.clone());
            events::forward(app.handle().clone(), events_rx);
            #[cfg(windows)]
            frameless(app);
            if let Err(error) = tray::install(app.handle()) {
                // A desktop without a notification area must retain the
                // normal close/minimize behaviour, never hide an orphan app.
                tracing::warn!(%error, "system tray unavailable; keeping normal window behaviour");
            }
            tray::watch_background_work(app.handle());
            show_window_fallback(app.handle().clone());
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("mcshot", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(screenshot_response(&app, request.uri()));
            });
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                use tauri::Manager;
                let app = window.app_handle();
                if window.label() == "main" && !QUITTING.load(Ordering::SeqCst)
                    && tray::available() && app.state::<LauncherState>().services().settings.get().close_to_tray
                {
                    api.prevent_close();
                    tray::hide_main_window(app, true);
                } else if !allow_close(app) {
                    api.prevent_close();
                }
            }
        })
        .on_page_load(|webview, payload| {
            tracing::info!(url = %payload.url(), event = ?payload.event(), label = webview.label(), "page load");
        })
        .invoke_handler(tauri::generate_handler![
            tunnel::start_tunnel,
            tunnel::stop_tunnel,
            tunnel::tunnel_status,
            p2p::p2p_defaults,
            p2p::p2p_detect_lan,
            p2p::p2p_host,
            p2p::p2p_join,
            p2p::p2p_status,
            p2p::p2p_stop,
            open_files::take_open_files,
            commands::bootstrap,
            commands::set_language,
            commands::quit_launcher,
            commands::quit_when_idle,
            commands::quit_scheduled,
            commands::cancel_scheduled_quit,
            commands::set_unsaved_changes,
            commands::request_quit,
            commands::system_memory,
            commands::frontend_log,
            commands::update_settings,
            commands::login_configured,
            commands::add_offline_account,
            commands::remove_account,
            commands::login_yggdrasil,
            commands::account_capes,
            commands::change_skin,
            commands::set_cape,
            commands::set_active_account,
            commands::create_instance,
            commands::delete_instance,
            commands::set_active_instance,
            commands::configure_instance,
            commands::configure_launch,
            commands::organize_instance,
            commands::change_version,
            commands::instance_busy,
            commands::read_log,
            commands::analyze_crash,
            commands::install_java_for_instance,
            commands::load_versions,
            commands::load_java_runtimes,
            commands::scan_java_directory,
            commands::download_java,
            commands::load_loader_versions,
            commands::search_mods,
            commands::load_mod_versions,
            commands::install_mod,
            commands::list_mods,
            commands::last_mod_change,
            commands::list_worlds,
            commands::update_mods,
            commands::install_pack,
            commands::inspect_mods,
            commands::inspect_packs,
            commands::mod_icons,
            commands::mod_meta,
            commands::list_packs,
            commands::recycled_instances,
            commands::list_datapacks,
            commands::list_configs,
            commands::read_config,
            commands::write_config,
            commands::save_backups,
            commands::storage_usage,
            shortcut::create_shortcut,
            shortcut::take_startup_launch,
            commands::patch_notes,
            commands::mod_details,
            commands::mod_changelog,
            commands::search_modpacks,
            commands::modpack_versions,
            commands::upload_log,
            commands::set_resource_packs,
            commands::check_update,
            commands::update_source,
            commands::install_update,
            commands::modpack_info,
            commands::modpack_update,
            commands::install_optifine,
            commands::scan_games,
            commands::known_game_dirs,
            commands::file_operation,
            commands::relocate_storage,
            commands::install_instance,
            commands::cancel_task,
            commands::launch_instance,
            commands::stop_instance,
            commands::start_login,
            commands::cancel_login,
            commands::open_location,
            commands::open_url,
            commands::list_servers,
            commands::edit_server,
            commands::ping_server,
            commands::check_server_mods,
            commands::remote_info,
            commands::remote_connect,
            commands::remote_settings,
            commands::remote_forget,
            commands::remote_status,
            commands::remote_action,
            commands::remote_command,
            commands::remote_log,
            commands::remote_download_backup,
            commands::list_screenshots,
            commands::recycle_screenshot,
            commands::read_game_options,
            commands::write_game_options,
        ])
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|app, event| match event {
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Opened { urls } => {
                    open_files::enqueue(urls.into_iter().filter_map(|u| u.to_file_path().ok()), Some(app));
                    events::show_main_window(app, true);
                }
                // The Dock icon brings back a window hidden while playing.
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    events::show_main_window(app, true);
                }
                tauri::RunEvent::ExitRequested { api, .. } if !allow_close(app) => api.prevent_exit(),
                // Managed state is not dropped on exit, so the ssh and
                // EasyTier children would outlive the launcher.
                tauri::RunEvent::Exit => {
                    use tauri::Manager;
                    tray::shutdown(app);
                    app.state::<tunnel::Tunnels>().stop_all();
                    app.state::<p2p::P2p>().stop_all();
                }
                _ => {}
            })
        });
    if let Err(error) = result {
        tracing::error!(error = %error, "failed to run the window");
        fatal(&islet::tr!("无法创建启动器窗口。\n\n{error:#}", "Couldn't create the launcher window.\n\n{error:#}"));
    }
}

/// Serves `mcshot://localhost/<instance>%2F<file>` (on Windows
/// `http://mcshot.localhost/...`): a cached thumbnail, or with `?full` the
/// screenshot itself. Names are checked to be images inside the instance's
/// screenshots folder.
fn screenshot_response(app: &tauri::AppHandle, uri: &tauri::http::Uri) -> tauri::http::Response<Vec<u8>> {
    use tauri::Manager;
    use tauri::http::{Response, StatusCode, header};
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .expect("static response")
    };
    // convertFileSrc encodes the whole path, slash included.
    let path = percent_decode(uri.path());
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let [id, name] = segments.as_slice() else {
        return not_found();
    };
    let full = uri.query().is_some_and(|query| query.split('&').any(|pair| pair == "full"));
    let services = app.state::<LauncherState>().services();
    let paths = &services.paths;
    let result = if full {
        islet::core::screenshots::file(paths, id, name)
            .and_then(|path| Ok((std::fs::read(path)?, islet::core::screenshots::content_type(name))))
    } else {
        islet::core::screenshots::thumbnail(paths, id, name)
    };
    match result {
        Ok((bytes, kind)) => Response::builder()
            .header(header::CONTENT_TYPE, kind)
            // URLs carry the file's time, so a cached copy never goes stale.
            .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
            .body(bytes)
            .unwrap_or_else(|_| not_found()),
        Err(error) => {
            tracing::debug!(error = %error, "screenshot request failed");
            not_found()
        }
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The window starts hidden and the page shows it after its first paint. If
/// the page never gets there (a script error, a failed load), show the window
/// anyway so the launcher cannot end up running with nothing on screen.
fn show_window_fallback(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        // A launch that started within those four seconds may have hidden the
        // window on purpose; leave it hidden until the game exits.
        if window_auto_hidden() || window_tray_hidden() {
            tracing::debug!("window is hidden by a running launch; skipping fallback");
            return;
        }
        if let Some(window) = app.get_webview_window("main")
            && !window.is_visible().unwrap_or(true)
            && !window_auto_hidden() && !window_tray_hidden()
        {
            tracing::warn!("page did not reveal the window in time; showing it");
            let _ = window.show();
            let _ = window.set_focus();
            // A launch can hide the window between the check above and the
            // show; the flag is set before hiding, so re-check and honour it.
            if window_auto_hidden() || window_tray_hidden() {
                let _ = window.hide();
            }
        }
    });
}

/// The overlay title bar in tauri.conf.json is macOS-only; Windows would put
/// its white title bar above the forest frame. So the window drops it and the
/// page draws its own buttons (`WindowBar`), keeping the shadow and Windows
/// 11's rounded corners. The thin border Windows 11 draws around the window
/// goes too: it shows as a pale outline against the dark frame.
#[cfg(windows)]
fn frameless(app: &tauri::App) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.set_decorations(false) {
        tracing::warn!(%error, "couldn't remove the native title bar");
        return;
    }
    // Still hidden: centre it again now that it is shorter.
    let _ = window.center();

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: *mut std::ffi::c_void, attribute: u32, value: *const std::ffi::c_void, size: u32) -> i32;
    }
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    if let Ok(hwnd) = window.hwnd() {
        // Windows 10 has no border colour and just returns an error.
        unsafe {
            DwmSetWindowAttribute(hwnd.0, DWMWA_BORDER_COLOR, (&DWMWA_COLOR_NONE as *const u32).cast(), 4);
        }
    }
}

/// Release builds abort on panic, so log the reason before the process ends.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(panic = %info, "launcher crashed");
        default(info);
    }));
}

fn fatal(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}
