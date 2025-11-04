mod search;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use dashmap::DashMap;
use derive_more::Debug;
use directories::ProjectDirs;
use futures::SinkExt;
use iced::{
    Event, Length,
    advanced::widget::{Id, operate, operation::scrollable::scroll_to},
    alignment::{Horizontal, Vertical},
    keyboard::{self, Key, key},
    stream,
    widget::{
        button, center, column, horizontal_rule, image, row, scrollable, svg, text, text_input,
    },
};
use iced_aw::Spinner;
use iced_fonts::{NERD_FONT, Nerd, nerd::icon_to_string};
use iced_layershell::{
    build_pattern::MainSettings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::{LayerShellSettings, Settings, StartMode},
    to_layer_message,
};
use itertools::Itertools;
use tokio_stream::StreamExt;

use db::{
    DB, DBAction, DBResult, InstrumentedDBQuery,
    apps::{AppWithIcon, GetAppWithIconsQuery, GetLiveAppIconUpdates, GetLiveAppWithIconsQuery},
    init_db,
};
use executor::LeaperExecutor;
use macros::lerror;
use mode::{
    LeaperMode, LeaperModeTheme,
    config::{LeaperAppModeConfigError, LeaperModeConfig},
};

use crate::search::AppsFinder;

type AppsIcons = Vec<AppWithIcon>;

type InitAppsIconsResult = DBResult<AppsIcons>;
type LoadAppsIconsResult = LeaperLauncherResult<()>;

#[derive(Default)]
pub struct LeaperLauncher {
    config: LeaperModeConfig,
    db: Option<DB>,

    apps: AppsIcons,
    filtered: AppsIcons,

    search: String,
    matcher: nucleo::Matcher,
    selected: usize,

    xpm_handles: Arc<Mutex<DashMap<PathBuf, image::Handle>>>,

    pub stop_search_sender: Option<tokio_mpmc::Sender<()>>,
}

impl LeaperMode for LeaperLauncher {
    type RunError = LeaperLauncherError;
    type Task = iced::Task<Self::Msg>;

    type Subscription = iced::Subscription<Self::Msg>;

    type Renderer = iced::Renderer;

    type Element<'a>
        = iced::Element<'a, Self::Msg, LeaperModeTheme, Self::Renderer>
    where
        Self: 'a;

    type Msg = LeaperLauncherMsg;

    fn run() -> Result<(), Self::RunError> {
        let Settings {
            fonts,
            default_font,
            default_text_size,
            antialiasing,
            virtual_keyboard_support,
            ..
        } = Settings::<()>::default();

        let settings = MainSettings {
            id: Some("com.tukanoid.leaper-launcher".into()),
            layer_settings: LayerShellSettings {
                anchor: Anchor::empty(),
                layer: Layer::Overlay,
                exclusive_zone: 0,
                size: Some((500, 800)),
                margin: (0, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                start_mode: StartMode::Active,
                events_transparent: false,
            },
            fonts,
            default_font,
            default_text_size,
            antialiasing,
            virtual_keyboard_support,
        };

        let project_dirs = Self::project_dirs();
        let config = LeaperModeConfig::open(&project_dirs)?;

        iced_layershell::build_pattern::application(Self::title, Self::update, Self::view)
            .settings(settings)
            .theme(Self::theme)
            .subscription(Self::subscription)
            .font(iced_fonts::REQUIRED_FONT_BYTES)
            .font(iced_fonts::NERD_FONT_BYTES)
            .executor::<LeaperExecutor>()
            .run_with(move || Self::init(project_dirs, config))?;

        Ok(())
    }

    fn init(project_dirs: ProjectDirs, config: LeaperModeConfig) -> (Self, Self::Task)
    where
        Self: Sized,
    {
        #[cfg(feature = "db-websocket")]
        let _project_dirs = project_dirs;

        let launcher = Self {
            config,
            ..Default::default()
        };
        let task = {
            let init_db_task = Self::Task::perform(
                init_db(
                    #[cfg(not(feature = "db-websocket"))]
                    project_dirs,
                ),
                Self::Msg::InitDB,
            );

            Self::Task::batch([text_input::focus(Self::SEARCH_ID), init_db_task])
        };

        (launcher, task)
    }

    fn view(&self) -> Self::Element<'_> {
        column![self.search(), horizontal_rule(2), self.list()]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .spacing(10)
            .into()
    }

