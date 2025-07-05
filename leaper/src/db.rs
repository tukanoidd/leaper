pub mod apps;
pub mod fs;
pub mod queries;

use std::sync::Arc;

#[cfg(not(feature = "db-websocket"))]
use directories::ProjectDirs;

use macros::lerror;
use surrealdb::{
    Surreal,
    opt::{Config, capabilities::Capabilities},
};
use surrealdb_extras::{SurrealTableInfo, use_ns_db};

use crate::{
    LeaperError, LeaperResult,
    db::{
        apps::{AppEntry, AppIcon},
        fs::{Directory, FSNode, File, Symlink},
    },
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

    let connection = DB::new::<Schema>((
        endpoint,
        Config::default()
            .capabilities(Capabilities::all().with_all_experimental_features_allowed())
            .strict(),
    ));
    let db = use_ns_db(
        connection,
        "leaper",
        "data",
        vec![
            // FS
            FSNode::register(),
            Directory::register(),
            File::register(),
            Symlink::register(),
            // Apps & Icons
            AppEntry::register(),
            AppIcon::register(),
        ]
        .into_iter()
        .map(|res| res.map_err(LeaperError::SurrealExtra))
        .collect::<LeaperResult<Vec<_>>>()?,
    )
    .await?;

    Ok(Arc::new(db))
}

#[lerror]
#[lerr(prefix = "[leaper::db]", result_name = DBResult)]
pub enum DBError {
    #[lerr(str = "[surrealdb] {0}")]
    Surreal(#[lerr(from, wrap = Arc)] surrealdb::Error),
}
