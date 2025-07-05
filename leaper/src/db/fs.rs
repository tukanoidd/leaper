use std::{path::PathBuf, sync::Arc};

use async_walkdir::Filtering;
use futures::StreamExt;
use macros::{db_table, sql};
use surrealdb::RecordId;
use tokio::task::JoinSet;

use crate::db::DB;

#[tracing::instrument(skip(db), level = "debug")]
pub async fn index(db: Arc<DB>) -> FSResult<()> {
    let db_clone = db.clone();
    let mut walkdir = async_walkdir::WalkDir::new("/").filter(move |entry| {
        let db = db_clone.clone();

        async move {
            if find_node_by_path(entry.path(), db)
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

async fn find_node_by_path(path: PathBuf, db: Arc<DB>) -> FSResult<Option<RecordId>> {
    Ok(db
        .query(sql!(
            "SELECT VALUE id FROM ONLY fs_nodes WHERE path = $path LIMIT 1"
        ))
        .bind(("path", path))
        .await?
        .take(0)?)
}

#[tracing::instrument(skip(db), level = "trace")]
async fn add_fs_node(path: PathBuf, db: Arc<DB>) -> FSResult<RecordId> {
    if let Some(id) = find_node_by_path(path.clone(), db.clone()).await? {
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

#[tracing::instrument(skip(db), level = "debug")]
async fn add_symlink(path: PathBuf, fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
    let links_to = match path.read_link() {
        Ok(path) => path,
        Err(err) => {
            tracing::trace!("WARN: Failed to read the symlink {path:?}: {err}");
            return Ok(());
        }
    };

    let symlink_id: RecordId = db
        .query(sql!("(CREATE symlinks).id"))
        .await?
        .take::<Option<RecordId>>(0)?
        .expect("Should be able to create a symlink entry here");

    db.query(sql!("RELATE $fs_node->is_symlink->$symlink"))
        .bind(("fs_node", fs_node_id.clone()))
        .bind(("symlink", symlink_id.clone()))
        .await?
        .check()?;

    let symlinked_fs_node: RecordId = Box::pin(add_fs_node(links_to, db.clone())).await?;

    db.query(sql!("RELATE $symlink->is_symlink_of->$symlinked_node"))
        .bind(("symlink", symlink_id))
        .bind(("symlinked_node", symlinked_fs_node))
        .await?
        .check()?;

    Ok(())
}

#[tracing::instrument(skip(db), level = "debug")]
async fn add_dir(fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
    let dir_id: RecordId = db
        .query(sql!("(CREATE directories).id"))
        .await?
        .take::<Option<RecordId>>(0)?
        .expect("Should be able to create a directory entry without trouble here");

    db.query(sql!("RELATE $fs_node->is_dir->$dir_id"))
        .bind(("fs_node", fs_node_id.clone()))
        .bind(("dir_id", dir_id))
        .await?
        .check()?;

    // TODO: checks

    Ok(())
}

#[tracing::instrument(skip(db), level = "debug")]
async fn add_file(fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
    let file_id: RecordId = db
        .query(sql!("(CREATE files).id"))
        .await?
        .take::<Option<RecordId>>(0)?
        .expect("Should be able to create a file entry at this point");

    db.query(sql!("RELATE $fs_node->is_file->$file_id"))
        .bind(("fs_node", fs_node_id))
        .bind(("file_id", file_id))
        .await?
        .check()?;

    Ok(())
}

#[tracing::instrument(skip(db), level = "debug")]
async fn add_parent(path: PathBuf, child_fs_node_id: RecordId, db: Arc<DB>) -> FSResult<RecordId> {
    // Should be fine as we only call this function on parent directories of nodes
    let parent_fs_node_id: RecordId = Box::pin(add_fs_node(path, db.clone())).await?;

    db.query(sql!("RELATE $parent->is_parent_of->$child"))
        .bind(("parent", parent_fs_node_id.clone()))
        .bind(("child", child_fs_node_id))
        .await?
        .check()?;

    Ok(parent_fs_node_id)
}

#[db_table]
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

#[db_table]
#[table(
    db = directories,
)]
pub struct Directory {
    pub id: RecordId,
}

#[db_table]
#[table(
    db = files,
)]
pub struct File {
    id: RecordId,
}

#[db_table]
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
}
