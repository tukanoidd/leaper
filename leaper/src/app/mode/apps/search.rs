use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use futures::StreamExt;
use itertools::Itertools;

use macros::lerror;
use surrealdb::{Notification, value};
use surrealdb_extras::SurrealQuery;
use tokio::task::JoinSet;

use crate::{
    check_stop,
    db::{
        DB, InstrumentedSurrealQuery,
        apps::CreateAppEntryQuery,
        fs::{self, FSError, FSResult},
    },
};

#[derive(Clone, derive_more::Debug)]
pub struct AppsFinder {
    #[debug(skip)]
    stop_receiver: tokio_mpmc::Receiver<()>,
}

#[bon::bon]
impl AppsFinder {
    pub fn new() -> (Self, tokio_mpmc::Sender<()>) {
        let (stop_sender, stop_receiver) = tokio_mpmc::channel(10);
        let res = Self { stop_receiver };

        (res, stop_sender)
    }

    #[tracing::instrument(skip_all, level = "debug", name = "AppsFinder::search")]
    pub async fn search(self, db: DB) -> AppsResult<()> {
        let Self { stop_receiver } = self;

        let mut tasks = JoinSet::new();

        static DEFAULT_PATHS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
            ["/usr/share/", "/usr/local/share/", "/snap/"]
                .into_iter()
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .collect_vec()
        });

        let xdg_paths = std::env::var("XDG_DATA_DIRS")
            .ok()
            .map(|dirs_str| {
                dirs_str
                    .split(":")
                    .map(PathBuf::from)
                    .filter(|p| p.exists())
                    .collect_vec()
            })
            .into_iter()
            .flatten()
            .collect_vec();

        let home_path = std::env::var("HOME").ok().map(PathBuf::from);

        let home_icons_path = home_path.as_ref().and_then(|hp| {
            let p = hp.join(".icons/");
            p.exists().then_some(p)
        });

        let home_share_path = home_path.as_ref().and_then(|hp| {
            let p = hp.join(".local/share/applications/");
            p.exists().then_some(p)
        });

        let icon_paths = DEFAULT_PATHS
            .iter()
            .chain(xdg_paths.iter())
            .chain(home_icons_path.iter())
            .unique()
            .cloned()
            .collect_vec();

        let app_paths = DEFAULT_PATHS
            .iter()
            .chain(xdg_paths.iter())
            .chain(home_share_path.iter())
            .unique()
            .cloned()
            .collect_vec();

        // Apps Search
        {
            let db_clone = db.clone();
            let stop_receiver_clone = stop_receiver.clone();

            tasks.spawn(async move {
                let mut desktop_entries_stream = LiveSearchAppsQuery
                    .instrumented_execute(db_clone.clone())
                    .await?;

                check_stop!([AppsError] stop_receiver_clone);

                while let Some(entry) = desktop_entries_stream.next().await {
                    check_stop!([AppsError] stop_receiver_clone);

                    match entry {
                        Ok(Notification { action, data, .. }) => match action {
                            value::Action::Create => {
                                let _ = CreateAppEntryQuery::new(data)
                                    .inspect_err(|err| tracing::error!("{err}"))?
                                    .instrumented_execute(db_clone.clone())
                                    .await;
                            }
                            value::Action::Update => {
                                tracing::error!("UPDATE???");
                                // TODO
                            }
                            value::Action::Delete => {
                                tracing::error!("DELETE???");
                                // TODO
                            }
                            _ => todo!(),
                        },
                        Err(err) => {
                            tracing::error!("{err}");
                            continue;
                        }
                    }
                }

                Ok(())
            });
        }

        // .desktop Search
        Self::search_paths()
            .tasks(&mut tasks)
            .stop_receiver(stop_receiver.clone())
            .db(db.clone())
            .paths(app_paths)
            .exts(vec!["desktop"])
            .kind(".desktop")
            .call();

        // Icons Search
        Self::search_paths()
            .tasks(&mut tasks)
            .stop_receiver(stop_receiver.clone())
            .db(db.clone())
            .paths(icon_paths)
            .exts(vec![
                "png", "jpg", "jpeg", "gif", "webp", "pbm", "pam", "ppm", "pgm", "tiff", "tif",
                "tga", "dds", "bmp", "ico", "hdr", "exr", "ff", "avif", "qoi", "pcx", "svg", "xpm",
            ])
            .kind("icon")
            .call();

        tasks
            .join_all()
            .await
            .into_iter()
            .collect::<AppsResult<Vec<_>>>()?;

        Ok(())
    }

    #[builder]
    #[tracing::instrument(
        skip(tasks, stop_receiver, db),
        level = "debug",
        name = "AppsFinder::search_paths"
    )]
    fn search_paths(
        tasks: &mut JoinSet<AppsResult<()>>,
        stop_receiver: tokio_mpmc::Receiver<()>,
        db: DB,
        paths: Vec<PathBuf>,
        exts: Vec<&'static str>,
        #[builder(into)] kind: String,
    ) {
        tasks.spawn(async move {
            let mut index_tasks = JoinSet::new();

            check_stop!([AppsError] stop_receiver);

            paths.into_iter().for_each(|path| {
                let exts = exts.clone();
                let stop_receiver = stop_receiver.clone();

                index_tasks.spawn(
                    fs::index()
                        .root(path.clone())
                        .kind(kind.clone())
                        .db(db.clone())
                        .pre_filter(move |path| {
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
                        })
                        .stop_receiver(stop_receiver.clone())
                        .call(),
                );
            });

            check_stop!([AppsError] stop_receiver);

            index_tasks
                .join_all()
                .await
                .into_iter()
                .collect::<FSResult<Vec<_>>>()?;

            Ok(())
        });
    }
}

