use std::{collections::HashSet, path::PathBuf};

use color_eyre::Result;
use futures::StreamExt;
use itertools::Itertools;
use tokio::task::JoinSet;
use vfs::async_vfs::{AsyncPhysicalFS, AsyncVfsPath};

use db::fs::FSNode;

use crate::DB_REF;

#[tracing::instrument(skip(pre_filter), level = "debug", name = "daemon::index")]
pub async fn index(
    root: PathBuf,
    parents: bool,
    pre_filter: impl Fn(&PathBuf) -> Option<bool> + Clone + Send + Sync + 'static,
) {
    let db = DB_REF.get().unwrap();

    let mut walkdir = AsyncVfsPath::new(AsyncPhysicalFS::new(&root))
        .walk_dir()
        .await
        .expect("Initialize walkdir")
        .filter_map(|path| {
            let pre_filter = pre_filter.clone();
            let db = db.clone();
            let root = root.clone();

            async move {
                let path = match path {
                    Ok(path) => path,
                    Err(err) => {
                        tracing::error!("{err}");
                        return None;
                    }
                };

                let path_real = root.join(path.as_str().trim_start_matches('/'));

                if let Some(res) = pre_filter.clone()(&path_real)
                    && !res
                {
                    return None;
                }

                if let Err(err) = FSNode::add_db()
                    .path(&path_real)
                    .db(db)
                    .parents(parents)
                    .call()
                    .await
                {
                    tracing::error!("Failed to add fs_node: {err}");
                }

                Some(())
            }
        })
        .boxed();

    while walkdir.next().await.is_some() {}
}

#[tracing::instrument(skip(tasks), level = "debug", name = "daemon::search_paths")]
pub fn search_paths(
    tasks: &mut JoinSet<Result<()>>,
    paths: Vec<PathBuf>,
    exts: Vec<&'static str>,
    kind: String,
) {
    tasks.spawn(async move {
        let mut index_tasks = JoinSet::new();
        let mut indexed = HashSet::new();

        paths.into_iter().for_each(|path| {
            let exts = exts.clone();

            if indexed.contains(&path) {
                return;
            }

            index_tasks.spawn(index(path.clone(), false, move |path| {
                if path.is_dir() {
                    return Some(false);
                }

                let Some(ext) = path.extension().and_then(|x| x.to_str()) else {
                    return Some(false);
                };

                if exts.contains(&ext) {
                    return None;
                }

                Some(false)
            }));

            indexed.insert(path);
        });

        index_tasks.join_all().await.into_iter().collect_vec();

        Ok(())
    });
}
