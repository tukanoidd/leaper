use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use async_walkdir::{Filtering, WalkDir};
use freedesktop_desktop_entry::DesktopEntry;
use futures::{StreamExt, TryStreamExt, stream};
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
use tokio::sync::{
    Mutex,
    oneshot::{self, Receiver, Sender},
};

use leaper_db::{DB, DBEntryId};
use macros::{db_entry, lerror};

#[derive(Clone, Debug)]
pub struct AppsFinder {
    stop_receiver: Arc<Mutex<Receiver<()>>>,
}

impl AppsFinder {
    pub fn new() -> (Self, Sender<()>) {
        let (stop_sender, stop_receiver) = oneshot::channel();
        let res = Self {
            stop_receiver: Arc::new(Mutex::new(stop_receiver)),
        };

        (res, stop_sender)
    }

    pub async fn search(self, db: Arc<DB>) -> AppsResult<()> {
        let Self { stop_receiver } = self;

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
            .collect_vec();

        macro_rules! check_stop {
            () => {
                match stop_receiver.lock().await.try_recv() {
                    Ok(_) => return Err(AppsError::InterruptedByParent),
                    Err(err) => match err {
                        oneshot::error::TryRecvError::Empty => {}
                        oneshot::error::TryRecvError::Closed => {
                            return Err(AppsError::LostConnectionToParent);
                        }
                    },
                }
            };
        }

        check_stop!();

        tracing::debug!("Getting cached icon paths...");

        let cached_icon_paths = Arc::new(
            db.get_table_field::<AppIcon, PathBuf>(AppIcon::FIELD_PATH)
                .await?,
        );

        tracing::debug!("Cached icon paths: {}", cached_icon_paths.len());

        check_stop!();

        tracing::debug!("Looking for icon cache directories...");

        let icon_caches_dirs = icon_search_paths
            .iter()
            .map(|icon_search_path| {
                WalkDir::new(icon_search_path).filter(move |entry| async move {
                    let path = entry.path();

                    if path.file_name().and_then(|s| s.to_str()) != Some("icon-theme.cache") {
                        return Filtering::Ignore;
                    }

                    Filtering::Continue
                })
            })
            .collect_vec();

        for mut dir in icon_caches_dirs {
            while let Some(entry) = dir.next().await {
                check_stop!();

                let path = entry?.path();

                let icon_cache = OwnedIconCache::open(&path)?;
                let icon_cache_ref = icon_cache
                    .icon_cache()
                    .map_err(|e| AppsError::Dynamic(e.to_string()))?;

                let img_dirs_list = icon_cache_ref
                    .iter()
                    .flat_map(|icon| {
                        icon.image_list
                            .iter()
                            .map(|img| img.directory.to_path_buf())
                            .collect_vec()
                    })
                    .collect_vec();

                for img_dir in img_dirs_list {
                    let dir = match img_dir.is_relative() {
                        true => path.parent().unwrap().join(img_dir),
                        false => img_dir.to_path_buf(),
                    };

                    let mut walkdir = WalkDir::new(dir).filter({
                        let icon_cache_paths = cached_icon_paths.clone();

                        move |entry| {
                            let value = icon_cache_paths.clone();

                            async move {
                                let path = entry.path();

                                if value.contains(&path) {
                                    return Filtering::Ignore;
                                }

                                let Some(ext) = path.extension() else {
                                    return Filtering::Ignore;
                                };

                                let ext = ext.to_string_lossy().to_string().to_lowercase();

                                if !image::ImageFormat::all()
                                    .flat_map(|f| f.extensions_str())
                                    .chain([&"svg", &"xpm"])
                                    .map(|s| s.to_lowercase())
                                    .unique()
                                    .any(|e| e == ext)
                                {
                                    return Filtering::Ignore;
                                }

                                Filtering::Continue
                            }
                        }
                    });

                    while let Some(entry) = walkdir.next().await {
                        check_stop!();
                        db.new_entry::<AppIcon>(AppIcon::new(entry?.path())?)
                            .await?;
                    }
                }
            }
        }

        for icon_search_path in icon_search_paths {
            let cached_icon_paths = cached_icon_paths.clone();

            let mut walkdir = WalkDir::new(icon_search_path).filter(move |entry| {
                let cached_icon_paths = cached_icon_paths.clone();

                async move {
                    let path = entry.path();

                    if cached_icon_paths.contains(&path) {
                        return Filtering::Ignore;
                    }

                    let Some(ext) = path.extension() else {
                        return Filtering::Ignore;
                    };

                    let ext = ext.to_string_lossy().to_string().to_lowercase();

                    if !image::ImageFormat::all()
                        .flat_map(|f| f.extensions_str())
                        .chain([&"svg", &"xpm"])
                        .map(|s| s.to_lowercase())
                        .unique()
                        .any(|e| e == ext)
                    {
                        return Filtering::Ignore;
                    }

                    Filtering::Continue
                }
            });

            while let Some(entry) = walkdir.next().await {
                check_stop!();
                db.new_entry::<AppIcon>(AppIcon::new(entry?.path())?)
                    .await?;
            }
        }

        tracing::debug!("Done searching for new icons");

        check_stop!();

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
            .collect_vec();

        check_stop!();

        tracing::debug!("Getting cached icons with ids...");

        let cached_icons_with_id = Arc::new(db.get_table::<AppIconWithId>().await?);

        tracing::debug!("Cached icons with ids: {}", cached_icons_with_id.len());

        check_stop!();

        tracing::debug!("Getting cached app paths...");

        let cached_app_paths = Arc::new(
            db.get_table_field::<App, PathBuf>(App::FIELD_DESKTOP_ENTRY_PATH)
                .await?,
        );

        tracing::debug!("Cached app paths: {}", cached_app_paths.len());

        check_stop!();

        tracing::debug!("Searching for new apps...");

        for app_search_path in app_search_paths {
            let cached_app_paths = cached_app_paths.clone();

            let mut walkdir = WalkDir::new(app_search_path).filter(move |entry| {
                let cached_app_paths = cached_app_paths.clone();

                async move {
                    let path = entry.path();

                    if cached_app_paths.contains(&path) {
                        return Filtering::Ignore;
                    }

                    let Some(ext) = path.extension() else {
                        return Filtering::Ignore;
                    };

                    if ext != "desktop" {
                        return Filtering::Ignore;
                    }

                    Filtering::Continue
                }
            });

            while let Some(entry) = walkdir.next().await {
                check_stop!();
                db.new_entry::<App>(App::new(entry?.path(), cached_icons_with_id.clone())?)
                    .await?;
            }
        }

        tracing::debug!("Done searching for new apps");

        Ok(())
    }
}

