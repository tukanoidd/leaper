mod cli;
mod ui;

use clap::Parser;
use directories::ProjectDirs;
use iced_layershell::{
    Application,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::{LayerShellSettings, StartMode},
};
use miette::Diagnostic;
use thiserror::Error;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::cli::Cli;

fn main() -> LeaperResult<()> {
    miette::set_panic_hook();

    let cli = Cli::parse();

    init_logging()?;

    let project_dirs =
        ProjectDirs::from("com", "tukanoid", "leaper").ok_or(LeaperError::ProjectDirsNotFound)?;

    let iced_layershell::settings::Settings {
        fonts,
        default_font,
        default_text_size,
        antialiasing,
        virtual_keyboard_support,
        ..
    } = iced_layershell::settings::Settings::<()>::default();

    ui::app::App::run(iced_layershell::settings::Settings {
        id: Some("com.tukanoid.leaper".into()),
        layer_settings: LayerShellSettings {
            anchor: Anchor::Left | Anchor::Top | Anchor::Right | Anchor::Bottom,
            layer: Layer::Overlay,
            exclusive_zone: 0,
            size: None,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            start_mode: StartMode::Active,
            ..Default::default()
        },
        flags: ui::app::AppFlags { cli, project_dirs },
        fonts,
        default_font,
        default_text_size,
        antialiasing,
        virtual_keyboard_support,
    })?;

    Ok(())
}

fn init_logging() -> LeaperResult<()> {
    let level = format!(
        "leaper={}",
        match cfg!(debug_assertions) {
            true => "debug",
            false => "info",
        }
    );

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().pretty())
        .with(tracing_subscriber::EnvFilter::new(level))
        .try_init()?;

    Ok(())
}

type LeaperResult<T> = Result<T, LeaperError>;

#[derive(Debug, Error, Diagnostic)]
enum LeaperError {
    #[error("[leaper] [tracing_subscriber::try_init] {0}")]
    TracingInit(#[from] tracing_subscriber::util::TryInitError),

    #[error("[leaper] Failed to get project directories")]
    ProjectDirsNotFound,

    #[error("[leaper] [iced_layershell] {0}")]
    IcedLayershell(#[from] iced_layershell::Error),
}
