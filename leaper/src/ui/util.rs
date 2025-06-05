use std::path::PathBuf;

use iced::{
    Length, Pixels,
    alignment::Vertical,
    widget::{button, center, container, image, row, text, text_input},
};
use lcore::state::app::AppTheme;

use crate::ui::app::{AppElement, AppMsg};

fn main_container_centered<'a>(el: impl Into<AppElement<'a>>) -> AppElement<'a> {
    center(container(el).width(400).height(200)).into()
}

#[bon::builder]
pub fn text_container<'a>(
    txt: impl text::IntoFragment<'a>,
    style: impl Fn(&AppTheme) -> text::Style + 'a,
    size: impl Into<Pixels>,
) -> AppElement<'a> {
    main_container_centered(text(txt).size(size).style(style))
}

macro_rules! txt_containers {
    ($($which:ident: $style:ident),+ $(,)?) => {
        pastey::paste! {
            $(
                #[bon::builder]
                pub fn [< $which _container >]<'a>(
                    $which: impl text::IntoFragment<'a>,
                ) -> AppElement<'a> {
                    text_container()
                        .txt($which)
                        .style(text::$style)
                        .size(30)
                        .call()
                }
            )+
        }
    };
}

txt_containers![info: success, error: danger];

pub fn main_container<'a>(el: impl Into<AppElement<'a>>) -> AppElement<'a> {
    center(container(el).max_height(800).max_width(600)).into()
}

#[bon::builder]
pub fn filter_input<'a>(
    id: impl Into<text_input::Id>,
    placeholder: &'a str,
    value: &'a str,
    on_submit: AppMsg,
) -> AppElement<'a> {
    container(
        text_input(placeholder, value)
            .id(id)
            .size(35)
            .padding(5)
            .on_input(AppMsg::Filter)
            .on_submit(on_submit),
    )
    .padding(10)
    .into()
}

pub const SELECTOR_BUTTON_HEIGHT: f32 = 80.0;

#[bon::builder]
pub fn selector_button<'a>(
    ind: usize,
    name: impl text::IntoFragment<'a>,
    active_ind: &'a Option<usize>,
    icon: &'a Option<PathBuf>,
    on_press: Option<AppMsg>,
) -> AppElement<'a> {
    container(
        button({
            let mut row = row![].spacing(5).padding(15).align_y(Vertical::Center);

            if let Some(icon_path) = icon {
                row = row.push(image(icon_path));
            }
            row = row.push(text(name).size(20));

            center(row).width(Length::Fill).height(Length::Fill)
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |theme, mut status| {
            if let Some(active) = active_ind {
                status = match active == &ind {
                    true => button::Status::Hovered,
                    false => button::Status::Active,
                }
            }

            button::secondary(theme, status)
        })
        .on_press_maybe(on_press),
    )
    .width(Length::Fill)
    .height(Length::Fixed(SELECTOR_BUTTON_HEIGHT))
    .padding(5)
    .into()
}
