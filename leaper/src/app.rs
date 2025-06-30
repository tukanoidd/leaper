mod mode;

mod style;
mod types;

use std::sync::Arc;

use directories::ProjectDirs;
use iced::{
    Event, Task,
    keyboard::{self, Key, key},
    widget::text_input,
};
use leaper_db::{DB, DBResult};

use crate::{
    app::mode::{
        AppMode, AppModeMsg, AppModeTask,
        apps::{Apps, AppsMsg},
        power::PowerMsg,
        runner::Runner,
    },
    cli,
    config::Config,
};

pub type AppTheme = iced::Theme;
pub type AppRenderer = iced::Renderer;
pub type AppElement<'a> = iced::Element<'a, AppMsg, AppTheme, AppRenderer>;
pub type AppTask<Msg = AppMsg> = iced::Task<Msg>;
pub type AppSubscription<Msg = AppMsg> = iced::Subscription<Msg>;

pub struct App {
    config: Arc<Config>,

    db: Option<Arc<DB>>,

    mode: AppMode,
}

#[bon::bon]
impl App {
    #[builder]
    pub fn new(project_dirs: ProjectDirs, config: Config, mode: cli::AppMode) -> (Self, AppTask) {
        let db_path = project_dirs.data_local_dir().join("db");

        let task = match mode {
            cli::AppMode::Apps => {
                let init_db_task =
                    AppTask::perform(DB::init(db_path), |db| AppMsg::InitDB(db.map(Arc::new)));

                AppTask::batch([text_input::focus(Apps::SEARCH_ID), init_db_task])
            }
            cli::AppMode::Runner => text_input::focus(Runner::INPUT_ID),
            cli::AppMode::Power => AppModeTask::done(PowerMsg::ConnectZbus).map(Into::into),
        };

        let res = Self {
            config: Arc::new(config),

            db: None,

            mode: mode.into(),
        };

        (res, task)
    }

    pub fn update(&mut self, message: AppMsg) -> AppTask {
        match message {
            AppMsg::Exit => return iced::exit(),

            AppMsg::InitDB(db) => match db {
                Ok(db) => {
                    self.db = Some(db.clone());
                    return AppModeTask::done(AppsMsg::InitApps(db)).map(Into::into);
                }
                Err(err) => {
                    tracing::error!("Failed to initialize the database: {err}");
                    return Task::done(AppMsg::Exit);
                }
            },

            AppMsg::Mode(mode_msg) => {
                return self
                    .mode
                    .update(mode_msg, self.config.clone())
                    .map(Into::into);
            }

            AppMsg::IcedEvent(ev) => {
                if let Event::Keyboard(event) = ev
                    && let keyboard::Event::KeyPressed { key, .. } = event
                {
                    match key.as_ref() {
                        Key::Named(key::Named::Escape) | Key::Character("q" | "Q") => {
                            return Task::done(AppMsg::Exit);
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
        self.mode.view().map(Into::into)
    }

    pub fn theme(&self) -> AppTheme {
        self.config.theme.clone()
    }

    pub fn subscription(&self) -> AppSubscription {
        iced::event::listen().map(AppMsg::IcedEvent)
    }
}

#[iced_layershell::to_layer_message]
#[derive(Debug, Clone)]
pub enum AppMsg {
    Exit,

    InitDB(DBResult<Arc<DB>>),

    Mode(AppModeMsg),

    IcedEvent(Event),
}
