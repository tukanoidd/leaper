use iced::widget;
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
    style.container = widget::container::rounded_box(theme);

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

    widget::button::secondary(theme, status)
}
