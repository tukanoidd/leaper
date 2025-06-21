mod app;
mod cli;

use std::sync::Arc;

use clap::Parser;
use directories::ProjectDirs;
use iced_layershell::{
    build_pattern::MainSettings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::{LayerShellSettings, Settings, StartMode},
};
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{app::App, cli::Cli};

fn main() -> LeaperResult<()> {
    miette::set_panic_hook();

    let Cli { mode, trace, debug } = Cli::parse();

    init_tracing(trace, debug)?;

    let project_dirs =
        ProjectDirs::from("com", "tukanoid", "leaper").ok_or(LeaperError::NoProjectDirs)?;

    let Settings {
        fonts,
        default_font,
        default_text_size,
        antialiasing,
        virtual_keyboard_support,
        ..
    } = Settings::<()>::default();

    let size = match mode {
        cli::AppMode::Apps => (500, 800),
        cli::AppMode::Runner => (600, 100),
    };

    let settings = MainSettings {
        id: Some("com.tukanoid.leaper".into()),
        layer_settings: LayerShellSettings {
            anchor: Anchor::empty(),
            layer: Layer::Overlay,
            exclusive_zone: 0,
            size: Some(size),
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            start_mode: StartMode::Active,
            events_transparent: false,
        },
        fonts,
        default_font,
        default_text_size,
        antialiasing,
        virtual_keyboard_support,
    };

    iced_layershell::build_pattern::application("leaper", App::update, App::view)
        .settings(settings)
        .theme(App::theme)
        .subscription(App::subscription)
        .run_with(move || App::new(project_dirs, mode))?;

    Ok(())
}

fn init_tracing(trace: bool, debug: bool) -> LeaperResult<()> {
    let level = (cfg!(feature = "profile") || trace)
        .then_some("trace")
        .or_else(|| (cfg!(debug_assertions) || debug).then_some("debug"))
        .unwrap_or("info");
    let directives = ["leaper", "leaper_apps", "leaper_db"]
        .map(|target| format!("{target}={level}"))
        .join(",");

    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .pretty()
                .with_span_events(FmtSpan::CLOSE),
        )
        .with(tracing_subscriber::EnvFilter::new(directives));

    #[cfg(feature = "profile")]
    let registry = registry.with(tracing_tracy::TracyLayer::default());

    registry.try_init()?;

    tracing::debug!("Logging initialized!");

    Ok(())
}

#[macros::lerror]
#[lerr(prefix = "[leaper]", result_name = LeaperResult)]
enum LeaperError {
    #[lerr(str = "No ProjectDirs!")]
    NoProjectDirs,

    #[lerr(str = "[tracing::init] {0}")]
    TracingInit(#[lerr(from, wrap = Arc)] tracing_subscriber::util::TryInitError),

    #[lerr(str = "[iced_layershell] {0}")]
    IcedLayerShell(#[lerr(from, wrap = Arc)] iced_layershell::Error),
}
