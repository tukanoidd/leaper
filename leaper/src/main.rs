mod cli;

use clap::Parser;
use color_eyre::Result;
use mode::{LeaperMode, LeaperModeMultiWindow};

fn main() -> Result<()> {
    use crate::cli::Cli;

    color_eyre::install()?;

    let Cli {
        mode,
        trace,
        debug,
        error,
    } = Cli::parse();

    leaper_tracing::init_tracing(trace, debug, error)?;

    match mode {
        cli::AppMode::Launcher => launcher::LeaperLauncher::run()?,
        cli::AppMode::Runner => runner::LeaperRunner::run()?,
        cli::AppMode::Power => power::LeaperPower::run()?,
        cli::AppMode::Lock => lock::LeaperLock::run()?,
    }

    Ok(())
}
