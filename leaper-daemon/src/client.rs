use color_eyre::eyre::Result;
use tarpc::{client::Config, tokio_serde::formats::Bincode};

pub use tarpc::context;

use crate::{ADDRESS, LeaperDaemonClient};

pub async fn connect() -> Result<LeaperDaemonClient> {
    let mut transport = tarpc::serde_transport::tcp::connect(ADDRESS, Bincode::default);
    transport.config_mut().max_frame_length(usize::MAX);

    let transport = transport.await?;
    let client = LeaperDaemonClient::new(Config::default(), transport).spawn();

    Ok(client)
}
