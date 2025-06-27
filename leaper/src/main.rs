mod app;
mod cli;
mod config;

use std::sync::Arc;

use clap::Parser;
use directories::ProjectDirs;
use iced_aw::iced_fonts::REQUIRED_FONT_BYTES;
use iced_fonts::NERD_FONT_BYTES;
use iced_layershell::{
    build_pattern::MainSettings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::{LayerShellSettings, Settings, StartMode},
};
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{app::App, cli::Cli, config::Config};

fn main() -> LeaperResult<()> {
    miette::set_panic_hook();

    let Cli { mode, trace, debug } = Cli::parse();

    init_tracing(trace, debug)?;

    let project_dirs =
        ProjectDirs::from("com", "tukanoid", "leaper").ok_or(LeaperError::NoProjectDirs)?;

    let config = Config::open(&project_dirs)?;

    let Settings {
        fonts,
        default_font,
        default_text_size,
        antialiasing,
        virtual_keyboard_support,
        ..
    } = Settings::<()>::default();

    let size = match mode {
        cli::AppMode::Apps => Some((500, 800)),
        cli::AppMode::Runner => Some((600, 100)),
        cli::AppMode::Power => None,
    };
    let anchor = match mode {
        cli::AppMode::Apps | cli::AppMode::Runner => Anchor::empty(),
        cli::AppMode::Power => Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
    };
    let exclusive_zone = match mode {
        cli::AppMode::Apps | cli::AppMode::Runner => 0,
        cli::AppMode::Power => -1,
    };

    let settings = MainSettings {
        id: Some("com.tukanoid.leaper".into()),
        layer_settings: LayerShellSettings {
            anchor,
            layer: Layer::Overlay,
            exclusive_zone,
            size,
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
        .font(REQUIRED_FONT_BYTES)
        .font(NERD_FONT_BYTES)
        .run_with(move || {
            App::builder()
                .project_dirs(project_dirs)
                .config(config)
                .mode(mode)
                .build()
        })?;

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
    #[lerr(str = "Empty cmd args list for action {0}")]
    ActionCMDEmpty(String),
    #[lerr(str = "No dbus connection!")]
    NoDBusConnection,

    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),

    #[lerr(str = "[toml::de] {0}")]
    TomlDeser(#[lerr(from)] toml::de::Error),
    #[lerr(str = "[toml::ser] {0}")]
    TomlSer(#[lerr(from)] toml::ser::Error),

    #[lerr(str = "[tracing::init] {0}")]
    TracingInit(#[lerr(from, wrap = Arc)] tracing_subscriber::util::TryInitError),

    #[lerr(str = "[iced_layershell] {0}")]
    IcedLayerShell(#[lerr(from, wrap = Arc)] iced_layershell::Error),

    #[lerr(str = "Failed to connect to session bus: {0}")]
    ZBus(#[lerr(from)] zbus::Error),
}
