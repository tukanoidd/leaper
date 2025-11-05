use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use surrealdb::RecordId;
use surrealdb_extras::{SurrealQuery, SurrealTable};

use crate::{DB, DBError, DBResult, InstrumentedDBQuery, queries::RelateQuery};

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

#[bon::bon]
impl FSNode {
    #[builder]
    #[tracing::instrument(skip(db), level = "debug", name = "fs::FSNode::add_db")]
    pub async fn add_db(
        #[builder(into)] path: PathBuf,
        db: DB,
        parents: bool,
    ) -> DBResult<RecordId> {
        if let Some(id) = FindNodeByPathQuery::builder()
            .path(&path)
            .build()
            .instrumented_execute(db.clone())
            .await?
        {
            return Ok(id.clone());
        }

        let fs_node_id = CreateFsNodeQuery::builder()
            .path(path.clone())
            .build()
            .instrumented_execute(db.clone())
            .await?
            .expect("Should be able to create an fs node here");

        if path.is_symlink() {
            Symlink::add_db()
                .path(path.clone())
                .fs_node_id(fs_node_id.clone())
                .db(db.clone())
                .parents(parents)
                .call()
                .await?;
        } else if path.is_dir() {
            Directory::add_db(fs_node_id.clone(), db.clone()).await?;
        } else if path.is_file() {
            File::add_db(path.clone(), fs_node_id.clone(), db.clone()).await?;
        }

        if parents && let Some(parent) = path.parent() {
            Self::add_parent(parent.to_path_buf(), fs_node_id.clone(), db).await?;
        }

        Ok(fs_node_id)
    }

    #[tracing::instrument(
        skip(db, child_fs_node_id),
        level = "debug",
        name = "fs::FSNode::add_parent"
    )]
    async fn add_parent(path: PathBuf, child_fs_node_id: RecordId, db: DB) -> DBResult<RecordId> {
        // Should be fine as we only call this function on parent directories of nodes
        let parent_fs_node_id: RecordId = Box::pin(
            FSNode::add_db()
                .path(path)
                .db(db.clone())
                .parents(true)
                .call(),
        )
        .await?;

        RelateQuery::builder()
            .in_(parent_fs_node_id.clone())
            .table("is_parent_of")
            .out(child_fs_node_id)
            .build()
            .instrumented_execute(db)
            .await?;

        Ok(parent_fs_node_id)
    }
}

#[derive(Debug, bon::Builder, SurrealQuery)]
#[query(
    output = "Option<RecordId>",
    error = DBError,
    sql = "SELECT VALUE id FROM ONLY fs_node WHERE path == {path} LIMIT 1"
)]
struct FindNodeByPathQuery {
    #[builder(into)]
    pub path: PathBuf,
}

#[derive(Debug, SurrealQuery)]
#[query(
    output = "Option<RecordId>",
    error = DBError,
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
    #[tracing::instrument(skip(db), level = "debug", name = "fs::Directory::add_db")]
    async fn add_db(fs_node_id: RecordId, db: DB) -> DBResult<()> {
        CreateDirectoryQuery::builder()
            .fs_node(fs_node_id)
            .build()
            .instrumented_execute(db)
            .await?;

        // TODO: checks

        Ok(())
    }
}

#[derive(Debug, bon::Builder, SurrealQuery)]
#[query(
    check,
    error = DBError,
    sql = "
        BEGIN TRANSACTION;

        LET $dir = (CREATE directory).id;
        RELATE {fs_node}->is_dir->$dir;

        COMMIT TRANSACTION;
    "
)]
struct CreateDirectoryQuery {
    fs_node: RecordId,
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = file,
    sql(
        // fs_nodes->
        "DEFINE TABLE is_file TYPE RELATION",

        // file->
        "DEFINE TABLE is_icon TYPE RELATION",
        "DEFINE TABLE is_app TYPE RELATION",

        "
        DEFINE EVENT icon_file_added ON TABLE is_file
            WHEN $event = 'CREATE'
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
                LET $dims = array::filter(
                    string::split($fs_node.path, '/'), 
                    |$str| string::contains($str, 'x')
                )
                    .map(|$str| string::split($str, 'x'))
                    .filter(|$split| array::len($split) == 2 && array::all($split, |$dim| string::is::numeric($dim)))
                    .map(|$split| {{
                        width: <int>($split[0]),
                        height: <int>($split[0])
                    }})[0];
                LET $icon = (CREATE icon SET
                    name = ($file
                        .stem
                        .replace('-default', '')
                        .replace('-symbolic', '')
                        .replace('-generic', '')),
                    path = $fs_node.path,
                    svg = ($file.ext == 'svg'),
                    xpm = ($file.ext == 'xpm'),
                    dims = $dims).id;
                RELATE $file->is_icon->$icon;
            }
        ",
    )
)]
pub struct File {
    id: RecordId,
    stem: String,
    ext: Option<String>,
}

impl File {
    #[tracing::instrument(skip(db), level = "debug", name = "fs::File::add_db")]
    async fn add_db(path: PathBuf, fs_node_id: RecordId, db: DB) -> DBResult<()> {
        CreateFileQuery::builder()
            .fs_node(fs_node_id.clone())
            .maybe_ext(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_owned()),
            )
            .stem(
                path.file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or("[ERROR]")
                    .to_string(),
            )
            .build()
            .instrumented_execute(db.clone())
            .await
            .inspect_err(|err| tracing::error!("File {{ {path:?}->{fs_node_id} }}, {err}"))?;

        Ok(())
    }
}

#[derive(Debug, bon::Builder, SurrealQuery)]
#[query(
    check,
    error = DBError,
    sql = "
        BEGIN TRANSACTION;

        LET $file = (CREATE file SET ext = {ext}, stem = {stem}).id;
        RELATE {fs_node}->is_file->$file;

        COMMIT TRANSACTION;

        RETURN $file.id;
    "
)]
struct CreateFileQuery {
    fs_node: RecordId,
    #[builder(into)]
    stem: String,
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

#[bon::bon]
impl Symlink {
    #[builder]
    #[tracing::instrument(skip(db), level = "debug", name = "fs::Symlink::add_db")]
    async fn add_db(
        #[builder(into)] path: PathBuf,
        fs_node_id: RecordId,
        db: DB,
        parents: bool,
    ) -> DBResult<()> {
        let links_to = match path.read_link() {
            Ok(path) => path,
            Err(err) => {
                tracing::trace!("WARN: Failed to read the symlink {path:?}: {err}");
                return Ok(());
            }
        };

        let symlinked_fs_node: RecordId = Box::pin(
            FSNode::add_db()
                .path(links_to)
                .db(db.clone())
                .parents(parents)
                .call(),
        )
        .await?;

        CreateSymlinkQuery::builder()
            .fs_node(fs_node_id)
            .symlinked_fs_node(symlinked_fs_node)
            .build()
            .instrumented_execute(db)
            .await?;

        Ok(())
    }
}

#[derive(Debug, bon::Builder, SurrealQuery)]
#[query(
    check,
    error = DBError,
    sql = "
        BEGIN TRANSACTION;

        LET $symlink = (CREATE symlink).id;
        RELATE {fs_node}->is_symlink->$symlink;
        RELATE $symlink->is_symlink_of->{symlinked_fs_node};

        COMMIT TRANSACTION;
    "
)]
struct CreateSymlinkQuery {
    fs_node: RecordId,
    symlinked_fs_node: RecordId,
}
