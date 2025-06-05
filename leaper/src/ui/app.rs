use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use directories::ProjectDirs;
use iced::{
    Color, Length,
    alignment::Horizontal,
    keyboard::key,
    widget::{column, horizontal_rule, scrollable, text_input},
};
use iced_layershell::{Appearance, Application, to_layer_message};
use itertools::Itertools;
use lcore::{
    modules::applications::AppEntry,
    state::app::{AppState, AppStateResult, AppTheme},
};

use crate::{
    cli::{Cli, LeaperMode},
    ui::util::{
        SELECTOR_BUTTON_HEIGHT, error_container, filter_input, info_container, main_container,
        selector_button,
    },
};

pub type AppExecutor = iced::executor::Default;
pub type AppTask = iced::Task<AppMsg>;
pub type AppRenderer = iced::Renderer;
pub type AppElement<'a> = iced::Element<'a, AppMsg, AppTheme, AppRenderer>;

static APP_FILTER_ID: LazyLock<text_input::Id> = LazyLock::new(text_input::Id::unique);
static APP_LIST_ID: LazyLock<scrollable::Id> = LazyLock::new(scrollable::Id::unique);

pub struct App {
    mode: LeaperMode,
    state: Option<Arc<AppState>>,

    filter: String,
    active_item: Option<usize>,
    app_entries: Option<AppStateResult<Vec<AppEntry>>>,
}

impl Application for App {
    type Executor = AppExecutor;
    type Message = AppMsg;
    type Theme = AppTheme;
    type Flags = AppFlags;

    fn new(
        AppFlags {
            cli: Cli { mode },
            project_dirs,
        }: Self::Flags,
    ) -> (Self, AppTask) {
        let res = Self {
            mode,
            state: None,

            active_item: None,
            filter: Default::default(),
            app_entries: None,
        };
        let task = AppTask::perform(AppState::new(project_dirs), AppMsg::State);

        (res, task)
    }

    fn namespace(&self) -> String {
        "com.tukanoid.leaper".into()
    }

    fn update(&mut self, message: Self::Message) -> iced::Task<Self::Message> {
        match message {
            AppMsg::IcedEvent(ev) => {
                if let iced::Event::Keyboard(event) = ev
                    && let iced::keyboard::Event::KeyPressed { key, .. } = event
                {
                    let list_len = match self.mode {
                        LeaperMode::Apps => self
                            .app_entries
                            .as_ref()
                            .and_then(|e| e.as_ref().ok())
                            .map(|e| e.len())
                            .unwrap_or_default(),
                        LeaperMode::Finder => {
                            // TODO
                            0
                        }
                    };

                    if let iced::keyboard::Key::Named(named) = key {
                        match named {
                            key::Named::Escape => {
                                return iced::exit();
                            }

                            key::Named::ArrowUp => {
                                self.active_item = (list_len > 0).then(|| match self.active_item {
                                    Some(0) | None => list_len - 1,
                                    Some(ai) => ai - 1,
                                });

                                if let Some(active) = self.active_item {
                                    return Self::scroll_to_active(active);
                                }
                            }
                            key::Named::ArrowDown => {
                                self.active_item = (list_len > 0).then(|| match self.active_item {
                                    Some(ai) => match ai == list_len - 1 {
                                        true => 0,
                                        false => ai + 1,
                                    },
                                    None => 0,
                                });

                                if let Some(active) = self.active_item {
                                    return Self::scroll_to_active(active);
                                }
                            }

                            key::Named::Enter => match self.mode {
                                LeaperMode::Apps => return AppTask::done(AppMsg::LaunchActiveApp),
                                LeaperMode::Finder => todo!(),
                            },

                            _ => {}
                        }
                    }
                }
            }

            AppMsg::State(res) => match res {
                Ok(state) => {
                    self.state = Some(Arc::new(state));

                    match self.mode {
                        LeaperMode::Apps => {
                            let cloned_state = self.state.clone().unwrap();

                            return AppTask::batch([
                                AppTask::done(AppMsg::GetAppList),
                                AppTask::perform(
                                    async move {
                                        cloned_state.apps.wait_for_refresh()().await;
                                    },
                                    |_| AppMsg::DoneWaitAppList,
                                ),
                            ]);
                        }
                        LeaperMode::Finder => todo!(),
                    }
                }
                Err(err) => {
                    tracing::error!("{err}");
                    return iced::exit();
                }
            },
            AppMsg::DoneWaitAppList => return AppTask::done(AppMsg::GetAppList),
            AppMsg::GetAppList => {
                let cloned_state = self.state.clone().unwrap();

                return AppTask::perform(
                    async move { cloned_state.apps_items().await },
                    AppMsg::AppList,
                );
            }
            AppMsg::AppList(res) => {
                self.app_entries = Some(res);

                tracing::debug!("{:#?}", self.app_entries);

                if self.app_entries.as_ref().unwrap().is_ok() {
                    return text_input::focus(APP_FILTER_ID.clone());
                }
            }

            AppMsg::Filter(filter) => {
                self.filter = filter;
                self.active_item = None;
            }

            AppMsg::LaunchActiveApp => {
                if let (Some(entries), ind) = (self.filtered_entries(), self.active_item)
                    && let Ok(entries) = entries
                    && let Some(entry) = entries.get(ind.unwrap_or_default())
                {
                    return AppTask::done(AppMsg::LaunchApp(entry.exe.clone()));
                }
            }
            AppMsg::LaunchApp(exe) => {
                let exe_str = exe.to_string_lossy().to_string();
                let Some(mut res) = shlex::split(&exe_str) else {
                    tracing::error!("Empty command!");
                    return iced::exit();
                };

                if res.is_empty() {
                    tracing::error!("Empty command!");
                    return iced::exit();
                }

                let command = res.remove(0);

                let mut cmd = std::process::Command::new(command);

                if !res.is_empty() {
                    cmd.args(res);
                }

                match cmd.spawn() {
                    Ok(_) => {
                        return iced::exit();
                    }
                    Err(err) => {
                        tracing::error!("Failed to launch the {exe:?}! Error: {err}",);
                    }
                }
            }

            AppMsg::OpenFile(_file) => {
                // TODO
            }
            AppMsg::OpenActiveFile => {
                // TODO
            }

            _ => {}
        }

        AppTask::none()
    }

