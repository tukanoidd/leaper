pub mod apps;
pub mod fs;
pub mod queries;

use std::{path::PathBuf, sync::Arc};

#[cfg(not(feature = "websocket"))]
use directories::ProjectDirs;

use macros::lerror;
use surrealdb::{
    Surreal,
    opt::{Config, capabilities::Capabilities},
};
use surrealdb_extras::{SurrealExt, SurrealQuery, SurrealTableInfo};

use crate::{
    apps::{AppEntry, AppIcon},
    fs::{Directory, FSNode, File, Symlink},
};

#[cfg(not(feature = "websocket"))]
pub type Db = surrealdb::engine::local::Db;
#[cfg(not(feature = "websocket"))]
pub type Scheme = surrealdb::engine::local::RocksDb;

#[cfg(feature = "websocket")]
pub type Db = surrealdb::engine::remote::ws::Client;
#[cfg(feature = "websocket")]
pub type Scheme = surrealdb::engine::remove::ws::Ws;

pub type DB = Surreal<Db>;
pub type DBNotification<T> = surrealdb::Notification<T>;
pub type DBAction = surrealdb::value::Action;

pub async fn init_db(#[cfg(not(feature = "websocket"))] project_dirs: ProjectDirs) -> DBResult<DB> {
    #[cfg(feature = "websocket")]
    let endpoint = "localhost:8000";

    #[cfg(not(feature = "websocket"))]
    let endpoint = project_dirs.data_local_dir().join("db");

    let db = DB::new::<Scheme>((
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
        .map(|res| res.map_err(DBError::SurrealExtra))
        .collect::<DBResult<Vec<_>>>()?,
    )
    .await?;

    Ok(db)
}

pub trait InstrumentedDBQuery: SurrealQuery {
    fn instrumented_execute(
        self,
        db: DB,
    ) -> impl std::future::Future<Output = Result<Self::Output, Self::Error>>;
}

impl<Q> InstrumentedDBQuery for Q
where
    Q: SurrealQuery + std::fmt::Debug,
    Q::Error: std::fmt::Display,
{
    #[tracing::instrument(skip(db), fields(QUERY_STR = Q::QUERY_STR), level = "debug", name = "db::intrumented_execute")]
    async fn instrumented_execute(self, db: DB) -> Result<Self::Output, Self::Error> {
        self.execute(db)
            .await
            .inspect_err(|err| tracing::error!("{err}"))
    }
}

#[lerror]
#[lerr(prefix = "[leaper-db]", result_name = DBResult)]
pub enum DBError {
    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),

    #[lerr(str = "[vfs] {0}")]
    VFS(#[lerr(from, wrap = Arc)] vfs::VfsError),

    #[lerr(str = "[tokio::task::join] {0}")]
    Join(#[lerr(from, wrap = Arc)] tokio::task::JoinError),

    #[lerr(str = "[surrealdb] {0}")]
    Surreal(#[lerr(from, wrap = Arc)] surrealdb::Error),
    #[lerr(str = "[surrealdb_extras] {0}")]
    SurrealExtra(String),

    #[lerr(str = "{0:?} provides no name!")]
    DesktopEntryNoName(PathBuf),
    #[lerr(str = "{0:?} provides no exec!")]
    DesktopEntryNoExec(PathBuf),
    #[lerr(str = "Failed to parse exec '{1}' from {0:?}!")]
    DesktopEntryParseExec(PathBuf, String),

    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::DecodeError),
    #[lerr(str = "[.desktop::exec] {0}")]
    DesktopEntryExec(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::ExecError),

    #[lerr(str = "Interrupted by parent")]
    InterruptedByParent,
    #[lerr(str = "Lost connection to the parent")]
    LostConnectionToParent,
}