#[derive(Debug, SurrealQuery)]
#[query(
    stream = "PathBuf",
    error = AppsError,
    sql = "
        LIVE SELECT VALUE in.path
            FROM is_file
            WHERE out.ext == 'desktop';
    "
)]
struct LiveSearchAppsQuery;

#[lerror]
#[lerr(prefix = "[apps]", result_name = AppsResult)]
pub enum AppsError {
    #[lerr(str = "Path {0:?} doesn't have a file name...")]
    NoFileName(PathBuf),

    #[lerr(str = "{0:?} provides no name!")]
    DesktopEntryNoName(PathBuf),
    #[lerr(str = "{0:?} provides no exec!")]
    DesktopEntryNoExec(PathBuf),
    #[lerr(str = "Failed to parse exec '{1}' from {0:?}!")]
    DesktopEntryParseExec(PathBuf, String),

    #[lerr(str = "Interrupted by parent")]
    InterruptedByParent,
    #[lerr(str = "Lost connection to the parent")]
    LostConnectionToParent,

    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),

    #[lerr(str = "[tokio::task::join] {0}")]
    TokioJoin(#[lerr(from, wrap = Arc)] tokio::task::JoinError),
    #[lerr(str = "[tokio::sync::mpsc::send<PathBuf>] {0}")]
    TokioMpscSendPathBuf(#[lerr(from)] tokio::sync::mpsc::error::SendError<PathBuf>),

    #[lerr(str = "[surrealdb] {0}")]
    DB(#[lerr(from, wrap = Arc)] surrealdb::Error),
    #[lerr(str = "[db::fs] {0}")]
    FS(#[lerr(from)] FSError),

    #[lerr(str = "[image] {0}")]
    Image(#[lerr(from, wrap = Arc)] image::ImageError),

    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::DecodeError),
    #[lerr(str = "[.desktop::exec] {0}")]
    DesktopEntryExec(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::ExecError),

    #[lerr(str = "[dynamic] {0}")]
    Dynamic(String),
}