#[db_entry]
#[db(db_name = "apps", table_name = "entries")]
pub struct App {
    pub desktop_entry_path: PathBuf,
    pub name: String,
    pub exec: Vec<String>,
    pub icon: Option<DBEntryId>,
}

impl App {
    pub fn new(path: impl AsRef<Path>, cached_icons: Arc<Vec<AppIconWithId>>) -> AppsResult<Self> {
        let path = path.as_ref();
        let entry = DesktopEntry::from_path::<&str>(path, None)?;
        let name = entry
            .full_name::<&str>(&[])
            .ok_or_else(|| AppsError::DesktopEntryNoName(path.to_path_buf()))
            .inspect_err(|err| tracing::error!("{err}"))
            .map(|s| s.to_string())
            .unwrap_or_else(|_| "Unknown".into());
        let exec = entry
            .exec()
            .ok_or_else(|| AppsError::DesktopEntryNoExec(path.to_path_buf()))
            .and_then(|exec_str| {
                shlex::split(exec_str).ok_or_else(|| {
                    AppsError::DesktopEntryParseExec(path.to_path_buf(), exec_str.into())
                })
            })?;
        let icon = entry.icon().and_then(|icon_name| {
            cached_icons
                .iter()
                .find_map(|icon| (icon.name == icon_name).then_some(icon.id.clone()))
        });

        Ok(Self {
            desktop_entry_path: path.into(),
            name,
            exec,
            icon,
        })
    }
}

#[db_entry]
#[db(db_name = "apps", table_name = "entries")]
pub struct AppWithIcon {
    pub desktop_entry_path: PathBuf,
    pub name: String,
    pub exec: Vec<String>,
    pub icon: Option<AppIcon>,
}

#[db_entry]
#[db(db_name = "apps", table_name = "icons")]
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

#[db_entry]
#[db(db_name = "apps", table_name = "icons")]
pub struct AppIconWithId {
    pub id: DBEntryId,
    pub name: String,
    pub path: PathBuf,
    pub svg: bool,
    pub xpm: bool,
    pub dims: Option<AppIconDims>,
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

    #[lerr(str = "{0}")]
    DB(#[lerr(from)] leaper_db::DBError),

    #[lerr(str = "[async_walkdir] {0}")]
    AsyncWalkDir(#[lerr(from, wrap = Arc)] async_walkdir::Error),

    #[lerr(str = "[image] {0}")]
    Image(#[lerr(from, wrap = Arc)] image::ImageError),

    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::DecodeError),

    #[lerr(str = "[dynamic] {0}")]
    Dynamic(String),
}
