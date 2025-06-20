use std::{fmt::Debug, sync::Arc};

use futures::{FutureExt, StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use surrealdb::{Surreal, Uuid, engine::local::Db, method::Stream, opt::IntoEndpoint};
use uuid::Timestamp;

pub use serde;

pub use macros::db_entry;

pub type DBEntryId = surrealdb::RecordId;

pub const NAMESPACE: &str = "leaper";

#[derive(Debug, Clone)]
pub struct DB {
    db: Surreal<Db>,
}

impl DB {
    #[tracing::instrument(level = "trace")]
    pub async fn init<P>(
        endpoint: impl IntoEndpoint<P, Client = Db> + std::fmt::Debug,
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
    pub async fn get_table<E>(&self) -> DBResult<Vec<E>>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        Ok(self.db.select(E::Table::NAME).await?)
    }

    #[tracing::instrument(skip(self), level = "trace")]
    pub async fn live_table<E>(&self) -> DBResult<Stream<Vec<E>>>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        Ok(self.db.select(E::Table::NAME).live().await?)
    }

    #[tracing::instrument(
        skip(self, table),
        level = "trace",
        fields(
            db_name = E::Table::DB_NAME,
            name = E::Table::NAME
        )
    )]
    pub async fn set_table<E, I>(&self, table: I) -> DBResult<Vec<E>>
    where
        E: TDBTableEntry + 'static,
        I: IntoIterator<Item = E>,
    {
        self.use_db::<E::Table>().await?;
        self.db.delete::<Vec<E>>(E::Table::NAME).await?;

        let list = table.into_iter().collect::<Vec<_>>();

        list.clone()
            .into_iter()
            .map(|entry| {
                let sc = self.clone();

                async move { sc.new_entry::<E>(entry.clone()).await }.boxed()
            })
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<DBResult<Vec<_>>>()?;

        Ok(list)
    }

    #[tracing::instrument(skip(self), level = "trace")]
    pub async fn clear_table<E>(&self) -> DBResult<Vec<E>>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        Ok(self.db.delete(E::Table::NAME).await?)
    }

    #[tracing::instrument(skip(self), level = "trace")]
    pub async fn entry<E>(&self, id: Uuid) -> DBResult<E>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        self.db.select((E::Table::NAME, id)).await?.or_not_found(id)
    }

    #[tracing::instrument(skip(self), level = "trace")]
    pub async fn new_entry<E>(&self, val: impl Into<E> + Debug) -> DBResult<E>
    where
        E: TDBTableEntry + 'static,
    {
        let val = val.into();
        let id = DBEntryId::new_timestamped::<E::Table>();

        self.use_db::<E::Table>().await?;
        self.db
            .create(E::Table::NAME)
            .content(val)
            .await?
            .or_failed_to_add(id.uuid())
    }

    #[tracing::instrument(skip(self), level = "trace")]
    pub async fn remove_entry<E>(&self, id: Uuid) -> DBResult<E>
    where
        E: TDBTableEntry,
    {
        self.use_db::<E::Table>().await?;
        self.db.delete((E::Table::NAME, id)).await?.or_not_found(id)
    }

    #[tracing::instrument(skip(self), level = "trace")]
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
    Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de> + Send + Sync
{
    type Table: DBTable;
}

#[macros::lerror]
#[lerr(prefix = "[db]", result_name = DBResult)]
pub enum DBError {
    #[lerr(str = "[surrealdb] {0}")]
    Surreal(#[lerr(from, wrap = Arc)] surrealdb::Error),

    #[lerr(str = "Entry {id} from {db}::{table} was not found!")]
    NotFound { db: String, table: String, id: Uuid },
    #[lerr(str = "Entry {id} from {db}::{table} could not be added!")]
    FailedToAdd { db: String, table: String, id: Uuid },
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