    fn view(&self) -> iced::Element<'_, Self::Message, Self::Theme, iced::Renderer> {
        match &self.state {
            Some(_state) => match self.mode {
                LeaperMode::Apps => match &self.filtered_entries() {
                    Some(entries) => match entries {
                        Ok(entries) => main_container(
                            column![
                                filter_input()
                                    .id(APP_FILTER_ID.clone())
                                    .placeholder("Search for an app...")
                                    .value(&self.filter)
                                    .on_submit(match self.mode {
                                        LeaperMode::Apps => AppMsg::LaunchActiveApp,
                                        LeaperMode::Finder => AppMsg::OpenActiveFile,
                                    })
                                    .call(),
                                horizontal_rule(2),
                                scrollable(
                                    column(entries.iter().enumerate().map(
                                        |(ind, AppEntry { icon, name, exe })| {
                                            selector_button()
                                                .ind(ind)
                                                .name(name)
                                                .active_ind(&self.active_item)
                                                .icon(icon)
                                                .on_press(AppMsg::LaunchApp(exe.clone()))
                                                .call()
                                        }
                                    ))
                                    .width(Length::Fill)
                                )
                                .id(APP_LIST_ID.clone())
                                .spacing(5)
                                .width(Length::Fill)
                            ]
                            .height(Length::Fill)
                            .padding(100)
                            .align_x(Horizontal::Center)
                            .spacing(10),
                        ),

                        Err(err) => error_container()
                            .error(format!("Encountered an error getting app entries: {err}"))
                            .call(),
                    },
                    None => info_container().info("Loading entries...").call(),
                },
                LeaperMode::Finder => todo!(),
            },
            None => info_container().info("Loading state...").call(),
        }
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::event::listen().map(AppMsg::IcedEvent)
    }

    fn theme(&self) -> Self::Theme {
        self.state
            .as_ref()
            .map(|c| c.theme.clone())
            .unwrap_or(AppTheme::TokyoNight)
    }

    fn style(&self, theme: &Self::Theme) -> iced_layershell::Appearance {
        Appearance {
            background_color: Color::TRANSPARENT,
            text_color: theme.palette().text,
        }
    }
}

impl App {
    fn filtered_entries(&self) -> Option<AppStateResult<Vec<&AppEntry>>> {
        let trimmed_filter = self.filter.trim();

        if trimmed_filter.is_empty() {
            return self.app_entries.as_ref().map(|e| {
                e.as_ref()
                    .map_err(|err| err.clone())
                    .map(|e| e.iter().collect::<Vec<_>>())
            });
        }

        self.state.as_ref().and_then(|state| {
            self.app_entries.as_ref().map(|x| {
                x.as_ref().map_err(|err| err.clone()).map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| {
                            state
                                .apps
                                .match_(&self.filter, &entry.name)
                                .map(|score| (entry, score))
                        })
                        .sorted_by_key(|(_, score)| *score)
                        .rev()
                        .map(|(entry, _)| entry)
                        .collect::<Vec<_>>()
                })
            })
        })
    }

    fn scroll_to_active(active: usize) -> iced::Task<AppMsg> {
        scrollable::scroll_to(
            APP_LIST_ID.clone(),
            scrollable::AbsoluteOffset {
                x: 0.0,
                y: match active {
                    0 => 0.0,
                    _ => (active as f32 * SELECTOR_BUTTON_HEIGHT) + 5.0,
                },
            },
        )
    }
}

pub struct AppFlags {
    pub cli: Cli,
    pub project_dirs: ProjectDirs,
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum AppMsg {
    IcedEvent(iced::Event),

    State(AppStateResult<AppState>),

    DoneWaitAppList,
    GetAppList,
    AppList(AppStateResult<Vec<AppEntry>>),

    Filter(String),

    LaunchApp(PathBuf),
    LaunchActiveApp,

    OpenFile(PathBuf),
    OpenActiveFile,
}
