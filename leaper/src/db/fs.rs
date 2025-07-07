use std::{path::PathBuf, sync::Arc};

use async_walkdir::Filtering;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;
use surrealdb_extras::{SurrealQuery, SurrealTable, sql};
use tokio::task::JoinSet;

use crate::db::{
    DB, DBError,
    queries::{CreateEmptyIdQuery, RelateQuery},
};

#[tracing::instrument(skip(db), level = "trace")]
pub async fn index(root: impl Into<PathBuf> + std::fmt::Debug, db: Arc<DB>) -> FSResult<()> {
    let db_clone = db.clone();
    let mut walkdir = async_walkdir::WalkDir::new(root.into()).filter(move |entry| {
        let db = db_clone.clone();

        async move {
            if FindNodeByPathQuery::builder()
                .path(entry.path())
                .build()
                .execute(db)
                .await
                .ok()
                .map(|x| x.is_some())
                .unwrap_or_default()
            {
                return Filtering::Ignore;
            }

            async_walkdir::Filtering::Continue
        }
    });

    let mut tasks = JoinSet::new();

    while let Some(entry) = walkdir.next().await {
        match entry {
            Ok(entry) => {
                let db = db.clone();

                tasks.spawn(async move {
                    if let Err(err) = add_fs_node(entry.path(), db).await {
                        tracing::error!("Failed to add fs_node: {err}");
                    }
                });
            }
            Err(err) => {
                tracing::trace!("WARN: Failed to get an entry: {err}");
            }
        }
    }

    tasks.join_all().await;

    Ok(())
}

#[derive(bon::Builder, SurrealQuery)]
#[query(output = "Option<RecordId>", error = FSError)]
pub struct FindNodeByPathQuery {
    #[var(sql = "SELECT VALUE id FROM ONLY fs_nodes WHERE path = {} LIMIT 1")]
    #[builder(into)]
    path: PathBuf,
}

#[tracing::instrument(skip(db), level = "debug")]
async fn add_fs_node(path: PathBuf, db: Arc<DB>) -> FSResult<RecordId> {
    if let Some(id) = FindNodeByPathQuery::builder()
        .path(&path)
        .build()
        .execute(db.clone())
        .await?
    {
        return Ok(id.clone());
    }

    let fs_node_id = db
        .query(sql!("(CREATE fs_nodes SET path = $path).id"))
        .bind(("path", path.clone()))
        .await?
        .take::<Option<RecordId>>(0)?
        .expect("Should be able to create an fs node here");

    if path.is_symlink() {
        add_symlink(path.clone(), fs_node_id.clone(), db.clone()).await?;
    } else if path.is_dir() {
        add_dir(fs_node_id.clone(), db.clone()).await?;
    } else if path.is_file() {
        add_file(fs_node_id.clone(), db.clone()).await?;
    }

    if let Some(parent) = path.parent() {
        add_parent(parent.to_path_buf(), fs_node_id.clone(), db).await?;
    }

    Ok(fs_node_id)
}

#[tracing::instrument(skip(db), level = "trace")]
async fn add_symlink(path: PathBuf, fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
    let links_to = match path.read_link() {
        Ok(path) => path,
        Err(err) => {
            tracing::trace!("WARN: Failed to read the symlink {path:?}: {err}");
            return Ok(());
        }
    };

    let symlink_id = CreateEmptyIdQuery::builder()
        .table("symlinks")
        .build()
        .execute(db.clone())
        .await?
        .expect("Should be able to create a symlink entry here");

    RelateQuery::builder()
        .in_(fs_node_id)
        .table("is_symlink")
        .out(symlink_id.clone())
        .build()
        .execute(db.clone())
        .await?;

    let symlinked_fs_node: RecordId = Box::pin(add_fs_node(links_to, db.clone())).await?;

    RelateQuery::builder()
        .in_(symlink_id)
        .table("is_symlink_of")
        .out(symlinked_fs_node)
        .build()
        .execute(db)
        .await?;

    Ok(())
}

#[tracing::instrument(skip(db), level = "trace")]
async fn add_dir(fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
    let dir_id = CreateEmptyIdQuery::builder()
        .table("directories")
        .build()
        .execute(db.clone())
        .await?
        .expect("Should be able to create a dir entry here");

    RelateQuery::builder()
        .in_(fs_node_id)
        .table("is_dir")
        .out(dir_id)
        .build()
        .execute(db)
        .await?;

    // TODO: checks

    Ok(())
}

#[tracing::instrument(skip(db), level = "trace")]
async fn add_file(fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
    let file_id = CreateEmptyIdQuery::builder()
        .table("files")
        .build()
        .execute(db.clone())
        .await?
        .expect("Should be able to create a file entry here");

    RelateQuery::builder()
        .in_(fs_node_id)
        .table("is_file")
        .out(file_id)
        .build()
        .execute(db)
        .await?;

    Ok(())
}

#[tracing::instrument(skip(db), level = "trace")]
async fn add_parent(path: PathBuf, child_fs_node_id: RecordId, db: Arc<DB>) -> FSResult<RecordId> {
    // Should be fine as we only call this function on parent directories of nodes
    let parent_fs_node_id: RecordId = Box::pin(add_fs_node(path, db.clone())).await?;

    RelateQuery::builder()
        .in_(parent_fs_node_id.clone())
        .table("is_parent_of")
        .out(child_fs_node_id)
        .build()
        .execute(db)
        .await?;

    Ok(parent_fs_node_id)
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = fs_nodes,
    sql(
        "DEFINE TABLE is_symlink TYPE RELATION",
        "DEFINE TABLE is_dir TYPE RELATION",
        "DEFINE TABLE is_file TYPE RELATION",

        "DEFINE TABLE is_parent_of TYPE RELATION"
    )
)]
pub struct FSNode {
    pub id: RecordId,
    pub path: PathBuf,
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = directories,
)]
pub struct Directory {
    pub id: RecordId,
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = files,
)]
pub struct File {
    id: RecordId,
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = symlinks,
    sql("DEFINE TABLE is_symlink_of TYPE RELATION")
)]
pub struct Symlink {
    id: RecordId,
}

#[macros::lerror]
#[lerr(prefix = "[leaper::db::fs]", result_name = FSResult)]
pub enum FSError {
    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),

    #[lerr(str = "[surreal] {0}")]
    Surreal(#[lerr(from, wrap = Arc)] surrealdb::Error),

    #[lerr(str = "{0}")]
    DB(#[lerr(from)] DBError),
}
