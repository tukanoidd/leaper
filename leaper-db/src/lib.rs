use std::{
    fmt::{Debug, Display},
    sync::Arc,
};

use derive_more::Deref;
use futures::Stream;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use surrealdb::{Surreal, Uuid, method::QueryStream, opt::IntoEndpoint};
use uuid::Timestamp;

pub use serde;

pub use macros::db_entry;

#[cfg(not(feature = "websocket"))]
type Db = surrealdb::engine::local::Db;

#[cfg(not(feature = "websocket"))]
type Scheme = surrealdb::engine::local::SurrealKv;

#[cfg(feature = "websocket")]
type Db = surrealdb::engine::remote::ws::Client;

#[cfg(feature = "websocket")]
type Scheme = surrealdb::engine::remote::ws::Ws;

pub type DBEntryId = surrealdb::RecordId;
pub type DBNotification<E> = surrealdb::Notification<E>;
pub type DBAction = surrealdb::Action;

pub const NAMESPACE: &str = "leaper";

#[derive(Debug, Clone, Deref)]
pub struct DB {
    db: Surreal<Db>,
}

impl DB {
    #[cfg_attr(feature = "profile", tracing::instrument(level = "trace"))]
    pub async fn init(
        endpoint: impl IntoEndpoint<Scheme, Client = Db> + std::fmt::Debug,
    ) -> DBResult<Self> {
        let db = Surreal::new(endpoint).await?;
        db.use_ns(NAMESPACE).await?;

        Ok(Self { db })
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = T::DB_NAME,
                table_name = T::NAME
            )
        ),
    )]
    async fn use_db<T>(&self) -> DBResult<()>
    where
        T: DBTable,
    {
        self.db.use_db(T::DB_NAME).await?;
        Ok(())
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn create_table<E>(&self) -> DBResult<()>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;

        self.db
            .query(format!("DEFINE TABLE IF NOT EXISTS {}", E::Table::NAME))
            .await?
            .check()?;

        Ok(())
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn get_table<E>(&self) -> DBResult<Vec<E>>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        self.create_table::<E>().await?;

        Ok(self.db.select(E::Table::NAME).await?)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn get_table_fetch<E, S>(
        &self,
        fetch: impl IntoIterator<Item = S>,
    ) -> DBResult<Vec<E>>
    where
        E: TDBTableEntry,
        S: Into<String>,
    {
        self.use_db::<E::Table>().await?;
        self.create_table::<E>().await?;

        let query = format!(
            "SELECT * FROM {} FETCH {}",
            E::Table::NAME,
            fetch
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .join(",")
        );

        tracing::debug!("{query}");

        Ok(self.db.query(query).await?.take(0)?)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn get_table_ids<E>(&self) -> DBResult<Vec<DBEntryId>>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        Ok(self.db.select(E::Table::NAME).await?)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn get_table_field<E, V>(&self, name: impl Display) -> DBResult<Vec<V>>
    where
        E: TDBTableEntry,
        V: DeserializeOwned,
    {
        self.use_db::<E::Table>().await?;
        self.create_table::<E>().await?;

        let name = name.to_string();

        let value: Vec<Map<String, Value>> = self
            .db
            .query(format!("SELECT {name} FROM {}", E::Table::NAME))
            .await?
            .take(0)?;

        let fields = value
            .into_iter()
            .map(|mut x| {
                x.remove(&name)
                    .ok_or_else(|| DBError::FieldNotFound {
                        db: E::Table::DB_NAME.into(),
                        table: E::Table::NAME.into(),
                        field: name.clone(),
                    })
                    .and_then(|val| Ok(serde_json::from_value::<V>(val)?))
            })
            .collect::<DBResult<Vec<_>>>()?;

        Ok(fields)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn live_table<E>(
        &self,
    ) -> DBResult<impl Stream<Item = surrealdb::Result<DBNotification<E>>>>
    where
        E: TDBTableEntry + Unpin,
    {
        self.use_db::<E::Table>().await?;
        self.create_table::<E>().await?;

        Ok(self.db.select(E::Table::NAME).live().await?)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn live_table_fetch<E, S>(
        &self,
        fetch: impl IntoIterator<Item = S>,
    ) -> DBResult<impl Stream<Item = surrealdb::Result<DBNotification<E>>>>
    where
        E: TDBTableEntry + Unpin,
        S: Into<String>,
    {
        self.use_db::<E::Table>().await?;
        self.create_table::<E>().await?;

        let query = format!(
            "LIVE SELECT * FROM {} FETCH {}",
            E::Table::NAME,
            fetch
                .into_iter()
                .map(Into::into)
                .collect::<Vec<_>>()
                .join(",")
        );

        tracing::debug!("{query}");

        let stream: QueryStream<DBNotification<E>> = self.db.query(query).await?.stream(0_usize)?;

        Ok(stream)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn entry<E>(&self, id: Uuid) -> DBResult<E>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        self.db.select((E::Table::NAME, id)).await?.or_not_found(id)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn new_entry<E>(&self, val: impl Into<E> + Debug) -> DBResult<E>
    where
        E: TDBTableEntry + 'static,
    {
        let val = val.into();
        let id = DBEntryId::new_timestamped::<E::Table>();

        self.use_db::<E::Table>().await?;
        self.create_table::<E>().await?;

        self.db
            .create(E::Table::NAME)
            .content(val)
            .await?
            .or_failed_to_add(id.uuid())
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn remove_entry<E>(&self, id: Uuid) -> DBResult<E>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        self.db.delete((E::Table::NAME, id)).await?.or_not_found(id)
    }

    #[cfg_attr(
        feature = "profile",
        tracing::instrument(
            skip(self),
            level = "trace",
            fields(
                db_name = E::Table::DB_NAME,
                name = E::Table::NAME
            )
        )
    )]
    pub async fn update_entry<E>(&self, id: Uuid, val: impl Into<E> + Debug) -> DBResult<E>
    where
        E: TDBTableEntry + 'static,
    {
        self.use_db::<E::Table>().await?;
        self.db
            .update((E::Table::NAME, id))
            .content(val.into())
            .await?
            .or_not_found(id)
    }
}

pub trait DBTable {
    const DB_NAME: &'static str;
    const NAME: &'static str;
}

pub trait TDBEntryId {
    fn new_timestamped<T>() -> Self
    where
        T: DBTable;
    fn uuid(&self) -> Uuid;
    fn timestamp(&self) -> Timestamp;
}

impl TDBEntryId for DBEntryId {
    fn new_timestamped<T>() -> Self
    where
        T: DBTable,
    {
        DBEntryId::from_table_key(T::NAME, Uuid::now_v7())
    }

    fn uuid(&self) -> Uuid {
        Uuid::try_from(self.key().clone()).unwrap()
    }

    fn timestamp(&self) -> Timestamp {
        self.uuid().get_timestamp().unwrap()
    }
}

pub trait TDBTableEntry:
    Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de> + DeserializeOwned + Send + Sync
{
    type Table: DBTable;
}

#[macros::lerror]
#[lerr(prefix = "[db]", result_name = DBResult)]
pub enum DBError {
    #[lerr(str = "[surrealdb] {0}")]
    Surreal(#[lerr(from, wrap = Arc)] surrealdb::Error),
    #[lerr(str = "[serde_json] {0}")]
    JSON(#[lerr(from, wrap = Arc)] serde_json::Error),

    #[lerr(str = "Entry {id} from {db}::{table} was not found!")]
    NotFound { db: String, table: String, id: Uuid },
    #[lerr(str = "Entry {id} from {db}::{table} could not be added!")]
    FailedToAdd { db: String, table: String, id: Uuid },
    #[lerr(str = "Entry from {db}::{table} doesn't have a field {field:?}")]
    FieldNotFound {
        db: String,
        table: String,
        field: String,
    },
}

pub trait DBOptionExt<E>
where
    E: TDBTableEntry,
{
    fn or_not_found(self, id: Uuid) -> DBResult<E>;
    fn or_failed_to_add(self, id: Uuid) -> DBResult<E>;
}

impl<E> DBOptionExt<E> for Option<E>
where
    E: TDBTableEntry,
{
    fn or_not_found(self, id: Uuid) -> DBResult<E> {
        self.ok_or_else(|| DBError::NotFound {
            db: E::Table::DB_NAME.into(),
            table: E::Table::NAME.into(),
            id,
        })
    }

    fn or_failed_to_add(self, id: Uuid) -> DBResult<E> {
        self.ok_or_else(|| DBError::FailedToAdd {
            db: E::Table::DB_NAME.into(),
            table: E::Table::NAME.into(),
            id,
        })
    }
}
