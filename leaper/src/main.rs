mod app;
mod cli;

use clap::Parser;
use color_eyre::Result;
use mode::LeaperMode;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    use crate::cli::Cli;

    color_eyre::install()?;

    let Cli {
        mode,
        trace,
        debug,
        error,
    } = Cli::parse();

    init_tracing(trace, debug, error)?;

    match mode {
        cli::AppMode::Launcher => launcher::LeaperLauncher::run()?,
        cli::AppMode::Runner => runner::LeaperRunner::run()?,
        cli::AppMode::Power => power::LeaperPower::run()?,
    }

    Ok(())
}

fn init_tracing(trace: bool, debug: bool, error: bool) -> Result<()> {
    let level = error
        .then_some("error")
        .or_else(|| (cfg!(feature = "profile") || trace).then_some("trace"))
        .or_else(|| (cfg!(debug_assertions) || debug).then_some("debug"))
        .unwrap_or("info");
    let directives = ["leaper"]
        .map(|target| format!("{target}={level}"))
        .join(",");

    #[cfg(not(feature = "profile"))]
    let layer = tracing_subscriber::fmt::layer().pretty();

    #[cfg(feature = "profile")]
    let layer = tracing_tracy::TracyLayer::default();

    let registry = tracing_subscriber::registry()
        .with(layer)
        .with(tracing_subscriber::EnvFilter::new(directives));

    registry.try_init()?;

    tracing::debug!("Logging initialized!");

    Ok(())
}
