use clap::{Parser, ValueEnum};

#[derive(Parser)]
pub struct Cli {
    #[arg(default_value_t = LeaperMode::Apps, value_enum)]
    pub mode: LeaperMode,
}

#[derive(Clone, ValueEnum)]
pub enum LeaperMode {
    Apps,
    Finder,
}
