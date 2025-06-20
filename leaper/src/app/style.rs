macro_rules! app_type_style {
    (@widgets [
        $($name:ident $fn:ident ($theme:ident $(, $status:ident)?) $impl:block),+
        $(,)?
    ]) => {
        pastey::paste! {
            $(
                pub fn [< $fn _style >](
                    $theme: &$crate::app::AppTheme
                    $(, $status: $crate::app::types::[< App $name Status >])?
                ) -> $crate::app::types::[< App $name Style >] $impl
            )+
        }
    };
}

app_type_style!(@widgets [
    TextInput app_text_input (theme, status) {
        let mut style = iced::widget::text_input::default(theme, status);
        style.border = style.border.rounded(10);

        style
    },
    Scrollable app_scrollable (theme, status) {
        let mut style = iced::widget::scrollable::default(theme, status);
        style.container = iced::widget::container::rounded_box(theme);

        style
    }
]);
