use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use icon_cache::file::OwnedIconCache;
use itertools::Itertools;

use macros::lerror;
use surrealdb::value;
use tokio::task::JoinSet;
use tracing::Instrument;
use walkdir::WalkDir;

use crate::db::{
    DB,
    apps::{AppEntry, AppIcon},
};

#[derive(Clone, derive_more::Debug)]
pub struct AppsFinder {
    #[debug(skip)]
    stop_receiver: tokio_mpmc::Receiver<()>,
}

impl AppsFinder {
    pub fn new() -> (Self, tokio_mpmc::Sender<()>) {
        let (stop_sender, stop_receiver) = tokio_mpmc::channel(10);
        let res = Self { stop_receiver };

        (res, stop_sender)
    }

    pub async fn search(self, db: Arc<DB>) -> AppsResult<()> {
        let Self { stop_receiver } = self;

        macro_rules! check_stop {
            ($stop_recv:expr) => {
                match $stop_recv.is_empty() {
                    true => {}
                    false => match $stop_recv.recv().await {
                        Ok(_) => return Err(AppsError::InterruptedByParent),
                        Err(err) => match err {
                            tokio_mpmc::ChannelError::ChannelClosed => {
                                return Err(AppsError::LostConnectionToParent)
                            }
                            _ => unreachable!(),
                        },
                    },
                }
            };
        }

        static DEFAULT_PATHS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
            ["/usr/share", "/usr/local/share", "/snap/"]
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

        // Icons Search
        let home_icons_path = home_path.as_ref().and_then(|hp| {
            let p = hp.join(".icons");
            p.exists().then_some(p)
        });

        let icon_search_paths = DEFAULT_PATHS
            .iter()
            .chain(xdg_paths.iter())
            .chain(home_icons_path.iter())
            .unique()
            .sorted()
            .cloned()
            .collect_vec();

        let mut unordered_search_tasks = JoinSet::new();

        let db_clone = db.clone();

        check_stop!(stop_receiver);

        let stop_receiver_clone = stop_receiver.clone();
        unordered_search_tasks.spawn(
            async move {
                let stop_receiver = stop_receiver_clone;
                let db = db_clone;

                let (icon_search_path_sender, mut icon_search_path_receiver) =
                    tokio::sync::mpsc::unbounded_channel::<PathBuf>();
                let icon_search_path_sender_from_cache = icon_search_path_sender.clone();

                let (icon_cache_sender, mut icon_cache_receiver) =
                    tokio::sync::mpsc::unbounded_channel::<PathBuf>();

                let (icon_path_sender, mut icon_path_receiver) =
                    tokio::sync::mpsc::unbounded_channel::<PathBuf>();

                let mut tasks = JoinSet::new();

                let stop_receiver_clone = stop_receiver.clone();
                let db_clone = db.clone();
                tasks.spawn(async move {
                    let stop_receiver = stop_receiver_clone;
                    let db = db_clone;

                    while !icon_search_path_receiver.is_closed() {
                        check_stop!(stop_receiver);

                        let Some(icon_search_path) = icon_search_path_receiver.recv().await else {
                            continue;
                        };

                        tracing::trace!("New Search Path: {icon_search_path:?}");

                        for entry in WalkDir::new(icon_search_path).min_depth(1).max_depth(10) {
                            check_stop!(stop_receiver);

                            let entry = entry?;
                            let path = entry.path();

                            if path.is_dir() {
                                continue;
                            }

                            if path.file_name().and_then(|n| n.to_str()) == Some("icon-theme.cache")
                            {
                                icon_cache_sender.send(path.to_path_buf())?;
                                continue;
                            }

                            if !value::from_value::<bool>(db
                                .query("RETURN (array::is_empty(SELECT VALUE id FROM icons WHERE path = $path))")
                                .bind(("path", path.to_path_buf()))
                                .await?
                                .take(0)?)?
                            {
                                continue;
                            }

                            let Some(ext) = path.extension() else {
                                continue;
                            };

                            let ext = ext.to_string_lossy().to_string().to_lowercase();

                            if image::ImageFormat::all()
                                .flat_map(|f| f.extensions_str())
                                .chain([&"svg", &"xpm"])
                                .map(|s| s.to_lowercase())
                                .unique()
                                .any(|e| e == ext)
                            {
                                icon_path_sender.send(path.to_path_buf())?;
                            }
                        }
                    }

                    AppsResult::Ok(())
                }.instrument(tracing::trace_span!("Icon Search Path Task")));

                let stop_receiver_clone = stop_receiver.clone();
                tasks.spawn(async move {
                    let stop_receiver = stop_receiver_clone;

                    while !icon_cache_receiver.is_closed() {
                        check_stop!(stop_receiver);

                        let Some(icon_cache) = icon_cache_receiver.recv().await else {
                            continue;
                        };

                        tracing::trace!("New icon cache: {icon_cache:?}");

                        let owned_cache = OwnedIconCache::open(&icon_cache)?;
                        let ref_cache = owned_cache
                            .icon_cache()
                            .map_err(|err| AppsError::Dynamic(err.to_string()))?;

                        let icon_search_paths = ref_cache.iter().flat_map(|icon| {
                            icon.image_list.iter().next().map(|img| img.directory)
                        });

                        for icon_search_path in icon_search_paths {
                            let icon_search_path = match icon_search_path.is_relative() {
                                true => icon_cache.parent().unwrap().join(icon_search_path),
                                false => icon_search_path.into()
                            };

                            tracing::trace!("Sending Icon Search Path From Icon Cache: {icon_search_path:?}");

                            icon_search_path_sender_from_cache.send(icon_search_path)?;
                        }
                    }

                    AppsResult::Ok(())
                }.instrument(tracing::trace_span!("Icon Cache Task")));

                let stop_receiver_clone = stop_receiver.clone();
                tasks.spawn(async move {
                    let stop_receiver = stop_receiver_clone;

                    while !icon_path_receiver.is_closed() {
                        check_stop!(stop_receiver);

                        let mut icon_paths = vec![];

                        icon_path_receiver.recv_many(&mut icon_paths, 1000).await;

                        if icon_paths.is_empty() {
                            continue;
                        }

                        tracing::trace!("New Icons [{}]: {icon_paths:#?}!", icon_paths.len());

                        for icon_path in icon_paths {
                            if let Err(err) = db.create::<Option<AppIcon>>("icons")
                                .content(AppIcon::new(icon_path)?)
                                .await
                            {
                                tracing::trace!("WARN: Failed to add icon to database: {err}");
                            }
                        }
                    }

                    AppsResult::Ok(())
                }.instrument(tracing::trace_span!("Icon Path Task")));

                for icon_search_path in icon_search_paths {
                    check_stop!(stop_receiver);

                    tracing::trace!("Sending search path: {icon_search_path:?}");
                    icon_search_path_sender.send(icon_search_path)?;
                }

                drop(icon_search_path_sender);

                check_stop!(stop_receiver);

                tasks.join_all().await.into_iter().collect::<AppsResult<Vec<_>>>()?;

                AppsResult::Ok(())
            }.instrument(tracing::debug_span!("Icon Search Task"))
        );

        // Apps search
        let home_share_path = home_path.as_ref().and_then(|hp| {
            let p = hp.join(".local");
            p.exists().then_some(p)
        });

        let app_search_paths = DEFAULT_PATHS
            .iter()
            .chain(xdg_paths.iter())
            .chain(home_share_path.iter())
            .unique()
            .sorted()
            .cloned()
            .collect_vec();

        unordered_search_tasks.spawn(async move {
            for search_path in app_search_paths {
                for entry in WalkDir::new(search_path).max_depth(5) {
                    let entry = entry?;
                    let path = entry.path();

                    if path.is_dir() {
                        continue;
                    }

                    if !value::from_value::<bool>(db
                        .query("RETURN (array::is_empty(SELECT VALUE id FROM entries WHERE desktop_entry_path = $path))")
                        .bind(("path", path.to_path_buf()))
                        .await?
                        .take(0)?)?
                    {
                        continue;
                    }

                    if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                        tracing::trace!("New app: {path:?}");

                        match AppEntry::new(path) {
                            Ok(entry) => {
                                if let Err(err)=   db
                                    .create::<Option<AppEntry>>("entries")
                                    .content(entry)
                                    .await
                                {
                                    tracing::trace!("WARN: Failed to add an app entry: {err}");
                                }
                            }
                            Err(err) => {
                                tracing::error!("{err}");
                                continue;
                            }
                        }
                    }
                }
            }

            AppsResult::Ok(())
        }.instrument(tracing::debug_span!("App Search Task")));

        unordered_search_tasks
            .join_all()
            .await
            .into_iter()
            .collect::<AppsResult<Vec<_>>>()?;

        Ok(())
    }
}

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

    #[lerr(str = "[walkdir] {0}")]
    WalkDir(#[lerr(from, wrap = Arc)] walkdir::Error),

    #[lerr(str = "[image] {0}")]
    Image(#[lerr(from, wrap = Arc)] image::ImageError),

    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::DecodeError),

    #[lerr(str = "[dynamic] {0}")]
    Dynamic(String),
}
