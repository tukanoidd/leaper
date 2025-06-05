use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use applications::{AppInfo, AppInfoContext};
use iced::futures::{FutureExt, future::BoxFuture, lock::Mutex};
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    db_env,
    state::db::{DB, DBError},
};

#[derive(Debug, Clone)]
pub struct Applications {
    ctx: AppInfoContext,

    matcher: Arc<Mutex<nucleo::Matcher>>,
}

impl Applications {
    pub fn new() -> Self {
        let mut ctx = AppInfoContext::new(
            std::env::var("XDG_DATA_DIRS")
                .ok()
                .iter()
                .flat_map(|s| s.split(":"))
                .filter_map(|x| match x.ends_with("applications") {
                    true => Path::new(x).exists().then(|| PathBuf::from(x)),
                    false => {
                        let path = Path::new(x).join("applications");
                        path.exists().then_some(path)
                    }
                })
                .map(|path| applications::common::SearchPath::new(path, 5))
                .collect(),
        );
        ctx.refresh_apps_in_background();

        let matcher = Arc::new(Mutex::new(nucleo::Matcher::new(nucleo::Config::DEFAULT)));

        Self { ctx, matcher }
    }

    pub async fn items(&self, db: &DB) -> ApplicationsResult<Vec<AppEntry>> {
        match self
            .ctx
            .refreshing
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            true => {
                tracing::debug!("Getting list from cache...");

                Ok(db.table::<CacheDBTable>().call().await?)
            }
            false => {
                tracing::debug!("Getting the list from lib...");

                let apps = self
                    .ctx
                    .get_all_apps()
                    .into_iter()
                    .filter_map(
                        |applications::App {
                             name,
                             icon_path,
                             app_path_exe,
                             ..
                         }| {
                            (!name.is_empty())
                                .then(|| {
                                    app_path_exe.and_then(|exe| {
                                        (!exe.to_string_lossy().is_empty())
                                            .then(|| AppEntry::new(icon_path, name, exe))
                                    })
                                })
                                .flatten()
                        },
                    )
                    .collect::<Vec<_>>();
                db.update_table::<CacheDBTable>(apps.clone()).await?;

                Ok(apps)
            }
        }
    }

    pub fn wait_for_refresh<'a>(&self) -> impl Fn() -> BoxFuture<'a, ()> {
        || {
            tracing::debug!("Waiting for app list to refresh...");

            let ctx_refreshing = self.ctx.refreshing.clone();

            async move {
                while ctx_refreshing.load(std::sync::atomic::Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }

                tracing::debug!("Finished waiting for app list to refresh!");
            }
            .boxed()
        }
    }

    pub fn match_(&self, needle: impl AsRef<str>, haystack: impl AsRef<str>) -> Option<u16> {
        let mut needle_buf = vec![];
        let mut haystack_buf = vec![];

        let mut matcher = self.matcher.try_lock();

        if matcher.is_none() {
            loop {
                if let Some(m) = self.matcher.try_lock() {
                    matcher = Some(m);
                    break;
                }
            }
        }

        matcher.unwrap().fuzzy_match(
            nucleo::Utf32Str::new(haystack.as_ref(), &mut haystack_buf),
            nucleo::Utf32Str::new(needle.as_ref(), &mut needle_buf),
        )
    }
}

impl Default for Applications {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub icon: Option<PathBuf>,
    pub name: String,
    pub exe: PathBuf,
}

impl AppEntry {
    fn new(icon: Option<PathBuf>, name: impl Into<String>, exe: PathBuf) -> Self {
        Self {
            icon,
            name: name.into(),
            exe,
        }
    }
}

db_env!(Applications {
    Cache = AppEntry ["app"]
});

pub type ApplicationsResult<T> = Result<T, ApplicationsError>;

#[derive(Debug, Clone, Error, Diagnostic)]
pub enum ApplicationsError {
    #[error("[applications] {0}")]
    DB(#[from] DBError),
}
