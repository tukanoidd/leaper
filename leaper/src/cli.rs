use clap::{Parser, ValueEnum};

/// A Launcher/Command Runner
#[derive(Parser)]
#[command(author, version, about, long_about = "None")]
pub struct Cli {
    #[arg(value_enum, default_value_t = Default::default())]
    pub mode: AppMode,

    #[arg(long)]
    pub trace: bool,
    #[arg(long)]
    pub debug: bool,
    #[arg(long)]
    pub error: bool,
}

#[derive(Default, Clone, Copy, ValueEnum)]
pub enum AppMode {
    #[default]
    Launcher,
    Runner,
    Power,
    Lock,
}
