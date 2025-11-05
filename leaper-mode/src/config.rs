use std::{io::Write, sync::Arc};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

use macros::lerror;

use crate::LeaperModeTheme;

#[derive(SmartDefault, Serialize, Deserialize)]
#[serde(default)]
pub struct LeaperModeConfig {
    #[serde(serialize_with = "ser_theme", deserialize_with = "de_theme")]
    #[default(LeaperModeTheme::TokyoNight)]
    pub theme: LeaperModeTheme,
    pub power: PowerConfig,
    #[default = 8000]
    pub db_port: u16,
}

impl LeaperModeConfig {
    pub fn open(dirs: &ProjectDirs) -> LeaperModeConfigResult<Self> {
        let config_dir = dirs.config_local_dir();

        if !config_dir.exists() {
            std::fs::create_dir_all(config_dir)?;
        }

        let config_file_path = config_dir.join("config.toml");

        let res = match config_file_path.exists() {
            true => toml::from_str(&std::fs::read_to_string(config_file_path)?)?,
            false => {
                let config = Default::default();

                let mut file = std::fs::File::create(config_file_path)?;
                file.write_all(toml::to_string_pretty(&config)?.as_bytes())?;

                config
            }
        };

        Ok(res)
    }
}
macro_rules! serde_theme {
    (
        $ty:ty => [
            $($name:ident),+
            $(,)?
        ]
    ) => {
        fn ser_theme<S>(val: &$ty, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            use heck::ToKebabCase;

            let str = match val {
                $(<$ty>::$name => stringify!($name).to_kebab_case(),)+
                _ => return Err(serde::ser::Error::custom("Custom themes are not supported!"))
            };

            serializer.serialize_str(&str)
        }

        fn de_theme<'de, D>(deserializer: D) -> Result<$crate::LeaperModeTheme, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_str(ThemeVisitor)
        }

        struct ThemeVisitor;

        impl serde::de::Visitor<'_> for ThemeVisitor {
            type Value = $crate::LeaperModeTheme;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "A string name of the theme")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                use heck::ToKebabCase;

                $(
                    if v == &stringify!($name).to_kebab_case() {
                        return Ok(<$ty>::$name);
                    }
                )+

                Err(serde::de::Error::invalid_value(
                    serde::de::Unexpected::Str(v),
                    &format!(
                        "{:?}",
                        [$(stringify!($name).to_kebab_case()),+]
                    ).as_str()
                ))
            }
        }
    }
}

serde_theme!(LeaperModeTheme => [
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
    TokyoNight,
    TokyoNightStorm,
    TokyoNightLight,
    KanagawaWave,
    KanagawaDragon,
    KanagawaLotus,
    Moonfly,
    Nightfly,
    Oxocarbon,
    Ferra
]);

#[derive(SmartDefault, Serialize, Deserialize)]
pub struct PowerConfig {
    pub actions: Actions,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Actions {
    pub lock: ActionMethod,
    pub log_out: ActionMethod,
    pub hibernate: ActionMethod,
    pub reboot: ActionMethod,
    pub shutdown: ActionMethod,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum ActionMethod {
    #[default]
    Dbus,
    Cmd(Vec<String>),
}

#[lerror]
#[lerr(prefix = "[leaper_mode::config]", result_name = LeaperModeConfigResult)]
pub enum LeaperAppModeConfigError {
    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),
    #[lerr(str = "[toml::de] {0}")]
    TomlDeser(#[lerr(from)] toml::de::Error),
    #[lerr(str = "[toml::ser] {0}")]
    TomlSer(#[lerr(from)] toml::ser::Error),
}
