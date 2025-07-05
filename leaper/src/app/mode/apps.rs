pub mod search;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use dashmap::DashMap;
use iced::{
    Length,
    advanced::widget::{Id, operate, operation::scrollable::scroll_to},
    alignment::{Horizontal, Vertical},
    widget::{
        button, center, column, horizontal_rule, image, row, scrollable, svg, text, text_input,
    },
};
use iced_aw::Spinner;
use iced_fonts::{NERD_FONT, Nerd, nerd::icon_to_string};
use itertools::Itertools;
use tracing::Instrument;

use crate::{
    LeaperResult,
    app::{
        AppTask,
        mode::{
            AppModeElement, AppModeMsg, AppModeTask,
            apps::search::{AppsFinder, AppsResult},
        },
        style::{app_scrollable_style, app_text_input_style},
    },
    db::{DB, apps::AppWithIcon},
};

type AppsIcons = Vec<AppWithIcon>;

type InitAppsIconsResult = LeaperResult<AppsIcons>;
type LoadAppsIconsResult = AppsResult<()>;

#[derive(Default)]
pub struct Apps {
    apps: AppsIcons,
    filtered: AppsIcons,

    search: String,
    matcher: nucleo::Matcher,
    selected: usize,

    xpm_handles: Arc<Mutex<DashMap<PathBuf, image::Handle>>>,

    pub stop_search_sender: Option<tokio_mpmc::Sender<()>>,
}

impl Apps {
    pub fn update(&mut self, msg: AppsMsg) -> AppModeTask {
        match msg {
            AppsMsg::InitApps(db) => {
                let (apps_finder, sender) = AppsFinder::new();

                self.stop_search_sender = Some(sender);

                let load_db = db.clone();

                return AppModeTask::batch([
                    AppModeTask::perform(
                        {
                            let db = db.clone();
                            let span = tracing::trace_span!("get_cached_list");

                            async move {
                                Ok(db
                                    .query(
                                        "
                                        SELECT * FROM apps
                                            ORDER BY name ASC
                                            FETCH icon
                                        ",
                                    )
                                    .await?
                                    .take(0)?)
                            }
                            .instrument(span)
                        },
                        AppsMsg::InitedApps,
                    )
                    .map(Into::into),
                    AppModeTask::done(AppsMsg::LoadApps(load_db, apps_finder).into()),
                ]);
            }
            AppsMsg::InitedApps(apps) => match apps {
                Ok(apps) => {
                    self.apps = apps;

                    tracing::trace!(
                        "Initialized apps list from cache [{} apps]",
                        self.apps.len()
                    );
                }
                Err(err) => {
                    tracing::error!("Failed to initialize app list from cache: {err}");

                    return AppModeTask::done(AppModeMsg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
            },

            AppsMsg::LoadApps(db, apps_finder) => {
                return AppTask::perform(apps_finder.search(db.clone()), AppsMsg::LoadedApps)
                    .map(Into::into);
            }
            AppsMsg::LoadedApps(apps) => match apps {
                Ok(_) => {
                    tracing::trace!("AppsFinder succeded!");
                }
                Err(err) => {
                    tracing::error!("AppsFinder errored out: {err}");

                    return AppModeTask::done(AppModeMsg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
            },

            AppsMsg::AddApp(app_with_icon) => {
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

            AppsMsg::SearchInput(new_search) => {
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
            }
            AppsMsg::SelectUp => {
                self.selected = match self.apps.is_empty() {
                    true => 0,
                    false => match self.selected {
                        0 => self.apps.len() - 1,
                        x => x - 1,
                    },
                };

                return AppTask::done(AppsMsg::ScrollToSelected).map(Into::into);
            }
            AppsMsg::SelectDown => {
                self.selected = match self.apps.is_empty() {
                    true => 0,
                    false => match self.selected >= self.apps.len() - 1 {
                        true => 0,
                        false => self.selected + 1,
                    },
                };

                return AppTask::done(AppsMsg::ScrollToSelected).map(Into::into);
            }
            AppsMsg::RunSelectedApp => match self.apps.is_empty() {
                true => {}
                false => return AppTask::done(AppsMsg::RunApp(self.selected)).map(Into::into),
            },
            AppsMsg::RunApp(ind) => match {
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

                    return AppModeTask::done(AppModeMsg::Exit {
                        app_search_stop_sender: self.stop_search_sender.clone(),
                    });
                }
                None => tracing::warn!("Logic error!"),
            },
            AppsMsg::ScrollToSelected => match self.apps.is_empty() {
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
        }

        AppTask::none()
    }

    pub fn view(&self) -> AppModeElement<'_> {
        column![self.search(), horizontal_rule(2), self.list()]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .spacing(10)
            .into()
    }

    pub const SEARCH_ID: &'static str = "app_search_input";
    const LIST_ID: &'static str = "list";

    fn search(&self) -> AppModeElement<'_> {
        center(
            text_input("Search for an app...", &self.search)
                .id(text_input::Id::new(Self::SEARCH_ID))
                .on_input_maybe(
                    (!self.apps.is_empty()).then_some(|s| AppsMsg::SearchInput(s).into()),
                )
                .on_submit(AppsMsg::RunSelectedApp.into())
                .size(25)
                .padding(10)
                .style(app_text_input_style),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .padding(10)
        .into()
    }

    fn list(&self) -> AppModeElement<'_> {
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
            .style(app_scrollable_style)
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
        app: &'a AppWithIcon,
        ind: usize,
        selected: usize,
        xpm_handles: Arc<Mutex<DashMap<PathBuf, image::Handle>>>,
    ) -> AppModeElement<'a> {
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
            .on_press(AppsMsg::RunApp(ind).into())
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

#[derive(Debug, Clone)]
pub enum AppsMsg {
    InitApps(Arc<DB>),
    InitedApps(InitAppsIconsResult),
    LoadApps(Arc<DB>, AppsFinder),
    LoadedApps(LoadAppsIconsResult),

    AddApp(AppWithIcon),

    SearchInput(String),

    SelectUp,
    SelectDown,

    RunSelectedApp,
    RunApp(usize),
    ScrollToSelected,
}
