use std::path::{Path, PathBuf};

use freedesktop_desktop_entry::DesktopEntry;
use serde::{Deserialize, Serialize};
use surrealdb::RecordId;
use surrealdb_extras::{SurrealQuery, SurrealTable};

use crate::{DBError, DBResult};

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = app,
    sql(
        "DEFINE TABLE has_icon TYPE RELATION",
        "DEFINE INDEX app_dep_ind ON TABLE app COLUMNS desktop_entry_path UNIQUE",
        "DEFINE INDEX app_name_ind ON TABLE app COLUMNS name UNIQUE",
        "
        DEFINE EVENT app_entry_added ON TABLE app
            WHEN $event = 'CREATE' && $after.icon_name != NULL
            THEN {
                LET $icon = (SELECT * FROM icon
                    WHERE name == $value.icon_name
                    ORDER BY dims.width,dims.height,svg
                    LIMIT 1);

                IF $icon != NONE THEN
                    RELATE $value->has_icon->$icon;
                END;
            }
        "
    )
)]
pub struct AppEntry {
    pub id: RecordId,
    pub desktop_entry_path: PathBuf,
    pub name: String,
    pub exec: Vec<String>,
    pub icon_name: Option<String>,
}

#[derive(Debug, SurrealQuery)]
#[query(
    output = "Option<RecordId>",
    error = DBError,
    sql = "
        BEGIN TRANSACTION;

        LET $app = (CREATE app SET
            desktop_entry_path = {path},
            name = {name},
            exec = {exec},
            icon_name = {icon_name}).id;
        LET $file = (SELECT VALUE ->is_file->file.id FROM ONLY fs_node WHERE path == {path} LIMIT 1);

        RELATE $file->is_app->$app;

        COMMIT TRANSACTION;

        RETURN $app;
    "
)]
pub struct CreateAppEntryQuery {
    path: PathBuf,
    name: String,
    exec: Vec<String>,
    icon_name: Option<String>,
}

impl CreateAppEntryQuery {
    pub fn new(path: impl AsRef<Path>) -> DBResult<Self> {
        let path = path.as_ref();
        let entry = DesktopEntry::from_path::<&str>(path, None)?;
        let name = entry
            .full_name::<&str>(&[])
            .ok_or_else(|| DBError::DesktopEntryNoName(path.to_path_buf()))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Unknown".into());

        let exec = entry
            .exec()
            .map(
                |exec_str| match exec_str.split(" ").skip(1).any(|x| x.contains("%")) {
                    true => entry.parse_exec().map_err(DBError::from).or_else(|_| {
                        entry
                            .parse_exec_with_uris::<&str>(&[], &[])
                            .map_err(DBError::from)
                            .or_else(|_| {
                                entry
                                    .exec()
                                    .ok_or_else(|| DBError::DesktopEntryNoExec(path.into()))
                                    .and_then(|exec_str| {
                                        shlex::split(exec_str).ok_or_else(|| {
                                            DBError::DesktopEntryParseExec(
                                                path.to_path_buf(),
                                                exec_str.into(),
                                            )
                                        })
                                    })
                            })
                    }),
                    false => shlex::split(exec_str).ok_or_else(|| {
                        DBError::DesktopEntryParseExec(path.to_path_buf(), exec_str.into())
                    }),
                },
            )
            .transpose()?
            .ok_or_else(|| DBError::DesktopEntryNoExec(path.into()))?;

        let icon_name = entry.icon().map(|icon_name| icon_name.to_string());

        Ok(Self {
            path: path.into(),
            name,
            exec,
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

#[derive(Debug, SurrealQuery)]
#[query(
    output = "Vec<AppWithIcon>",
    error = DBError,
    sql = "
        SELECT *, ->has_icon->icon.*[0][0] as icon FROM app
            ORDER BY name ASC
    "
)]
pub struct GetAppWithIconsQuery;

#[derive(Debug, SurrealQuery)]
#[query(
    stream = "AppWithIcon",
    error = DBError,
    sql = "LIVE SELECT
        *,
        (SELECT * FROM ->has_icon->icon
            ORDER BY dims.width,dims.height,svg)[0][0] as icon
        FROM app"
)]
pub struct GetLiveAppWithIconsQuery;

#[derive(Debug, SurrealQuery)]
#[query(
    stream = "AppWithIcon",
    error = DBError,
    sql = "
        LIVE SELECT VALUE object::from_entries(array::concat(
            object::entries(in.*),
            [['icon', out]]
        )) FROM has_icon FETCH icon
    "
)]
pub struct GetLiveAppIconUpdates;

#[derive(Debug, Clone, SurrealTable, Serialize, Deserialize)]
#[table(
    db = icon,
    sql(
        "DEFINE INDEX icon_path_ind ON TABLE icon COLUMNS path UNIQUE",
        "
        DEFINE EVENT icon_added ON TABLE icon
            WHEN $event = 'CREATE'
            THEN {
                LET $app = (SELECT * FROM ONLY app
                    WHERE icon_name == $value.name LIMIT 1).id;

                IF $app != NONE THEN
                    RELATE $app->has_icon->$value;
                END;
            }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppIconDims {
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, SurrealQuery)]
#[query(
    stream = "PathBuf",
    error = DBError,
    sql = "
        LIVE SELECT VALUE in.path
            FROM is_file
            WHERE out.ext == 'desktop';
    "
)]
pub struct LiveSearchAppsQuery;
