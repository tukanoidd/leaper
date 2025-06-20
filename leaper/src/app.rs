mod apps;

use std::sync::Arc;

use directories::ProjectDirs;
use iced::{
    Event,
    keyboard::{self, Key, key},
    widget::text_input,
};
use leaper_apps::AppEntry;
use leaper_db::{DB, DBResult};
use tracing::Instrument;

use crate::app::apps::{Apps, AppsMsg};

pub type AppTheme = iced::Theme;
pub type AppRenderer = iced::Renderer;
pub type AppElement<'a> = iced::Element<'a, AppMsg, AppTheme, AppRenderer>;
pub type AppTask<Msg = AppMsg> = iced::Task<Msg>;
pub type AppSubscription<Msg = AppMsg> = iced::Subscription<Msg>;

pub struct App {
    db: Option<Arc<DB>>,

    apps: Apps,
}

impl App {
    pub fn new(project_dirs: ProjectDirs) -> (Self, AppTask) {
        let db_path = project_dirs.data_local_dir().join("db");

        let res = Self {
            db: None,
            apps: Default::default(),
        };
        let task = AppTask::batch([
            text_input::focus(Apps::SEARCH_ID),
            AppTask::perform(DB::init(db_path), |db| AppMsg::InitDB(db.map(Arc::new))),
        ]);

        (res, task)
    }

    pub fn update(&mut self, message: AppMsg) -> AppTask {
        match message {
            AppMsg::InitDB(db) => match db {
                Ok(db) => {
                    self.db = Some(db.clone());

                    return AppTask::perform(
                        {
                            let span = tracing::trace_span!("get_cached_list");
                            async move { db.get_table::<AppEntry>().await }.instrument(span)
                        },
                        AppsMsg::InitApps,
                    )
                    .map(Into::into);
                }
                Err(err) => {
                    tracing::error!("Failed to initialize the database: {err}");
                    return iced::exit();
                }
            },

            AppMsg::Apps(apps_msg) => return self.apps.update(apps_msg, &self.db),

            AppMsg::IcedEvent(ev) => {
                if let Event::Keyboard(event) = ev
                    && let keyboard::Event::KeyPressed { key, .. } = event
                {
                    match key.as_ref() {
                        Key::Named(key::Named::Escape) | Key::Character("q" | "Q") => {
                            return iced::exit();
                        }

                        Key::Named(key::Named::ArrowUp) => {
                            return AppTask::done(AppsMsg::SelectUp.into());
                        }
                        Key::Named(key::Named::ArrowDown) => {
                            return AppTask::done(AppsMsg::SelectDown.into());
                        }
                        Key::Named(key::Named::Enter) => {
                            return AppTask::done(AppsMsg::RunSelectedApp.into());
                        }

                        _ => {}
                    }
                }
            }

            AppMsg::AnchorChange(_)
            | AppMsg::SetInputRegion(_)
            | AppMsg::AnchorSizeChange(_, _)
            | AppMsg::LayerChange(_)
            | AppMsg::MarginChange(_)
            | AppMsg::SizeChange(_)
            | AppMsg::VirtualKeyboardPressed { .. } => {}
        }

        AppTask::none()
    }

    pub fn view(&self) -> AppElement<'_> {
        self.apps.view().map(Into::into)
    }

    pub fn theme(&self) -> AppTheme {
        AppTheme::TokyoNightStorm
    }

    pub fn subscription(&self) -> AppSubscription {
        iced::event::listen().map(AppMsg::IcedEvent)
    }
}

#[iced_layershell::to_layer_message]
#[derive(Debug, Clone)]
pub enum AppMsg {
    InitDB(DBResult<Arc<DB>>),

    Apps(AppsMsg),

    IcedEvent(Event),
}

macro_rules! into_app_msg {
    ($($name:ident($err:ty)),+) => {
        $(
            impl From<$err> for AppMsg {
                fn from(val: $err) -> Self {
                    Self::$name(val)
                }
            }
        )+
    };
}

into_app_msg![Apps(AppsMsg)];
