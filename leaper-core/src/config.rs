pub mod modules;
pub mod theme;

use std::{path::Path, sync::Arc};

use directories::ProjectDirs;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::{
    config::{modules::builtins::Builtins, theme::LeaperTheme},
    err_from_wrapped,
};

#[derive(Debug, Clone, SmartDefault, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub builtins: Builtins,

    #[default = "rio"]
    pub terminal: String,
    pub theme: LeaperTheme,
}

impl Config {
    pub async fn open(dirs: &ProjectDirs) -> ConfigResult<Self> {
        let config_dir = dirs.config_local_dir();

        if !config_dir.exists() {
            tracing::warn!("Config directory {config_dir:?} doesn't exist, creating...");
            tokio::fs::create_dir_all(&config_dir).await?;
        }

        let mut config = match ["toml", "ron", "json"]
            .into_iter()
            .map(|ext| config_dir.join(format!("config.{ext}")))
            .find(|p| p.exists())
        {
            Some(config_path) => {
                let str = tokio::fs::read_to_string(&config_path).await?;

                match config_path.extension().unwrap().to_str().unwrap() {
                    "toml" => toml::from_str::<Self>(&str)?,
                    "ron" => ron::from_str::<Self>(&str)?,
                    "json" => ron::from_str::<Self>(&str)?,
                    _ => unreachable!(),
                }
            }
            None => {
                tracing::warn!(
                    "Couldn't find a config file (config.{{toml/ron/json}}) at {config_dir:?}, creating a default one (config.toml)..."
                );

                let config = Config::default();
                let config_str = toml::to_string_pretty(&config)?;

                let path = config_dir.join("config.toml");
                let mut file = tokio::fs::File::create(&path).await?;
                file.write_all(config_str.as_bytes()).await?;

                config
            }
        };

        Self::check_dotenv(config_dir)?;
        Self::check_term(&mut config.terminal)?;

        Ok(config)
    }

    fn check_dotenv(config_dir: &Path) -> ConfigResult<()> {
        let env_file = config_dir.join(".env");

        if env_file.exists() {
            dotenvy::from_path(env_file)?;
        }

        Ok(())
    }

    fn check_term(term: &mut String) -> ConfigResult<()> {
        const KNOWN_TERMINALS: &[&str] = &[
            "rio",
            "Eterm",
            "alacritty",
            "aterm",
            "foot",
            "gnome-terminal",
            "guake",
            "hyper",
            "kitty",
            "konsole",
            "lilyterm",
            "lxterminal",
            "mate-terminal",
            "qterminal",
            "roxterm",
            "rxvt",
            "st",
            "terminator",
            "terminix",
            "terminology",
            "termit",
            "termite",
            "tilda",
            "tilix",
            "urxvt",
            "uxterm",
            "wezterm",
            "x-terminal-emulator",
            "xfce4-terminal",
            "xterm",
            "ghostty",
        ];

        if term.is_empty()
            && let Some(env_term) = ["TERM", "TERMINAL"]
                .into_iter()
                .find_map(|env_var| std::env::var(env_var).ok())
        {
            *term = env_term;
        }

        if !term.is_empty() && which::which(&term).is_ok() {
            return Ok(());
        }

        if let Some(known_term) = KNOWN_TERMINALS
            .iter()
            .find(|term| which::which(term).is_ok())
        {
            *term = (*known_term).into();
            return Ok(());
        }

        Err(ConfigError::Term)
    }
}

pub type ConfigResult<T> = Result<T, ConfigError>;

#[derive(Debug, Clone, Error, Diagnostic)]
pub enum ConfigError {
    #[error("Couldn't determine terminal, try to set it to one on your system in the config")]
    Term,

    #[error("[config] [std::io] {0}")]
    IO(Arc<std::io::Error>),

    #[error("[config] [toml::ser] {0}")]
    TOMLSerialization(#[from] toml::ser::Error),
    #[error("[config] [toml::de] {0}")]
    TOMLDeserialization(#[from] toml::de::Error),

    #[error("[config] [ron::de] {0}")]
    RONDeserialization(#[from] ron::de::SpannedError),
    #[error("[config] [serde_json] {0}")]
    JSON(Arc<serde_json::Error>),

    #[error("[config] [dotenvy] {0}")]
    DotEnv(Arc<dotenvy::Error>),
}

err_from_wrapped!(ConfigError {
    IO: std::io::Error[Arc],
    JSON: serde_json::Error[Arc],
    DotEnv: dotenvy::Error[Arc],
});
