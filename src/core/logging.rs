use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;

use crate::core::paths::AppPaths;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Initialise file logging under <app_data>/logs. The worker guard must live
/// for the whole process, otherwise buffered log lines are dropped on exit.
pub fn init(paths: &AppPaths) {
    let file_appender = tracing_appender::rolling::daily(&paths.logs, "launcher.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let result = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .try_init();

    if let Err(err) = result {
        eprintln!("failed to initialise file logging: {err}");
    }

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        data_dir = %paths.root.display(),
        "launcher starting"
    );
}
