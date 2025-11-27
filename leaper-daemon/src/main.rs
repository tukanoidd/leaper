use std::{
    path::PathBuf,
    sync::{
        LazyLock,
        atomic::{
            AtomicBool,
            Ordering::{self, SeqCst},
        },
    },
};

use color_eyre::{Result, eyre::OptionExt};
use directories::ProjectDirs;
use futures::prelude::*;
use itertools::Itertools;
use tarpc::{
    server::{BaseChannel, Channel},
    tokio_serde::formats::Bincode,
};
use tokio::task::{self, JoinSet};

use db::{
    DBAction, DBNotification, InstrumentedDBQuery,
    apps::{CreateAppEntryQuery, LiveSearchAppsQuery},
    init_db,
};

use leaper_daemon::{
    ADDRESS, DB_REF, LeaperDaemon,
    fs::{self, search_paths},
};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    leaper_tracing::init_tracing(false, false, false)?;

    let project_dirs = ProjectDirs::from("com", "tukanoid", "leaper")
        .ok_or_eyre("Failed to get project directories")?;
    let config = mode::config::LeaperModeConfig::open(&project_dirs)?;
    let db = init_db(config.db_port).await?;

    DB_REF.set(db).unwrap();

    let mut listener = tarpc::serde_transport::tcp::listen(ADDRESS, Bincode::default).await?;
    listener.config_mut().max_frame_length(usize::MAX);

    listener
        .filter_map(|r| futures::future::ready(r.inspect_err(|err| tracing::error!("{err}")).ok()))
        .map(BaseChannel::with_defaults)
        .map(|channel| {
            let server = LeaperDaemonServer;

            tracing::debug!("Serving daemon server...");

            channel.execute(server.serve()).for_each(|x| async {
                tokio::spawn(x);
            })
        })
        .for_each(|c| c)
        .await;

    Ok(())
}

static SEARCHING_FOR_APPS_ICONS: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct LeaperDaemonServer;

impl LeaperDaemon for LeaperDaemonServer {
    #[tracing::instrument(
        skip(self, _context),
        level = "debug",
        name = "leaper_daemon::search_apps"
    )]
    async fn search_apps(self, _context: ::tarpc::context::Context) {
        if SEARCHING_FOR_APPS_ICONS.load(SeqCst) {
            tracing::warn!("Search job for apps and icons is already running");
            return;
        }

        SEARCHING_FOR_APPS_ICONS.store(true, Ordering::SeqCst);

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

        let db = DB_REF.get().unwrap();

        // Apps Search
        {
            let db_clone = db.clone();

            tasks.spawn(async move {
                let mut desktop_entries_stream = LiveSearchAppsQuery
                    .instrumented_execute(db_clone.clone())
                    .await?;

                while let Some(entry) = desktop_entries_stream.next().await {
                    match entry {
                        Ok(DBNotification { action, data, .. }) => match action {
                            DBAction::Create => {
                                let _ = CreateAppEntryQuery::new(data)
                                    .inspect_err(|err| tracing::error!("{err}"))?
                                    .instrumented_execute(db_clone.clone())
                                    .await;
                            }
                            DBAction::Update => {
                                tracing::error!("UPDATE???");
                                // TODO
                            }
                            DBAction::Delete => {
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
        search_paths(&mut tasks, app_paths, vec!["desktop"], ".desktop".into());

        // Icons Search
        search_paths(
            &mut tasks,
            icon_paths,
            vec![
                "png", "jpg", "jpeg", "gif", "webp", "pbm", "pam", "ppm", "pgm", "tiff", "tif",
                "tga", "dds", "bmp", "ico", "hdr", "exr", "ff", "avif", "qoi", "pcx", "svg", "xpm",
            ],
            "icon".into(),
        );

        task::spawn(async move {
            let _ = tasks
                .join_all()
                .await
                .into_iter()
                .collect::<Result<Vec<_>>>();

            tracing::debug!("Done searching for apps and icons!");
            SEARCHING_FOR_APPS_ICONS.store(false, SeqCst);
        });

        tracing::debug!("Waiting on rest of apps and icons in a detached task...");
    }

    async fn index(self, _context: ::tarpc::context::Context, root: PathBuf, parents: bool) {
        tracing::debug!("Indexing {root:?}");

        fs::index(root, parents, |_| None).await
    }
}
