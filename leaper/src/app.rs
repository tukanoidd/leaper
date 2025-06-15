use std::sync::Arc;

use directories::ProjectDirs;
use iced::{
    Event, Length,
    advanced::widget::{Id, operate, operation::scrollable::scroll_to},
    alignment::{Horizontal, Vertical},
    keyboard::{self, Key, key},
    widget::{button, center, column, horizontal_rule, image, row, scrollable, text, text_input},
};
use iced_aw::Spinner;
use iced_layershell::Application;
use itertools::Itertools;
use leaper_apps::{AppEntry, AppIcon, AppsResult, search_apps};
use leaper_db::{DB, DBResult, DBTableEntry, TDBEntryId};
use tokio::task::JoinSet;
use tracing::Instrument;

pub type AppTheme = iced::Theme;
pub type AppRenderer = iced::Renderer;
pub type AppElement<'a> = iced::Element<'a, AppMsg, AppTheme, AppRenderer>;
pub type AppTask = iced::Task<AppMsg>;

pub struct App {
    db: Option<Arc<DB>>,

    apps: AppsIcons,
    filtered: AppsIcons,

    search: String,
    matcher: nucleo::Matcher,
    selected: usize,
}

impl Application for App {
    type Executor = iced::executor::Default;
    type Message = AppMsg;
    type Theme = AppTheme;
    type Flags = AppFlags;

    fn new(AppFlags { project_dirs }: Self::Flags) -> (Self, iced::Task<Self::Message>) {
        let db_path = project_dirs.config_local_dir().join("db");

        let res = Self {
            db: None,

            apps: vec![],
            filtered: vec![],

            search: String::new(),
            matcher: nucleo::Matcher::new(nucleo::Config::DEFAULT),
            selected: 0,
        };
        let task = AppTask::batch([
            text_input::focus(Self::SEARCH_ID),
            AppTask::perform(DB::init(db_path), |db| AppMsg::InitDB(db.map(Arc::new))),
        ]);

        (res, task)
    }

    fn namespace(&self) -> String {
        "leaper".into()
    }

