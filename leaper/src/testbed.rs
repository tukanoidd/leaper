use clap::Parser;

/// Leaper testbed
#[derive(Parser)]
#[command(author, version, about, long_about)]
pub struct TestbedCli {
    #[arg(long)]
    pub trace: bool,
    #[arg(long)]
    pub debug: bool,
    #[arg(long)]
    pub error: bool,
}
