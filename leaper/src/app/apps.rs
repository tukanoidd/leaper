use std::sync::Arc;

use iced::{
    Length,
    advanced::widget::{Id, operate, operation::scrollable::scroll_to},
    alignment::{Horizontal, Vertical},
    widget::{
        Space, button, center, column, horizontal_rule, image, row, scrollable, svg, text,
        text_input,
    },
};
use iced_aw::Spinner;
use itertools::Itertools;
use leaper_apps::{AppEntry, AppsResult, search_apps};
use leaper_db::{DB, DBResult};

use crate::app::{
    AppElement, AppTask,
    style::{app_scrollable_style, app_text_input_style},
};

type AppsIcons = Vec<AppEntry>;

type InitAppsIconsResult = DBResult<AppsIcons>;
type LoadAppsIconsResult = AppsResult<AppsIcons>;

#[derive(Default)]
pub struct Apps {
    apps: AppsIcons,
    filtered: AppsIcons,

    search: String,
    matcher: nucleo::Matcher,
    selected: usize,
}

impl Apps {
    pub fn update(&mut self, msg: AppsMsg, db: &Option<Arc<DB>>) -> AppTask {
        match msg {
            AppsMsg::InitApps(apps) => match apps {
                Ok(apps) => {
                    self.apps = apps;
                    self.apps.sort_by_key(|a| a.name.clone());

                    tracing::trace!(
                        "Initialized apps list from cache [{} entries]",
                        self.apps.len()
                    );

                    return AppTask::perform(search_apps(db.clone().unwrap()), AppsMsg::LoadApps)
                        .map(Into::into);
                }
                Err(err) => {
                    tracing::error!("Failed to initialize app list from cache: {err}");
                    return iced::exit();
                }
            },
            AppsMsg::LoadApps(apps) => match apps {
                Ok(apps) => {
                    self.apps = apps;
                    self.selected = self.selected.clamp(0, self.apps.len() - 1);

                    tracing::trace!("Loaded a fresh list of apps [{} entries]", self.apps.len());
                }
                Err(err) => {
                    tracing::error!("Failed to load new app list: {err}. Retrying...");
                    return AppTask::perform(search_apps(db.clone().unwrap()), AppsMsg::LoadApps)
                        .map(Into::into);
                }
            },

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

                    return iced::exit();
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

    pub fn view(&self) -> AppElement<'_> {
        column![self.search(), horizontal_rule(2), self.list()]
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .spacing(10)
            .into()
    }

    pub const SEARCH_ID: &'static str = "app_search_input";
    const LIST_ID: &'static str = "list";

    fn search(&self) -> AppElement<'_> {
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
                        .map(|(ind, app)| Self::app_entry(app, ind, self.selected)),
                )
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

    fn app_entry<'a>(app: &'a AppEntry, ind: usize, selected: usize) -> AppElement<'a> {
        let r = match &app.icon {
            Some(icon) => match icon.svg {
                true => row![
                    svg(&icon.path)
                        .width(Self::APP_ENTRY_IMAGE_SIZE)
                        .height(Self::APP_ENTRY_IMAGE_SIZE),
                ],
                false => row![
                    image(&icon.path)
                        .width(Self::APP_ENTRY_IMAGE_SIZE)
                        .height(Self::APP_ENTRY_IMAGE_SIZE),
                ],
            },
            None => row![Space::new(
                Self::APP_ENTRY_IMAGE_SIZE,
                Self::APP_ENTRY_IMAGE_SIZE
            )],
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
    InitApps(InitAppsIconsResult),
    LoadApps(LoadAppsIconsResult),

    SearchInput(String),

    SelectUp,
    SelectDown,

    RunSelectedApp,
    RunApp(usize),
    ScrollToSelected,
}
