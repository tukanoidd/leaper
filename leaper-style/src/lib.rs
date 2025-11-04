use iced::{Color, widget};

use mode::LeaperModeTheme;

pub fn text_input(
    theme: &LeaperModeTheme,
    status: widget::text_input::Status,
) -> widget::text_input::Style {
    let mut style = widget::text_input::default(theme, status);
    style.border = style.border.rounded(10);

    style
}

pub fn scrollable(
    theme: &LeaperModeTheme,
    status: widget::scrollable::Status,
) -> widget::scrollable::Style {
    let mut style = widget::scrollable::default(theme, status);

    style.container = widget::container::rounded_box(theme).background(Color::TRANSPARENT);
    style.vertical_rail.border = style.vertical_rail.border.rounded(10.0);
    style.vertical_rail.scroller.border = style.vertical_rail.scroller.border.rounded(10.0);
    style.horizontal_rail.border = style.horizontal_rail.border.rounded(10.0);
    style.horizontal_rail.scroller.border = style.horizontal_rail.scroller.border.rounded(10.0);

    style
}

pub fn list_button(
    theme: &LeaperModeTheme,
    status: widget::button::Status,
    selected: bool,
) -> widget::button::Style {
    let status = match selected {
        true => widget::button::Status::Hovered,
        false => status,
    };

    let palette = theme.extended_palette();

    let mut style = widget::button::secondary(theme, status);

    style.background = style.background.map(|b| b.scale_alpha(0.75));
    style.border = style
        .border
        .color(palette.background.strong.color)
        .rounded(10.0);

    style
}
