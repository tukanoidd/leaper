use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use freedesktop_desktop_entry::DesktopEntry;
use icon_cache::file::OwnedIconCache;
use itertools::Itertools;
use nom::{
    IResult, Parser,
    branch::permutation,
    bytes::tag,
    character::{char, none_of, one_of},
    combinator::recognize,
    multi::{many0, many1},
    sequence::terminated,
};
use serde::{Deserialize, Serialize};
use surrealdb::{RecordId, value};
use surrealdb_extras::SurrealTable;

use macros::lerror;
use tokio::task::JoinSet;
use tracing::Instrument;
use walkdir::WalkDir;

use crate::db::DB;

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

#[derive(Clone, SurrealTable, Serialize, Deserialize)]
#[db("entries")]
#[sql([
        "DEFINE INDEX app_dep_ind ON TABLE entries COLUMNS desktop_entry_path UNIQUE",
        "DEFINE INDEX app_name_ind ON TABLE entries COLUMNS name UNIQUE",
        "
        DEFINE EVENT app_entry_added ON TABLE entries
            WHEN $event = \"CREATE\" && $after.icon_name != NULL
            THEN (
                UPDATE $after.id SET icon = (SELECT VALUE id FROM ONLY icons
                    WHERE name = $after.icon_name
                    LIMIT 1)
            )
        "
])]
pub struct AppEntry {
    pub desktop_entry_path: PathBuf,
    pub name: String,
    pub exec: Vec<String>,
    pub icon: Option<RecordId>,
    pub icon_name: Option<String>,
}

impl AppEntry {
    pub fn new(path: impl AsRef<Path>) -> AppsResult<Self> {
        let path = path.as_ref();
        let entry = DesktopEntry::from_path::<&str>(path, None)?;
        let name = entry
            .full_name::<&str>(&[])
            .ok_or_else(|| AppsError::DesktopEntryNoName(path.to_path_buf()))
            .inspect_err(|err| tracing::error!("{err}"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Unknown".into());
        let exec = entry
            .exec()
            .ok_or_else(|| AppsError::DesktopEntryNoExec(path.to_path_buf()))
            .and_then(|exec_str| {
                shlex::split(exec_str).ok_or_else(|| {
                    AppsError::DesktopEntryParseExec(path.to_path_buf(), exec_str.into())
                })
            })?;
        let icon_name = entry.icon().map(|icon_name| icon_name.to_string());

        Ok(Self {
            desktop_entry_path: path.into(),
            name,
            exec,
            icon: None,
            icon_name,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppWithIcon {
    pub id: RecordId,
    pub desktop_entry_path: PathBuf,
    pub name: String,
    pub exec: Vec<String>,
    #[serde(default)]
    pub icon: Option<AppIcon>,
}

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[db("icons")]
#[sql([
    "DEFINE INDEX icon_path_ind ON TABLE icons COLUMNS path UNIQUE",
    "
    DEFINE EVENT icon_added ON TABLE icons
        WHEN $event = \"CREATE\"
        THEN (
            UPDATE entries SET icon = $value.id WHERE icon_name = $value.name
        )
    "
])]
pub struct AppIcon {
    pub name: String,
    pub path: PathBuf,
    pub svg: bool,
    pub xpm: bool,
    pub dims: Option<AppIconDims>,
}

impl AppIcon {
    pub fn new(path: impl AsRef<Path>) -> AppsResult<Self> {
        let path = path.as_ref();
        let name = path
            .file_stem()
            .ok_or_else(|| AppsError::NoFileName(path.to_path_buf()))?
            .to_string_lossy()
            .to_string();

        let dims = path.components().rev().find_map(|comp| {
            let comp_str = comp.as_os_str().to_string_lossy().to_string();
            let dims = AppIconDims::parse(&comp_str);

            dims.ok().map(|(_, dims)| dims)
        });

        let ext = path.extension().and_then(|e| e.to_str());

        Ok(Self {
            name,
            path: path.to_path_buf(),
            svg: matches!(ext, Some("svg")),
            xpm: matches!(ext, Some("xpm")),
            dims,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AppIconDims {
    pub width: usize,
    pub height: usize,
}

impl AppIconDims {
    fn parse(input: &str) -> IResult<&str, Self> {
        permutation((
            many0(none_of("0123456789")),
            terminated(Self::parse_decimal, tag("x")),
            Self::parse_decimal,
        ))
        .map(|(_, width, height)| Self { width, height })
        .parse(input)
    }

    fn area(&self) -> usize {
        self.width * self.height
    }

    fn parse_decimal(input: &str) -> IResult<&str, usize> {
        recognize(many1(terminated(
            one_of("0123456789"),
            many0(char::<&str, _>('_')),
        )))
        .map_res(|s| {
            s.parse::<usize>()
                .map_err(|_| nom::error::Error::new(input, nom::error::ErrorKind::IsNot))
        })
        .parse(input)
    }
}

impl PartialOrd for AppIconDims {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AppIconDims {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.area().cmp(&other.area())
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