    fn update(&mut self, msg: Self::Msg) -> Self::Task {
        match msg {
            Self::Msg::Exit {
                mut app_search_stop_sender,
            } => {
                let stop_task = app_search_stop_sender.take().map(|sender| {
                    Self::Task::perform(
                        async move {
                            sender.send(()).await?;

                            Ok(())
                        },
                        Self::Msg::Result,
                    )
                });

                return match stop_task {
                    Some(stop_task) => Self::Task::batch([stop_task, iced::exit()]),
                    None => iced::exit(),
                };
            }
            Self::Msg::InitDB(db) => match db {
                Ok(db) => {
                    self.db = Some(db.clone());
                    return Self::Task::done(Self::Msg::InitApps).map(Into::into);
                }
                Err(err) => {
                    tracing::error!("Failed to initialize the database: {err}");
                    return Self::Task::done(Self::Msg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
            },
            Self::Msg::InitApps => {
                let (apps_finder, sender) = AppsFinder::new();

                self.stop_search_sender = Some(sender);

                return Self::Task::batch([
                    Self::Task::perform(
                        GetAppWithIconsQuery
                            .instrumented_execute(self.db.clone().expect("db is available")),
                        Self::Msg::InitedApps,
                    )
                    .map(Into::into),
                    Self::Task::done(Self::Msg::LoadApps(apps_finder)),
                ]);
            }
            Self::Msg::InitedApps(apps) => match apps {
                Ok(apps) => {
                    self.apps = apps;

                    tracing::trace!(
                        "Initialized apps list from cache [{} apps]",
                        self.apps.len()
                    );
                }
                Err(err) => {
                    tracing::error!("Failed to initialize app list from cache: {err}");

                    return Self::Task::done(Self::Msg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
            },

            Self::Msg::LoadApps(apps_finder) => {
                return Self::Task::perform(
                    apps_finder.search(self.db.clone().expect("db is available")),
                    Self::Msg::LoadedApps,
                )
                .map(Into::into);
            }
            Self::Msg::LoadedApps(apps) => match apps {
                Ok(_) => {
                    tracing::trace!("AppsFinder succeded!");
                }
                Err(err) => {
                    tracing::error!("AppsFinder errored out: {err}");

                    return Self::Task::done(Self::Msg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
            },

            Self::Msg::AddApp(app_with_icon) => {
                let existing_ind = self
                    .apps
                    .iter()
                    .enumerate()
                    .find_map(|(ind, app)| (app.id == app_with_icon.id).then_some(ind));

                match existing_ind {
                    Some(ind) => {
                        self.apps[ind] = app_with_icon;
                    }
                    None => {
                        self.apps.push(app_with_icon);
                        self.apps.sort_by_key(|x| x.name.clone());
                    }
                }
            }

            Self::Msg::SearchInput(new_search) => {
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
                            .filter_map(|app| {
                                self.matcher
                                    .fuzzy_match(
                                        nucleo::Utf32Str::new(&app.name, &mut vec![]),
                                        nucleo::Utf32Str::new(&search.to_lowercase(), &mut vec![]),
                                    )
                                    .map(|score| (score, app))
                            })
                            .sorted_by_key(|(score, _)| *score)
                            .rev()
                            .map(|(_, app)| app.clone())
                            .collect()
                    }
                };

                self.selected = self.selected.clamp(
                    0,
                    match self.search.is_empty() {
                        true => self.apps.len(),
                        false => self.filtered.len(),
                    } - 1,
                );
            }
            Self::Msg::SelectUp => {
                let len = match self.search.is_empty() {
                    true => self.apps.len(),
                    false => self.filtered.len(),
                };

                self.selected = match len == 0 {
                    true => 0,
                    false => match self.selected {
                        0 => len - 1,
                        x => x - 1,
                    },
                };

                return Self::Task::done(Self::Msg::ScrollToSelected).map(Into::into);
            }
            Self::Msg::SelectDown => {
                let len = match self.search.is_empty() {
                    true => self.apps.len(),
                    false => self.filtered.len(),
                };

                self.selected = match len == 0 {
                    true => 0,
                    false => match self.selected >= len - 1 {
                        true => 0,
                        false => self.selected + 1,
                    },
                };

                return Self::Task::done(Self::Msg::ScrollToSelected).map(Into::into);
            }

            Self::Msg::RunSelectedApp => match self.apps.is_empty() {
                true => {}
                false => return Self::Task::done(Self::Msg::RunApp(self.selected)).map(Into::into),
            },
            Self::Msg::RunApp(ind) => match {
                match self.search.is_empty() {
                    true => &self.apps,
                    false => &self.filtered,
                }
            }
            .get(ind)
            {
                Some(app) => {
                    tracing::trace!("Running {}: {:?}", app.name, app.exec);

                    let cmd = &app.exec[0];
                    let args = match app.exec.len() {
                        1 => None,
                        _ => Some(app.exec[1..].iter()),
                    };

                    let mut cmd = std::process::Command::new(cmd);

                    if let Some(args) = args {
                        cmd.args(args);
                    }

                    if let Err(err) = cmd.spawn() {
                        tracing::error!("Failed to run the app {}: {err}", app.name)
                    }

                    return Self::Task::done(Self::Msg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
                None => tracing::warn!("Logic error!"),
            },

            Self::Msg::ScrollToSelected => {
                if !self.apps.is_empty() {
                    let y_offset = self.selected as f32 * Self::APP_ENTRY_HEIGHT;

                    return operate(scroll_to(
                        Id::new(Self::LIST_ID),
                        scrollable::AbsoluteOffset {
                            x: 0.0,
                            y: y_offset,
                        },
                    ));
                }
            }

            Self::Msg::IcedEvent(event) => {
                if let Event::Keyboard(event) = event
                    && let keyboard::Event::KeyPressed { key, .. } = event
                {
                    match key.as_ref() {
                        Key::Named(key::Named::Escape) | Key::Character("q" | "Q") => {
                            return Self::Task::done(Self::Msg::Exit {
                                app_search_stop_sender: self.stop_search_sender.clone(),
                            });
                        }

                        Key::Named(key::Named::ArrowUp) => {
                            return Self::Task::done(Self::Msg::SelectUp);
                        }
                        Key::Named(key::Named::ArrowDown) => {
                            return Self::Task::done(Self::Msg::SelectDown);
                        }
                        Key::Named(key::Named::Enter) => {
                            return Self::Task::done(Self::Msg::RunSelectedApp);
                        }

                        _ => {}
                    }
                }
            }

            Self::Msg::Result(result) => {
                if let Err(result) = result {
                    tracing::error!("{result}");
                }
            }

            Self::Msg::AnchorChange(_)
            | Self::Msg::SetInputRegion(_)
            | Self::Msg::AnchorSizeChange(_, _)
            | Self::Msg::LayerChange(_)
            | Self::Msg::MarginChange(_)
            | Self::Msg::SizeChange(_)
            | Self::Msg::VirtualKeyboardPressed { .. } => {}
        }

        Self::Task::none()
    }

    fn subscription(&self) -> Self::Subscription {
        let iced_events = iced::event::listen().map(Self::Msg::IcedEvent);

        match &self.db {
            Some(db) => {
                let db = db.clone();
                let stop_sender = self.stop_search_sender.clone();

                Self::Subscription::batch([
                    iced_events,
                    Self::Subscription::run_with_id(
                        "live_apps",
                        stream::channel(1, |mut msg_sender| async move {
                            let app_icons_stream = GetLiveAppWithIconsQuery
                                .instrumented_execute(db.clone())
                                .await;
                            let app_icons_updates_stream =
                                GetLiveAppIconUpdates.instrumented_execute(db.clone()).await;

                            let mut stream = match app_icons_stream
                                .and_then(|x| app_icons_updates_stream.map(|y| (x, y)))
                            {
                                Ok((app_icons, app_icons_updates)) => {
                                    app_icons.merge(app_icons_updates)
                                }
                                Err(err) => {
                                    tracing::error!("{err}");

                                    if let Err(err) = msg_sender
                                        .send(Self::Msg::Exit {
                                            app_search_stop_sender: stop_sender,
                                        })
                                        .await
                                    {
                                        tracing::error!(
                                            "Failed to send exit message from live app table subscription: {err}"
                                        );
                                    }

                                    return;
                                }
                            };

                            while let Some(notification) = stream.next().await {
                                let notification = match notification {
                                    Ok(notification) => notification,
                                    Err(err) => {
                                        tracing::error!(
                                            "Failed to get notification from apps live table: {err}"
                                        );

                                        if let Err(err) = msg_sender
                                            .send(Self::Msg::Exit {
                                                app_search_stop_sender: stop_sender.clone(),
                                            })
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to send exit message from live app table subscription: {err}"
                                            );
                                        }

                                        return;
                                    }
                                };

                                match notification.action {
                                    DBAction::Create | DBAction::Update => {
                                        if let Err(err) = msg_sender
                                            .send(Self::Msg::AddApp(notification.data))
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to send add app from live app table subscription: {err}"
                                            );

                                            if let Err(err) = msg_sender
                                                .send(Self::Msg::Exit {
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
                                    _ => unreachable!(),
                                }
                            }
                        }),
                    ),
                ])
            }
            None => iced_events,
        }
    }

    fn title(&self) -> String {
        "Leaper Launcher".into()
    }

    fn theme(&self) -> LeaperModeTheme {
        self.config.theme.clone()
    }
}

impl LeaperLauncher {
    pub const SEARCH_ID: &'static str = "app_search_input";
    const LIST_ID: &'static str = "list";

    fn search(&self) -> <Self as LeaperMode>::Element<'_> {
        center(
            text_input("Search for an app...", &self.search)
                .id(text_input::Id::new(Self::SEARCH_ID))
                .on_input_maybe((!self.apps.is_empty()).then_some(LeaperLauncherMsg::SearchInput))
                .on_submit(LeaperLauncherMsg::RunSelectedApp)
                .size(25)
                .padding(10)
                .style(Self::text_input_style),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .padding(10)
        .into()
    }

    fn text_input_style(theme: &LeaperModeTheme, status: text_input::Status) -> text_input::Style {
        let mut style = iced::widget::text_input::default(theme, status);
        style.border = style.border.rounded(10);

        style
    }

    fn list(&self) -> <Self as LeaperMode>::Element<'_> {
        let (items, filtered) = match self.search.is_empty() {
            true => (&self.apps, false),
            false => (&self.filtered, true),
        };

        let scrllbl = || {
            scrollable(
                column(items.iter().enumerate().map(|(ind, app)| {
                    Self::app_entry(app, ind, self.selected, self.xpm_handles.clone())
                }))
                .align_x(Horizontal::Center),
            )
            .id(scrollable::Id::new(Self::LIST_ID))
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(5)
            .style(Self::scrollable_style)
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

    fn scrollable_style(theme: &LeaperModeTheme, status: scrollable::Status) -> scrollable::Style {
        let mut style = iced::widget::scrollable::default(theme, status);
        style.container = iced::widget::container::rounded_box(theme);

        style
    }

    const APP_ENTRY_HEIGHT: f32 = 50.0;
    const APP_ENTRY_PADDING: [f32; 2] = [10.0, 5.0];
    const APP_ENTRY_SPACING: f32 = 10.0;
    const APP_ENTRY_IMAGE_SIZE: f32 = Self::APP_ENTRY_HEIGHT - Self::APP_ENTRY_PADDING[1] * 2.0;
    const APP_ENTRY_TEXT_HEIGHT: f32 = Self::APP_ENTRY_IMAGE_SIZE * 0.5;

    fn app_entry<'a>(
        app: &'a AppWithIcon,
        ind: usize,
        selected: usize,
        xpm_handles: Arc<Mutex<DashMap<PathBuf, image::Handle>>>,
    ) -> <Self as LeaperMode>::Element<'a> {
        let r = match &app.icon {
            Some( icon) => match icon.svg {
                true => row![
                    svg(&icon.path)
                        .width(Self::APP_ENTRY_IMAGE_SIZE)
                        .height(Self::APP_ENTRY_IMAGE_SIZE),
                ],
                false => match icon.xpm {
                    true => {
                        let xpm_handles = xpm_handles.lock().expect("Should be fine");

                        let handle = match xpm_handles.contains_key(&icon.path) {
                            true => xpm_handles.get(&icon.path),
                            false => {
                                let img = std::fs::read_to_string(&icon.path).ok().and_then(|s| {
                                    let start = s.find('"').unwrap_or_default();
                                    let end = s.rfind('"').unwrap_or_else(|| match s.is_empty() {
                                        true => 0,
                                        false => s.len() - 1,
                                    });

                                    let lines = &s[start..=end]
                                        .lines()
                                        .map(|line| line.trim_end_matches(',').trim_matches('"'))
                                        .collect_vec();

                                    ez_pixmap::RgbaImage::from(lines)
                                        .inspect_err(|err| {
                                            tracing::error!(
                                                "Failed to parse pixmap at {:?}: {err}\n\nLines:\n{}",
                                                icon.path,
                                                lines.join("\n")
                                            )
                                        })
                                        .ok()
                                });

                                let img_handle = img.map(|img| {
                                    image::Handle::from_rgba(
                                        img.width(),
                                        img.height(),
                                        img.data().to_vec(),
                                    )
                                });

                                if let Some(handle) = img_handle {
                                    xpm_handles.insert(icon.path.clone(), handle);
                                }

                                xpm_handles.get(&icon.path)
                            }
                        };

                        match handle {
                            Some(handle) => row![
                                image(handle.clone())
                                    .width(Self::APP_ENTRY_IMAGE_SIZE)
                                    .height(Self::APP_ENTRY_IMAGE_SIZE)
                            ],
                            None => row![
                                text(icon_to_string(Nerd::Error))
                                    .font(NERD_FONT)
                                    .align_x(Horizontal::Center)
                                    .width(Self::APP_ENTRY_IMAGE_SIZE)
                                    .height(Self::APP_ENTRY_IMAGE_SIZE)
                                    .size(Self::APP_ENTRY_TEXT_HEIGHT)
                            ],
                        }
                    }
                    false => row![
                        image(&icon.path)
                            .width(Self::APP_ENTRY_IMAGE_SIZE)
                            .height(Self::APP_ENTRY_IMAGE_SIZE),
                    ],
                },
            },
            None => row![
                text(icon_to_string(Nerd::Question))
                    .font(NERD_FONT)
                    .align_x(Horizontal::Center)
                    .width(Self::APP_ENTRY_IMAGE_SIZE)
                    .height(Self::APP_ENTRY_IMAGE_SIZE)
                    .size(Self::APP_ENTRY_TEXT_HEIGHT)
            ],
        }
        .push(text(&app.name).size(Self::APP_ENTRY_TEXT_HEIGHT))
        .height(Length::Fill)
        .width(Length::Fill)
        .spacing(Self::APP_ENTRY_SPACING)
        .padding(Self::APP_ENTRY_PADDING)
        .align_y(Vertical::Center);

        button(r)
            .on_press(LeaperLauncherMsg::RunApp(ind))
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

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum LeaperLauncherMsg {
    Exit {
        #[debug(skip)]
        app_search_stop_sender: Option<tokio_mpmc::Sender<()>>,
    },

    InitDB(DBResult<DB>),

    InitApps,
    InitedApps(InitAppsIconsResult),
    LoadApps(AppsFinder),
    LoadedApps(LoadAppsIconsResult),

    AddApp(AppWithIcon),

    SearchInput(String),

    SelectUp,
    SelectDown,

    RunSelectedApp,
    RunApp(usize),
    ScrollToSelected,

    IcedEvent(Event),

    Result(LeaperLauncherResult<()>),
}

#[lerror]
#[lerr(prefix = "[leaper-launcher]", result_name = LeaperLauncherResult)]
pub enum LeaperLauncherError {
    #[lerr(str = "Path {0:?} doesn't have a file name...")]
    NoFileName(PathBuf),

    #[lerr(str = "Interrupted by parent")]
    InterruptedByParent,
    #[lerr(str = "Lost connection to the parent")]
    LostConnectionToParent,

    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),

    #[lerr(str = "[iced_layershell] {0}")]
    LayerShell(#[lerr(from, wrap = Arc)] iced_layershell::Error),

    #[lerr(str = "[tokio::task::join] {0}")]
    TokioJoin(#[lerr(from, wrap = Arc)] tokio::task::JoinError),
    #[lerr(str = "[tokio::sync::mpsc::send<PathBuf>] {0}")]
    TokioMpscSendPathBuf(#[lerr(from)] tokio::sync::mpsc::error::SendError<PathBuf>),
    #[lerr(str = "[tokio::mpmc::channel] {0}")]
    TokioMPMCChannel(#[lerr(from, wrap = Arc)] tokio_mpmc::ChannelError),

    #[lerr(str = "[image] {0}")]
    Image(#[lerr(from, wrap = Arc)] ::image::ImageError),

    #[lerr(str = "{0}")]
    Config(#[lerr(from)] LeaperAppModeConfigError),
    #[lerr(str = "{0}")]
    DB(#[lerr(from, wrap = Arc)] db::DBError),

    #[lerr(str = "[dynamic] {0}")]
    Dynamic(String),
}
