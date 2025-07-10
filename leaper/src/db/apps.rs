use std::path::{Path, PathBuf};

use freedesktop_desktop_entry::DesktopEntry;
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;
use surrealdb_extras::SurrealTable;

use crate::app::mode::apps::search::{AppsError, AppsResult};

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = app,
    sql(
        "DEFINE INDEX app_dep_ind ON TABLE app COLUMNS desktop_entry_path UNIQUE",
        "DEFINE INDEX app_name_ind ON TABLE app COLUMNS name UNIQUE",
        "
        DEFINE EVENT app_entry_added ON TABLE app
            WHEN $event = \"CREATE\" && $after.icon_name != NULL
            THEN (
                UPDATE $after.id SET icon = (SELECT VALUE id FROM ONLY icon
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

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = icon,
    sql(
        "DEFINE INDEX icon_path_ind ON TABLE icon COLUMNS path UNIQUE",
        "
        DEFINE EVENT icon_added ON TABLE icon
            WHEN $event = \"CREATE\"
            THEN (
                UPDATE app SET icon = $value.id WHERE icon_name = $value.name
            )
        "
    )
)]
pub struct AppIcon {
    pub name: String,
    pub path: PathBuf,
    pub svg: bool,
    pub xpm: bool,
}

impl AppIcon {
    pub fn new(path: impl AsRef<Path>) -> AppsResult<Self> {
        let path = path.as_ref();
        let name = path
            .file_stem()
            .ok_or_else(|| AppsError::NoFileName(path.to_path_buf()))?
            .to_string_lossy()
            .to_string();

        let ext = path.extension().and_then(|e| e.to_str());

        Ok(Self {
            name,
            path: path.to_path_buf(),
            svg: matches!(ext, Some("svg")),
            xpm: matches!(ext, Some("xpm")),
        })
    }
}
