use csscolorparser::Color;
use serde::{Deserialize, Serialize};

pub type AppThemeCustom = iced::theme::Custom;
pub type AppThemePalette = iced::theme::Palette;
pub type AppColor = iced::Color;

macro_rules! theme {
    ($($([$default:ident])? $name:ident),+ $(,)?) => {
        #[derive(Debug, Clone, smart_default::SmartDefault, serde::Serialize, serde::Deserialize)]
        pub enum LeaperTheme {
            $($(#[$default])? $name,)+
            Custom(CustomTheme)
        }

        impl From<LeaperTheme> for $crate::state::app::AppTheme {
            fn from(value: LeaperTheme) -> Self {
                match value {
                    $(LeaperTheme::$name => Self::$name,)+
                    LeaperTheme::Custom(custom) => Self::Custom(std::sync::Arc::new(custom.into()))
                }
            }
        }
    };
}

theme![
    Light,
    Dark,
    Dracula,
    Nord,
    SolarizedLight,
    SolarizedDark,
    GruvboxLight,
    GruvboxDark,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
    [default]
    TokyoNight,
    TokyoNightStorm,
    TokyoNightLight,
    KanagawaWave,
    KanagawaDragon,
    KanagawaLotus,
    Moonfly,
    Nightfly,
    Oxocarbon,
    Ferra,
];

impl From<CustomTheme> for LeaperTheme {
    fn from(value: CustomTheme) -> Self {
        Self::Custom(value)
    }
}

#[derive(Debug, Clone, bon::Builder, Serialize, Deserialize)]
pub struct CustomTheme {
    pub name: String,
    pub palette: ThemePalette,
}

impl From<CustomTheme> for AppThemeCustom {
    fn from(CustomTheme { name, palette }: CustomTheme) -> Self {
        Self::new(name, palette.into())
    }
}

macro_rules! palette {
    ($($name:ident),+ $(,)?) => {
        #[derive(Debug, Clone, bon::Builder, Serialize, Deserialize)]
        pub struct ThemePalette {
            $($name: csscolorparser::Color),+
        }

        impl From<ThemePalette> for AppThemePalette {
            fn from(ThemePalette {$($name),+}: ThemePalette) -> Self {
                Self {
                    $($name: $name.to_app_color()),+
                }
            }
        }
    };
}

palette![background, text, primary, success, danger];

pub trait ColorExt {
    fn to_app_color(self) -> AppColor;
}

impl ColorExt for Color {
    fn to_app_color(self) -> AppColor {
        let Self { r, g, b, a } = self;
        iced::Color::from_rgba(r, g, b, a)
    }
}
