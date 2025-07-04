use std::path::{Path, PathBuf};

use freedesktop_desktop_entry::DesktopEntry;
use macros::db_table;
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
use surrealdb::RecordId;

use crate::app::mode::apps::search::{AppsError, AppsResult};

#[db_table]
#[table(
    db = entries,
    sql(
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
    )
)]
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

#[db_table]
#[table(
    db = icons,
    sql(
        "DEFINE INDEX icon_path_ind ON TABLE icons COLUMNS path UNIQUE",
        "
        DEFINE EVENT icon_added ON TABLE icons
            WHEN $event = \"CREATE\"
            THEN (
                UPDATE entries SET icon = $value.id WHERE icon_name = $value.name
            )
        "
    )
)]
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
