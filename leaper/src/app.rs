pub mod mode;

mod style;
mod types;

use std::sync::Arc;

use directories::ProjectDirs;
use iced::{
    Event, Task,
    futures::{SinkExt, StreamExt},
    keyboard::{self, Key, key},
    stream,
    widget::text_input,
};
use surrealdb::Notification;

use crate::{
    LeaperResult,
    app::mode::{
        AppMode, AppModeMsg, AppModeTask,
        apps::{Apps, AppsMsg},
        power::PowerMsg,
        runner::Runner,
    },
    cli,
    config::Config,
    db::{DB, apps::AppWithIcon, init_db},
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
        #[cfg(feature = "db-websocket")]
        let _project_dirs = project_dirs;

        let task = match mode {
            cli::AppMode::Apps => {
                let init_db_task = AppTask::perform(
                    init_db(
                        #[cfg(not(feature = "db-websocket"))]
                        project_dirs,
                    ),
                    AppMsg::InitDB,
                );

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
            AppMsg::Exit {
                mut app_search_stop_sender,
            } => {
                let stop_task = app_search_stop_sender.take().map(|sender| {
                    Task::perform(
                        async move {
                            sender.send(()).await?;

                            Ok(())
                        },
                        AppMsg::Result,
                    )
                });

                return match stop_task {
                    Some(stop_task) => Task::batch([stop_task, iced::exit()]),
                    None => iced::exit(),
                };
            }

            AppMsg::InitDB(db) => match db {
                Ok(db) => {
                    self.db = Some(db.clone());
                    return AppModeTask::done(AppsMsg::InitApps(db)).map(Into::into);
                }
                Err(err) => {
                    tracing::error!("Failed to initialize the database: {err}");
                    return Task::done(AppMsg::Exit {
                        app_search_stop_sender: match &self.mode {
                            AppMode::Apps(apps) => apps.stop_search_sender.clone(),
                            _ => Default::default(),
                        },
                    });
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
                            return Task::done(AppMsg::Exit {
                                app_search_stop_sender: match &self.mode {
                                    AppMode::Apps(apps) => apps.stop_search_sender.clone(),
                                    _ => Default::default(),
                                },
                            });
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

            AppMsg::Result(result) => {
                if let Err(result) = result {
                    tracing::error!("{result}");
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
        let iced_events = iced::event::listen().map(AppMsg::IcedEvent);

        match &self.db {
            Some(db) => {
                let db = db.clone();

                let stop_sender = match &self.mode {
                    AppMode::Apps(apps) => apps.stop_search_sender.clone(),
                    _ => Default::default(),
                };

                AppSubscription::batch([
                    iced_events,
                    AppSubscription::run_with_id(
                        "live_apps",
                        stream::channel(1, |mut msg_sender| async move {
                            match db
                                .query("LIVE SELECT * FROM app FETCH icon")
                                .await
                                .expect("Should be able to get live stream from app apps table")
                                .stream::<Notification<AppWithIcon>>(0)
                            {
                                Ok(mut stream) => {
                                    while let Some(notification) = stream.next().await {
                                        match notification {
                                            Ok(notification) => match notification.action {
                                                surrealdb::Action::Create
                                                | surrealdb::Action::Update => {
                                                    if let Err(err) = msg_sender
                                                        .send(
                                                            AppModeMsg::Apps(AppsMsg::AddApp(
                                                                notification.data,
                                                            ))
                                                            .into(),
                                                        )
                                                        .await
                                                    {
                                                        tracing::error!(
                                                            "Failed to send add app from live app table subscription: {err}"
                                                        );

                                                        if let Err(err) = msg_sender
                                                            .send(AppMsg::Exit {
                                                                app_search_stop_sender: stop_sender
                                                                    .clone(),
                                                            })
                                                            .await
                                                        {
                                                            tracing::error!(
                                                                "Failed to send exit message from live app table subscription: {err}"
                                                            );
                                                        }
                                                    }
                                                }
                                                _ => unreachable!(),
                                            },
                                            Err(err) => {
                                                tracing::error!(
                                                    "Failed to get notification from apps live table: {err}"
                                                );

                                                if let Err(err) = msg_sender
                                                    .send(AppMsg::Exit {
                                                        app_search_stop_sender: stop_sender.clone(),
                                                    })
                                                    .await
                                                {
                                                    tracing::error!(
                                                        "Failed to send exit message from live app table subscription: {err}"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::error!("Failed to get live table for apps: {err}");

                                    if let Err(err) = msg_sender
                                        .send(AppMsg::Exit {
                                            app_search_stop_sender: stop_sender,
                                        })
                                        .await
                                    {
                                        tracing::error!(
                                            "Failed to send exit message from live app table subscription: {err}"
                                        );
                                    }
                                }
                            }
                        }),
                    ),
                ])
            }
            None => iced_events,
        }
    }
}

#[iced_layershell::to_layer_message]
#[derive(derive_more::Debug, Clone)]
pub enum AppMsg {
    Exit {
        #[debug(skip)]
        app_search_stop_sender: Option<tokio_mpmc::Sender<()>>,
    },

    InitDB(LeaperResult<Arc<DB>>),

    Mode(AppModeMsg),

    IcedEvent(Event),

    Result(LeaperResult<()>),
}
