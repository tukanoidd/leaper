use color_eyre::Result;
use tracing_subscriber::prelude::*;

pub fn init_tracing(trace: bool, debug: bool, error: bool) -> Result<()> {
    let level = error
        .then_some("error")
        .or_else(|| (cfg!(feature = "profile") || trace).then_some("trace"))
        .or_else(|| (cfg!(debug_assertions) || debug).then_some("debug"))
        .unwrap_or("info");
    let directives = ["leaper", "leaper-daemon"]
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
