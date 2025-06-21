macro_rules! app_types {
    (@widgets [
        $($name:ident [$mod_name:ident] {
            $($ty:ident),+
            $(,)?
        }),+
        $(,)?
    ]) => {
        pastey::paste! {
            $(
                #[allow(dead_code)]
                pub type [< App $name >]<
                    'a,
                    Msg = $crate::app::AppMsg
                > = iced::widget::$name<
                    'a,
                    Msg,
                    $crate::app::AppTheme,
                    $crate::app::AppRenderer
                >;

                $(pub type [< App $name $ty >] = iced::widget::$mod_name::$ty;)+
            )+
        }
    };
}

app_types!(@widgets [
    TextInput [text_input] {
        Status,
        Style
    },
    Scrollable [scrollable] {
        Status,
        Style
    }
]);
