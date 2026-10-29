mod app;
mod app_modals;
mod components;
mod nav;
mod screens;
mod state;
mod theme;

use app::GitPilotApp;
use directories::ProjectDirs;
use gitpilot_core::AppService;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() {
    if let Err(error) = run() {
        eprintln!("GitPilot failed to start: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let data_dir = std::env::var_os("GITPILOT_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            ProjectDirs::from("com", "gitpilot", "GitPilot")
                .map(|dirs| dirs.data_dir().to_path_buf())
        })
        .unwrap_or_else(|| std::path::PathBuf::from(".gitpilot"));

    if let Err(error) = validate_data_dir(&data_dir) {
        eprintln!(
            "GitPilot cannot use its data directory {}: {error}\n\
             Set GITPILOT_DATA_DIR to a writable directory and relaunch.",
            data_dir.display()
        );
        std::process::exit(1);
    }

    init_tracing(&data_dir);
    info!(?data_dir, "starting GitPilot");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("gitpilot-worker")
        .build()
        .expect("failed to initialize Tokio runtime");
    let handle = runtime.handle().clone();

    let credentials = gitpilot_core::credentials::platform_store();
    let bootstrap = runtime.block_on(async {
        let service = AppService::open(data_dir, credentials, true)
            .await
            .map_err(|error| error.user_message())?;
        let bootstrap = service
            .bootstrap()
            .await
            .map_err(|error| error.user_message())?;
        Ok::<_, String>((Arc::new(service), bootstrap))
    })?;
    let (service, bootstrap) = bootstrap;
    let _job_shutdown = tokio_util::sync::CancellationToken::new();
    let runtime_guard = runtime.enter();
    service.start_job_dispatch(_job_shutdown.clone());
    drop(runtime_guard);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("GitPilot")
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([1060.0, 680.0])
            .with_app_id("GitPilot"),
        vsync: true,
        ..Default::default()
    };
    let app = Box::new(move |cc: &eframe::CreationContext<'_>| {
        theme::install(
            &cc.egui_ctx,
            match bootstrap.settings.theme {
                gitpilot_core::ThemeMode::Light => theme::Theme::Light,
                _ => theme::Theme::Dark,
            },
            bootstrap.settings.font_scale,
            &bootstrap.settings.density,
        );
        Ok(Box::new(GitPilotApp::new(
            Arc::clone(&service),
            handle.clone(),
            bootstrap,
        )) as Box<dyn eframe::App>)
    });
    let result = eframe::run_native("GitPilot", options, app);
    drop(runtime);
    result.map_err(|error| {
        let message = format!("GUI initialization failed: {error}");
        eprintln!("{message}");
        message
    })
}

fn init_tracing(data_dir: &std::path::Path) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("gitpilot=info,sqlx=warn"));
    if let Err(error) = gitpilot_core::logging::install_file_logging(data_dir, filter) {
        eprintln!("Warning: durable file logging is unavailable: {error}");
    }
}

fn validate_data_dir(data_dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|error| format!("could not create directory: {error}"))?;
    let probe = data_dir.join(".gitpilot-write-probe");
    std::fs::write(&probe, b"ok").map_err(|error| format!("directory is not writable: {error}"))?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}
