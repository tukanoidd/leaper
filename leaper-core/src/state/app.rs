use directories::ProjectDirs;
use miette::Diagnostic;
use thiserror::Error;

use crate::{
    config::{Config, ConfigError, modules::builtins::Builtins},
    modules::applications::{AppEntry, Applications, ApplicationsError},
    state::db::{DB, DBError},
};

pub type AppTheme = iced::Theme;

#[derive(Debug, Clone)]
pub struct AppState {
    pub terminal: String,
    pub theme: AppTheme,

    pub db: DB,

    pub apps: Applications,
}

impl AppState {
    pub async fn new(project_dirs: ProjectDirs) -> AppStateResult<Self> {
        let Config {
            builtins: Builtins { finder },
            terminal,
            theme,
        } = Config::open(&project_dirs).await?;

        let db = DB::new(&project_dirs).await?;

        let apps = Applications::new();

        Ok(Self {
            terminal,
            theme: theme.into(),

            db,

            apps,
        })
    }

    pub async fn apps_items(&self) -> AppStateResult<Vec<AppEntry>> {
        Ok(self.apps.items(&self.db).await?)
    }
}

pub type AppStateResult<T> = Result<T, AppStateError>;

#[derive(Debug, Clone, Error, Diagnostic)]
pub enum AppStateError {
    #[error("[app] {0}")]
    Config(#[from] ConfigError),
    #[error("[app] {0}")]
    DB(#[from] DBError),
    #[error("[app] {0}")]
    Applications(#[from] ApplicationsError),
}