    fn update(&mut self, message: Self::Message) -> AppTask {
        match message {
            AppMsg::InitDB(db) => match db {
                Ok(db) => {
                    self.db = Some(db.clone());

                    return AppTask::perform(
                        {
                            let span = tracing::trace_span!("get_cached_list");

                            async move {
                                let apps = db.get_table::<AppEntry>().await?;

                                let app_icons = apps
                                    .into_iter()
                                    .fold(JoinSet::new(), |mut join_set, app| {
                                        let db = db.clone();
                                        let icon = app.icon.clone();

                                        join_set.spawn(async move {
                                            match icon {
                                                Some(icon) => DBResult::Ok((
                                                    app,
                                                    Some(db.entry::<AppIcon>(icon.uuid()).await?),
                                                )),
                                                None => Ok((app, None)),
                                            }
                                        });

                                        join_set
                                    })
                                    .join_all()
                                    .await
                                    .into_iter()
                                    .collect::<DBResult<Vec<_>>>()?;

                                Ok(app_icons)
                            }
                            .instrument(span)
                        },
                        AppMsg::InitApps,
                    );
                }
                Err(err) => {
                    tracing::error!("Failed to initialize the database: {err}");
                    return iced::exit();
                }
            },
            AppMsg::InitApps(apps) => match apps {
                Ok(apps) => {
                    self.apps = apps;

                    return AppTask::perform(
                        search_apps(self.db.clone().unwrap()),
                        AppMsg::LoadApps,
                    );
                }
                Err(err) => {
                    tracing::error!("Failed to initialize app list from cache: {err}");
                    return iced::exit();
                }
            },
            AppMsg::LoadApps(apps) => match apps {
                Ok(apps) => {
                    self.apps = apps;
                    self.selected = self.selected.clamp(0, self.apps.len() - 1);
                }
                Err(err) => {
                    tracing::error!("Failed to load new app list: {err}");
                    return iced::exit();
                }
            },

            AppMsg::SearchInput(new_search) => {
                self.search = new_search;

                self.filtered = match self.search.as_str() {
                    "" => {
                        self.selected = match self.apps.len() {
                            0 => 0,
                            len => self.selected.clamp(0, len - 1),
                        };

                        vec![]
                    }
                    search => {
                        self.selected = match self.filtered.len() {
                            0 => 0,
                            len => self.selected.clamp(0, len - 1),
                        };

                        self.apps
                            .iter()
                            .filter_map(|(app, icon)| {
                                self.matcher
                                    .fuzzy_match(
                                        nucleo::Utf32Str::new(&app.name, &mut vec![]),
                                        nucleo::Utf32Str::new(&search.to_lowercase(), &mut vec![]),
                                    )
                                    .map(|score| (score, app, icon))
                            })
                            .sorted_by_key(|(score, _, _)| *score)
                            .rev()
                            .map(|(_, app, icon)| (app.clone(), icon.clone()))
                            .collect()
                    }
                };
            }

            AppMsg::RunSelectedApp => match self.apps.is_empty() {
                true => {}
                false => return AppTask::done(AppMsg::RunApp(self.selected)),
            },
            AppMsg::RunApp(ind) => match {
                match self.search.is_empty() {
                    true => &self.apps,
                    false => &self.filtered,
                }
            }
            .get(ind)
            {
                Some((app, _)) => {
                    let cmd = &app.exec[0];
                    let args = match app.exec.len() {
                        1 => None,
                        _ => Some(app.exec[1..].iter()),
                    };

                    let mut cmd = std::process::Command::new(cmd);

                    if let Some(args) = args {
                        cmd.args(args);
                    }

                    match cmd.spawn() {
                        Ok(_) => {
                            tracing::trace!("Running {}", app.name);
                        }
                        Err(err) => tracing::error!("Failed to run the app {}: {err}", app.name),
                    }

                    return iced::exit();
                }
                None => tracing::warn!("Logic error!"),
            },
            AppMsg::ScrollToSelected => match self.apps.is_empty() {
                true => {}
                false => {
                    let y_offset = self.selected as f32 * Self::APP_ENTRY_HEIGHT;

                    return operate(scroll_to(
                        Id::new(Self::LIST_ID),
                        scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: y_offset,
                        },
                    ));
                }
            },

            AppMsg::IcedEvent(ev) => {
                if let Event::Keyboard(event) = ev
                    && let keyboard::Event::KeyPressed { key, .. } = event
                {
                    match key.as_ref() {
                        Key::Named(key::Named::Escape) | Key::Character("q" | "Q") => {
                            return iced::exit();
                        }

                        Key::Named(key::Named::ArrowUp) => {
                            self.selected = match self.apps.is_empty() {
                                true => 0,
                                false => match self.selected {
                                    0 => self.apps.len() - 1,
                                    x => x - 1,
                                },
                            };

                            return AppTask::done(AppMsg::ScrollToSelected);
                        }
                        Key::Named(key::Named::ArrowDown) => {
                            self.selected = match self.apps.is_empty() {
                                true => 0,
                                false => match self.selected >= self.apps.len() - 1 {
                                    true => 0,
                                    false => self.selected + 1,
                                },
                            };

                            return AppTask::done(AppMsg::ScrollToSelected);
                        }
                        Key::Named(key::Named::Enter) => {
                            return AppTask::done(AppMsg::RunSelectedApp);
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

    fn view(&self) -> AppElement<'_> {
        column![self.search(), horizontal_rule(2), self.list()]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .spacing(10)
            .into()
    }

    fn theme(&self) -> Self::Theme {
        AppTheme::TokyoNightStorm
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::event::listen().map(AppMsg::IcedEvent)
    }
}

impl App {
    const SEARCH_ID: &'static str = "search_input";
    const LIST_ID: &'static str = "list";

    fn search(&self) -> AppElement<'_> {
        center(
            text_input("Search for an app...", &self.search)
                .id(text_input::Id::new(Self::SEARCH_ID))
                .on_input_maybe((!self.apps.is_empty()).then_some(AppMsg::SearchInput))
                .on_submit(AppMsg::RunSelectedApp)
                .size(25)
                .padding(10),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .padding(10)
        .into()
    }

    fn list(&self) -> AppElement<'_> {
        let (items, filtered) = match self.search.is_empty() {
            true => (&self.apps, false),
            false => (&self.filtered, true),
        };

        let scrllbl = || {
            scrollable(
                column(
                    items
                        .iter()
                        .enumerate()
                        .map(|(ind, (app, icon))| Self::app_entry(app, icon, ind, self.selected)),
                )
                .align_x(Horizontal::Center),
            )
            .id(scrollable::Id::new(Self::LIST_ID))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        match filtered {
            true => match items.is_empty() {
                true => center(text("No matches found!").size(25)).into(),
                false => scrllbl(),
            },
            false => match items.is_empty() {
                true => center(
                    row![
                        Spinner::new().width(30).height(30),
                        text("Loading...").size(20)
                    ]
                    .align_y(Vertical::Center)
                    .spacing(10),
                )
                .into(),
                false => scrllbl(),
            },
        }
    }

    const APP_ENTRY_HEIGHT: f32 = 50.0;
    const APP_ENTRY_PADDING: [f32; 2] = [10.0, 5.0];
    const APP_ENTRY_SPACING: f32 = 10.0;
    const APP_ENTRY_IMAGE_SIZE: f32 = Self::APP_ENTRY_HEIGHT - Self::APP_ENTRY_PADDING[1] * 2.0;
    const APP_ENTRY_TEXT_HEIGHT: f32 = Self::APP_ENTRY_IMAGE_SIZE * 0.5;

    fn app_entry<'a>(
        app: &'a DBTableEntry<AppEntry>,
        icon: &'a Option<DBTableEntry<AppIcon>>,
        ind: usize,
        selected: usize,
    ) -> AppElement<'a> {
        let r = match icon {
            Some(icon) => row![
                image(&icon.path)
                    .width(Self::APP_ENTRY_IMAGE_SIZE)
                    .height(Self::APP_ENTRY_IMAGE_SIZE)
            ],
            None => row![],
        }
        .push(text(&app.name).size(Self::APP_ENTRY_TEXT_HEIGHT))
        .height(Length::Fill)
        .spacing(Self::APP_ENTRY_SPACING)
        .align_y(Vertical::Center);

        button(
            center(r)
                .width(Length::Fill)
                .padding(Self::APP_ENTRY_PADDING),
        )
        .on_press(AppMsg::RunApp(ind))
        .style(move |theme, status| {
            let status = match selected == ind {
                true => button::Status::Hovered,
                false => status,
            };

            button::secondary(theme, status)
        })
        .height(Length::Fixed(Self::APP_ENTRY_HEIGHT))
        .width(Length::Fill)
        .into()
    }
}

pub struct AppFlags {
    pub project_dirs: ProjectDirs,
}

type AppsIcons = Vec<(DBTableEntry<AppEntry>, Option<DBTableEntry<AppIcon>>)>;

type InitAppsIconsResult = DBResult<AppsIcons>;
type LoadAppsIconsResult = AppsResult<AppsIcons>;

#[iced_layershell::to_layer_message]
#[derive(Debug, Clone)]
pub enum AppMsg {
    InitDB(DBResult<Arc<DB>>),
    InitApps(InitAppsIconsResult),
    LoadApps(LoadAppsIconsResult),

    SearchInput(String),

    RunSelectedApp,
    RunApp(usize),
    ScrollToSelected,

    IcedEvent(Event),
}
