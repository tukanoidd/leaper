use std::sync::Arc;

#[cfg(not(feature = "db-websocket"))]
use directories::ProjectDirs;

use surrealdb::{Surreal, opt::Config};
use surrealdb_extras::{SurrealTableInfo, use_ns_db};

use crate::{
    LeaperError, LeaperResult,
    app::mode::apps::search::{AppEntry, AppIcon},
};

#[cfg(not(feature = "db-websocket"))]
pub type Db = surrealdb::engine::local::Db;

#[cfg(not(feature = "db-websocket"))]
pub type Schema = surrealdb::engine::local::SurrealKv;

#[cfg(feature = "db-websocket")]
pub type Db = surrealdb::engine::remote::ws::Client;

#[cfg(feature = "db-websocket")]
pub type Schema = surrealdb::engine::remote::ws::Ws;

pub type DB = Surreal<Db>;

pub async fn init_db(
    #[cfg(not(feature = "db-websocket"))] project_dirs: ProjectDirs,
) -> LeaperResult<Arc<DB>> {
    #[cfg(feature = "db-websocket")]
    let endpoint = "localhost:8000";

    #[cfg(not(feature = "db-websocket"))]
    let endpoint = project_dirs.data_local_dir().join("db");

    let connection = DB::new::<Schema>((endpoint, Config::default().strict()));
    let db = use_ns_db(
        connection,
        "leaper",
        "data",
        vec![
            AppEntry::register().map_err(LeaperError::SurrealExtra)?,
            AppIcon::register().map_err(LeaperError::SurrealExtra)?,
        ],
    )
    .await?;

    Ok(Arc::new(db))
}
