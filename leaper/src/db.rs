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
use surrealdb_extras::{SurrealExt, SurrealQuery, SurrealTableInfo};
use tracing::{Instrument, debug_span};

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
) -> LeaperResult<DB> {
    #[cfg(feature = "db-websocket")]
    let endpoint = "localhost:8000";

    #[cfg(not(feature = "db-websocket"))]
    let endpoint = project_dirs.data_local_dir().join("db");

    let db = Surreal::new((
        endpoint,
        Config::default()
            .capabilities(Capabilities::all().with_all_experimental_features_allowed())
            .strict(),
    ))
    .await?;
    db.use_ns_db_checked(
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

    Ok(db)
}

pub trait InstrumentedSurrealQuery: SurrealQuery {
    async fn instrumented_execute(self, db: DB) -> Result<Self::Output, Self::Error>;
}

impl<Q> InstrumentedSurrealQuery for Q
where
    Q: SurrealQuery,
    Q::Error: std::fmt::Display,
{
    async fn instrumented_execute(self, db: DB) -> Result<Self::Output, Self::Error> {
        self.execute(db)
            .instrument(debug_span!("Calling query", query = Self::QUERY_STR))
            .await
            .inspect_err(|err| tracing::error!("{err}"))
    }
}

#[lerror]
#[lerr(prefix = "[leaper::db]", result_name = DBResult)]
pub enum DBError {
    #[lerr(str = "[surrealdb] {0}")]
    Surreal(#[lerr(from, wrap = Arc)] surrealdb::Error),
}
