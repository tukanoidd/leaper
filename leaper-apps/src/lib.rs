use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use freedesktop_desktop_entry::DesktopEntry;
use itertools::Itertools;
use leaper_db::{DB, db_entry};
use macros::lerror;
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
use walkdir::{DirEntry, WalkDir};

pub async fn search_apps(db: Arc<DB>) -> AppsResult<Vec<AppEntry>> {
    let default_paths = ["/usr/share", "/usr/local/share", "/snap/"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect_vec();

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

    let home_share_path = home_path.as_ref().and_then(|hp| {
        let p = hp.join(".local");
        p.exists().then_some(p)
    });
    let home_icons_path = home_path.as_ref().and_then(|hp| {
        let p = hp.join(".icons");
        p.exists().then_some(p)
    });

    let app_search_paths = default_paths
        .iter()
        .chain(xdg_paths.iter())
        .chain(home_share_path.iter())
        .unique()
        .sorted()
        .collect_vec();

    let icon_search_paths = default_paths
        .iter()
        .chain(xdg_paths.iter())
        .chain(home_icons_path.iter())
        .unique()
        .sorted()
        .collect_vec();

    let icons = icon_search_paths
        .into_iter()
        .map(|search_path| {
            SearchPath::builder()
                .path(search_path)
                .depth(10)
                .build()
                .search(|entry| {
                    let path = entry.path();

                    if path.is_dir() {
                        return Ok(None);
                    }

                    let Some(ext) = path.extension() else {
                        return Ok(None);
                    };

                    let ext = ext.to_string_lossy().to_string().to_lowercase();

                    if !image::ImageFormat::all()
                        .flat_map(|f| f.extensions_str())
                        .chain([&"svg", &"xpm"])
                        .map(|s| s.to_lowercase())
                        .unique()
                        .any(|e| e == ext)
                    {
                        return Ok(None);
                    }

                    Ok(Some(AppIcon::new(path)?))
                })
        })
        .collect::<AppsResult<Vec<_>>>()?
        .into_iter()
        .flat_map(|x| {
            x.into_iter()
                .flatten()
                .sorted_by_key(|i| (i.path.clone(), i.dims))
        })
        .collect_vec();

    let apps = app_search_paths
        .into_iter()
        .map(|search_path| {
            SearchPath::builder()
                .path(search_path)
                .depth(5)
                .build()
                .search(|entry| {
                    let path = entry.path();

                    if path.is_dir() {
                        return Ok(None);
                    }

                    let Some(ext) = path.extension() else {
                        return Ok(None);
                    };

                    if ext != "desktop" {
                        return Ok(None);
                    }

                    Ok(AppEntry::new(path, &icons)
                        .inspect_err(|err| tracing::error!("{err}"))
                        .ok())
                })
        })
        .collect::<AppsResult<Vec<_>>>()?
        .into_iter()
        .flat_map(|x| x.into_iter().flatten().unique_by(|x| x.name.clone()))
        .unique_by(|x| x.name.clone())
        .sorted_by_key(|x| x.name.clone())
        .collect_vec();

    db.set_table(apps.clone()).await?;
    tracing::trace!("Cached app list [{} entries]", apps.len());

    Ok(apps)
}

#[derive(bon::Builder, Clone)]
struct SearchPath {
    #[builder(into)]
    path: PathBuf,
    depth: usize,
}

impl SearchPath {
    fn search<V>(self, process: impl Fn(DirEntry) -> AppsResult<V>) -> AppsResult<Vec<V>> {
        let Self { path, depth } = self;

        WalkDir::new(path)
            .min_depth(1)
            .max_depth(depth)
            .into_iter()
            .flatten()
            .map(process)
            .collect::<AppsResult<Vec<_>>>()
    }
}

#[db_entry]
#[db(db_name = "apps", table_name = "entries")]
pub struct AppEntry {
    pub name: String,
    pub exec: Vec<String>,
    pub icon: Option<AppIcon>,
}

impl AppEntry {
    pub fn new(path: impl AsRef<Path>, icons: &[AppIcon]) -> AppsResult<Self> {
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

        let icon = match entry.icon() {
            Some(icon_str) => match icons.iter().find_map(|e| {
                (e.name == icon_str || e.path.as_path() == Path::new(icon_str)).then(|| e.clone())
            }) {
                None => {
                    tracing::error!("Failed to find an icon for {name} ({icon_str:?}): {exec:?}");
                    None
                }
                val => val,
            },
            None => {
                tracing::warn!("Filed to find an icon entry for {name}: {exec:?}");
                None
            }
        };

        Ok(Self { name, exec, icon })
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
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

    #[lerr(str = "{0}")]
    DB(#[lerr(from)] leaper_db::DBError),

    #[lerr(str = "[image] {0}")]
    Image(#[lerr(from, wrap = Arc)] image::ImageError),

    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::DecodeError),
}
