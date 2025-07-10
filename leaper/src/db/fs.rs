use std::{path::PathBuf, sync::Arc};

use async_walkdir::Filtering;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;
use surrealdb_extras::{SurrealQuery, SurrealTable};
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
                    if let Err(err) = FSNode::add_db(entry.path(), db).await {
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

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = fs_node,
    sql(
        "DEFINE TABLE is_symlink TYPE RELATION",
        "DEFINE TABLE is_dir TYPE RELATION",


        "DEFINE TABLE is_parent_of TYPE RELATION"
    )
)]
pub struct FSNode {
    pub id: RecordId,
    pub path: PathBuf,
    pub name: String,
}

impl FSNode {
    #[tracing::instrument(skip(db), level = "debug")]
    async fn add_db(path: PathBuf, db: Arc<DB>) -> FSResult<RecordId> {
        if let Some(id) = FindNodeByPathQuery::builder()
            .path(&path)
            .build()
            .execute(db.clone())
            .await?
        {
            return Ok(id.clone());
        }

        let fs_node_id = CreateFsNodeQuery::builder()
            .path(path.clone())
            .build()
            .execute(db.clone())
            .await?
            .expect("Should be able to create an fs node here");

        if path.is_symlink() {
            Symlink::add_db(path.clone(), fs_node_id.clone(), db.clone()).await?;
        } else if path.is_dir() {
            Directory::add_db(fs_node_id.clone(), db.clone()).await?;
        } else if path.is_file() {
            File::add_db(path.clone(), fs_node_id.clone(), db.clone()).await?;
        }

        if let Some(parent) = path.parent() {
            Self::add_parent(parent.to_path_buf(), fs_node_id.clone(), db).await?;
        }

        Ok(fs_node_id)
    }

    #[tracing::instrument(skip(db), level = "trace")]
    async fn add_parent(
        path: PathBuf,
        child_fs_node_id: RecordId,
        db: Arc<DB>,
    ) -> FSResult<RecordId> {
        // Should be fine as we only call this function on parent directories of nodes
        let parent_fs_node_id: RecordId = Box::pin(FSNode::add_db(path, db.clone())).await?;

        RelateQuery::builder()
            .in_(parent_fs_node_id.clone())
            .table("is_parent_of")
            .out(child_fs_node_id)
            .build()
            .execute(db)
            .await?;

        Ok(parent_fs_node_id)
    }
}

#[derive(bon::Builder, SurrealQuery)]
#[query(
    output = "Option<RecordId>",
    error = FSError,
    sql = "SELECT VALUE id FROM ONLY fs_node WHERE path = {path} LIMIT 1"
)]
struct FindNodeByPathQuery {
    #[builder(into)]
    pub path: PathBuf,
}

#[derive(SurrealQuery)]
#[query(
    output = "Option<RecordId>",
    error = FSError,
    sql = "(CREATE fs_node SET path = {path}, name = {name}).id"
)]
struct CreateFsNodeQuery {
    path: PathBuf,
    name: String,
}

#[bon::bon]
impl CreateFsNodeQuery {
    #[builder]
    fn new(path: PathBuf) -> Self {
        let name: String = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("[ERROR]")
            .into();

        Self { path, name }
    }
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = directory,
)]
pub struct Directory {
    pub id: RecordId,
}

impl Directory {
    #[tracing::instrument(skip(db), level = "trace")]
    async fn add_db(fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
        let dir_id = CreateEmptyIdQuery::builder()
            .table("directory")
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
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = file,
    sql(
        "DEFINE TABLE is_file TYPE RELATION",
        "
        DEFINE EVENT icon_file_added ON TABLE is_file
            WHEN $event = \"CREATE\"
                && array::any([
                    'png',
                    'jpg', 'jpeg',
                    'gif', 'webp',
                    'pbm', 'pam', 'ppm', 'pgm',
                    'tiff', 'tif',
                    'tga', 'dds', 'bmp', 'ico', 'hdr', 'exr', 'ff', 'avif',
                    'qoi', 'pcx', 'svg', 'xpm'
                ], $value.out.ext)
            THEN {
                LET $fs_node = $value.in;
                LET $file = $value.out;
                LET $icon = CREATE icon SET
                    name = $fs_node.name,
                    path = $fs_node.path,
                    svg = ($value.ext == 'svg'),
                    xpm = ($value.ext == 'xpm');
                RELATE $file->is_icon->$icon;
            }
        ",
        "DEFINE TABLE is_icon TYPE RELATION"
    )
)]
pub struct File {
    id: RecordId,
    ext: Option<String>,
}

impl File {
    #[tracing::instrument(skip(db), level = "trace")]
    async fn add_db(path: PathBuf, fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
        CreateFileQuery::builder()
            .fs_node(fs_node_id)
            .maybe_ext(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_owned()),
            )
            .build()
            .execute(db.clone())
            .await?;

        Ok(())
    }
}

#[derive(bon::Builder, SurrealQuery)]
#[query(
    check,
    error = FSError,
    sql = "
        LET $file = (CREATE file SET ext = {ext});
        RELATE {fs_node}->is_file->$file;
        RETURN $file.id
    "
)]
struct CreateFileQuery {
    fs_node: RecordId,
    #[builder(into)]
    ext: Option<String>,
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = symlink,
    sql("DEFINE TABLE is_symlink_of TYPE RELATION")
)]
pub struct Symlink {
    id: RecordId,
}

impl Symlink {
    #[tracing::instrument(skip(db), level = "trace")]
    async fn add_db(path: PathBuf, fs_node_id: RecordId, db: Arc<DB>) -> FSResult<()> {
        let links_to = match path.read_link() {
            Ok(path) => path,
            Err(err) => {
                tracing::trace!("WARN: Failed to read the symlink {path:?}: {err}");
                return Ok(());
            }
        };

        let symlink_id = CreateEmptyIdQuery::builder()
            .table("symlink")
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

        let symlinked_fs_node: RecordId = Box::pin(FSNode::add_db(links_to, db.clone())).await?;

        RelateQuery::builder()
            .in_(symlink_id)
            .table("is_symlink_of")
            .out(symlinked_fs_node)
            .build()
            .execute(db)
            .await?;

        Ok(())
    }
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
