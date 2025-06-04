use std::sync::Arc;

use directories::ProjectDirs;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use surrealdb::{Surreal, Uuid, engine::local::Db};
use thiserror::Error;
use uuid::Timestamp;

use crate::err_from_wrapped;

#[derive(Debug, Clone)]
pub struct DB {
    surreal: Surreal<Db>,
}

#[bon::bon]
impl DB {
    pub async fn new(project_dirs: &ProjectDirs) -> DBResult<Self> {
        let data_dir = project_dirs.data_local_dir();
        let db_dir = data_dir.join("db");

        let surreal = Surreal::new(db_dir).await?;
        surreal.use_ns("leaper").await?;

        Ok(Self { surreal })
    }

    #[builder]
    pub async fn table<T>(&self, max: Option<usize>) -> DBResult<Vec<T::Item>>
    where
        T: DBTable,
    {
        let records_table = self.table_records::<T>().maybe_max(max).call().await?;
        let table = records_table.into_iter().map(|r| r.data).collect();

        Ok(table)
    }

    pub async fn table_item<T>(&self, id: Uuid) -> DBResult<T::Item>
    where
        T: DBTable,
    {
        self.surreal.use_db(T::DB::NAME).await?;
        self.surreal
            .select((T::NAME, id))
            .await?
            .failed_to_get(T::Item::NAME, id)
    }

    pub async fn table_only_item<T, I>(&self) -> DBResult<T::Item>
    where
        T: DBTable<Item = I>,
        I: DBTableOnlyItem<T>,
    {
        self.surreal.use_db(T::DB::NAME).await?;

        Ok(self
            .surreal
            .select(I::ID)
            .await?
            .unwrap_or(Default::default()))
    }

    #[builder]
    pub async fn table_records<T>(&self, max: Option<usize>) -> DBResult<Vec<DBRecord<T>>>
    where
        T: DBTable,
    {
        self.surreal.use_db(T::DB::NAME).await?;
        let mut records_table = self.surreal.select(T::NAME).await?;

        records_table.sort_by_key(|r: &DBRecord<T>| r.timestamp().to_unix());

        if let Some(max) = max {
            records_table.truncate(max);
        }

        Ok(records_table)
    }

    pub async fn reset_table<T>(&self) -> DBResult<()>
    where
        T: DBTable,
    {
        self.surreal.use_db(T::DB::NAME).await?;
        self.surreal.delete::<Vec<T::Item>>(T::NAME).await?;

        Ok(())
    }

    pub async fn update_table<T>(&self, values: impl IntoIterator<Item = T::Item>) -> DBResult<()>
    where
        T: DBTable,
    {
        self.reset_table::<T>().await?;
        self.surreal
            .insert::<Vec<T::Item>>(T::NAME)
            .content(values.into_iter().collect::<Vec<_>>())
            .await?;

        Ok(())
    }

    pub async fn set_table_only_item<T, I>(&self, item: I) -> DBResult<()>
    where
        T: DBTable<Item = I>,
        I: DBTableOnlyItem<T>,
    {
        self.surreal.use_db(T::DB::NAME).await?;
        self.surreal
            .upsert::<Option<I>>(I::ID)
            .content(item)
            .await?;

        Ok(())
    }

    pub async fn add_table_item<T>(&self, item: T::Item) -> DBResult<()>
    where
        T: DBTable,
    {
        self.surreal.use_db(T::DB::NAME).await?;
        self.surreal
            .create((T::NAME, Uuid::now_v7()))
            .content(item)
            .await?
            .failed_to_insert(T::Item::NAME)
    }

    pub async fn remove_table_item<T>(&self, id: Uuid) -> DBResult<()>
    where
        T: DBTable,
    {
        self.surreal.use_db(T::DB::NAME).await?;
        self.surreal
            .delete::<Option<T::Item>>((T::NAME, id))
            .await?;

        Ok(())
    }
}

#[macro_export]
macro_rules! db_env {
    ($name:ident {$(
        $table:ident = $item:ty [$item_name:literal $(($only:ident))?]
    ),+ $(,)?}) => {
        pastey::paste! {
            pub struct [< $name DB >];

            impl $crate::state::db::DBEnv for [< $name DB >] {
                const NAME: &'static str = stringify!([< $name:snake >]);
            }

            $(
                pub struct [< $table DBTable >];

                impl $crate::state::db::DBTable for [< $table DBTable >] {
                    type DB = [< $name DB >];
                    type Item = $item;

                    const NAME: &'static str = stringify!([< $table:snake >]);
                }

                impl $crate::state::db::DBTableItem<[< $table DBTable >]>
                for $item {
                    const NAME: &'static str = $item_name;
                }

                $($crate::db_env!(@ $only $table $item);)?
            )+
        }
    };
    (@only $table:ident $item:ty) => {
        pastey::paste! {
            impl $crate::state::db::DBTableOnlyItem<[< $table DBTable >]> for $item {}
        }
    }
}

pub trait DBEnv {
    const NAME: &'static str;
}

pub trait DBTable: 'static {
    type DB: DBEnv;
    type Item: DBTableItem<Self>
    where
        Self: Sized;

    const NAME: &'static str;
}

pub trait DBTableItem<T>: Serialize + for<'de> Deserialize<'de> + 'static
where
    T: DBTable<Item = Self>,
{
    const NAME: &'static str;
}

pub trait DBTableOnlyItem<T>: DBTableItem<T> + Default
where
    T: DBTable<Item = Self>,
{
    const ITEM_ID: &'static str = "only";
    const ID: (&'static str, &'static str) = (T::NAME, Self::ITEM_ID);
}

#[derive(Serialize, Deserialize)]
pub struct DBRecord<T>
where
    T: DBTable,
{
    id: Uuid,
    #[serde(flatten)]
    data: T::Item,
}

impl<T> DBRecord<T>
where
    T: DBTable,
{
    pub fn timestamp(&self) -> Timestamp {
        self.id.get_timestamp().unwrap()
    }
}

pub trait DBOpt<T> {
    fn failed_to_get(self, what: impl std::fmt::Display, id: impl std::fmt::Display)
    -> DBResult<T>;
    fn failed_to_insert(self, what: impl std::fmt::Display) -> DBResult<T>;
}

impl<T> DBOpt<T> for Option<T> {
    fn failed_to_get(
        self,
        what: impl std::fmt::Display,
        id: impl std::fmt::Display,
    ) -> DBResult<T> {
        self.ok_or_else(|| DBError::FailedToGet {
            what: what.to_string(),
            id: id.to_string(),
        })
    }

    fn failed_to_insert(self, what: impl std::fmt::Display) -> DBResult<T> {
        self.ok_or_else(|| DBError::FailedToInsert(what.to_string()))
    }
}

pub type DBResult<T> = Result<T, DBError>;

#[derive(Debug, Clone, Error, Diagnostic)]
pub enum DBError {
    #[error("[db] [surrealdb] {0}")]
    Surreal(Arc<surrealdb::Error>),

    #[error("[db] Failed to insert {0}")]
    FailedToInsert(String),

    #[error("[db] Failed to get {what} with id {id}")]
    FailedToGet { what: String, id: String },
}

err_from_wrapped!(DBError {
    Surreal: surrealdb::Error[Arc]
});
