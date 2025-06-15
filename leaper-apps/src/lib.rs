use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use freedesktop_desktop_entry::DesktopEntry;
use itertools::Itertools;
use leaper_db::{DB, DBEntryId, DBTableEntry, db_entry};
use macros::lerror;
use nom::{
    IResult, Parser,
    branch::permutation,
    bytes::tag,
    character::{char, one_of},
    combinator::recognize,
    multi::{many0, many1},
    sequence::terminated,
};
use tokio::task::JoinSet;
use walkdir::{DirEntry, WalkDir};

pub async fn search_apps(
    db: Arc<DB>,
) -> AppsResult<Vec<(DBTableEntry<AppEntry>, Option<DBTableEntry<AppIcon>>)>> {
    let xdg_paths = std::env::var("XDG_DATA_DIRS").ok().map(|dirs_str| {
        dirs_str
            .split(":")
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect_vec()
    });

    let search_paths = xdg_paths
        .into_iter()
        .flatten()
        .chain(["/usr/share".into()])
        .unique()
        .sorted()
        .map(SearchPath::from)
        .collect_vec();

    let mut icons = search_paths
        .clone()
        .into_iter()
        .fold(JoinSet::new(), |mut join_set, search_path| {
            join_set.spawn(search_path.search(|entry| async move {
                let path = entry.path();

                if path.is_dir() {
                    return Ok(None);
                }

                let Some(ext) = path.extension() else {
                    return Ok(None);
                };

                let ext = ext.to_string_lossy().to_string();

                if !image::ImageFormat::all()
                    .flat_map(|f| f.extensions_str())
                    .any(|e| e == &ext)
                {
                    return Ok(None);
                }

                Ok(Some(AppIcon::new(path)?))
            }));

            join_set
        })
        .join_all()
        .await
        .into_iter()
        .collect::<AppsResult<Vec<_>>>()?
        .into_iter()
        .flat_map(|x| {
            x.into_iter()
                .flatten()
                .sorted_by_key(|i| (i.path.clone(), i.dims))
        })
        .map(DBTableEntry::from)
        .collect_vec();

    db.set_table(icons.clone()).await?;

    let apps = search_paths
        .into_iter()
        .fold(JoinSet::new(), |mut join_set, search_path| {
            let db = db.clone();

            join_set.spawn(search_path.search(move |entry| {
                let db = db.clone();

                async move {
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

                    Ok(AppEntry::new(path, db)
                        .await
                        .inspect_err(|err| tracing::error!("{err}"))
                        .ok())
                }
            }));

            join_set
        })
        .join_all()
        .await
        .into_iter()
        .collect::<AppsResult<Vec<_>>>()?
        .into_iter()
        .flat_map(|x| x.into_iter().flatten().unique_by(|x| x.name.clone()))
        .unique_by(|x| x.name.clone())
        .sorted_by_key(|x| x.name.clone())
        .map(DBTableEntry::from)
        .collect_vec();

    db.set_table(apps.clone()).await?;

    Ok(apps
        .into_iter()
        .map(|app| {
            let ind = icons
                .iter()
                .enumerate()
                .find_map(|(ind, i)| (Some(i.id()) == app.icon).then_some(ind));
            let icon = ind.map(|ind| icons.remove(ind));

            (app, icon)
        })
        .collect_vec())
}

#[derive(bon::Builder, Clone)]
struct SearchPath {
    #[builder(into)]
    path: PathBuf,
    #[builder(default = 10)]
    depth: usize,
}

impl SearchPath {
    async fn search<F, V>(self, process: impl Fn(DirEntry) -> F) -> AppsResult<Vec<V>>
    where
        F: Future<Output = AppsResult<V>> + Send + 'static,
        V: Send + 'static,
    {
        let Self { path, depth } = self;

        WalkDir::new(path)
            .min_depth(1)
            .max_depth(depth)
            .into_iter()
            .flatten()
            .fold(JoinSet::new(), |mut join_set, entry| {
                join_set.spawn(process(entry));
                join_set
            })
            .join_all()
            .await
            .into_iter()
            .collect::<AppsResult<Vec<_>>>()
    }
}

impl<P> From<P> for SearchPath
where
    P: Into<PathBuf>,
{
    fn from(value: P) -> Self {
        Self::builder().path(value).build()
    }
}

#[db_entry]
#[db(db_name = "apps", table_name = "entries")]
#[derive(Clone)]
pub struct AppEntry {
    pub name: String,
    pub exec: Vec<String>,
    pub icon: Option<DBEntryId>,
}

impl AppEntry {
    pub async fn new(path: impl AsRef<Path>, db: Arc<DB>) -> AppsResult<Self> {
        let path = path.as_ref();
        let entry = DesktopEntry::from_path::<&str>(path, None)?;
        let name = entry
            .full_name::<&str>(&[])
            .ok_or_else(|| AppsError::DesktopEntryNoName(path.to_path_buf()))
            .inspect_err(|err| tracing::error!("{err}"))
            .map(|s| s.to_string())
            .unwrap_or_else(|_| "Unknown".into());
        let exec = entry.parse_exec()?;

        let icon = match entry.icon() {
            Some(icon_str) => {
                let icons = db.get_table::<AppIcon>().await?;
                icons.into_iter().find_map(|e| {
                    (e.name == icon_str || e.path.as_path() == Path::new(icon_str)).then(|| e.id())
                })
            }
            None => None,
        };

        Ok(Self { name, exec, icon })
    }
}

#[db_entry]
#[db(db_name = "apps", table_name = "icons", derives(Hash, PartialEq, Eq))]
pub struct AppIcon {
    pub name: String,
    pub path: PathBuf,
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
            let dims = AppIconDims::parse(&comp_str).inspect_err(|err| {
                #[cfg(not(feature = "profile"))]
                let _err = err;

                #[cfg(feature = "profile")]
                tracing::trace!("[ERR] {err}");
            });

            dims.ok().map(|(_, dims)| dims)
        });

        #[cfg(feature = "profile")]
        if dims.is_none() {
            tracing::trace!("[WARN] Couldn't identify image dimensions!");
        }

        Ok(Self {
            name,
            path: path.to_path_buf(),
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
            terminated(Self::parse_decimal, tag("x")),
            Self::parse_decimal,
        ))
        .map(|(width, height)| Self { width, height })
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

    #[lerr(str = "{0}")]
    DB(#[lerr(from)] leaper_db::DBError),

    #[lerr(str = "[image] {0}")]
    Image(#[lerr(from, wrap = Arc)] image::ImageError),

    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::DecodeError),
    #[lerr(str = "[.desktop::decode] {0}")]
    DesktopEntryExecParse(#[lerr(from, wrap = Arc)] freedesktop_desktop_entry::ExecError),
}
