use std::sync::Arc;

use directories::ProjectDirs;
use iced::{
    Event,
    keyboard::{self, Key, key},
    widget::{center, text_input},
};
use iced_layershell::{
    build_pattern::MainSettings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::{LayerShellSettings, Settings, StartMode},
    to_layer_message,
};

use macros::lerror;
use mode::{
    LeaperMode,
    config::{LeaperAppModeConfigError, LeaperModeConfig},
};

#[derive(Default)]
pub struct LeaperRunner {
    config: LeaperModeConfig,

    input: String,
}

impl LeaperMode for LeaperRunner {
    type RunError = LeaperRunnerError;

    type Msg = LeaperRunnerMsg;

    fn run() -> Result<(), Self::RunError> {
        let project_dirs = Self::project_dirs();
        let config = LeaperModeConfig::open(&project_dirs)?;

        let Settings {
            fonts,
            default_font,
            default_text_size,
            antialiasing,
            virtual_keyboard_support,
            ..
        } = Settings::<()>::default();

        let settings = MainSettings {
            id: Some("com.tukanoid.leaper".into()),
            layer_settings: LayerShellSettings {
                anchor: Anchor::empty(),
                layer: Layer::Overlay,
                exclusive_zone: 0,
                size: Some((600, 100)),
                margin: (0, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                start_mode: StartMode::Active,
                events_transparent: false,
            },
            fonts,
            default_font,
            default_text_size,
            antialiasing,
            virtual_keyboard_support,
        };

        iced_layershell::build_pattern::application("leaper", Self::update, Self::view)
            .settings(settings)
            .theme(Self::theme)
            .subscription(Self::subscription)
            .run_with(move || Self::init(project_dirs, config))?;

        Ok(())
    }

    fn init(_project_dirs: ProjectDirs, config: LeaperModeConfig) -> (Self, Self::Task)
    where
        Self: Sized,
    {
        let runner = Self {
            config,
            ..Default::default()
        };
        let task = text_input::focus(Self::INPUT_ID);

        (runner, task)
    }

    fn view(&self) -> Self::Element<'_> {
        center(
            text_input("Input command to run...", &self.input)
                .id(Self::INPUT_ID)
                .size(30)
                .padding(10)
                .style(style::text_input)
                .on_input(Self::Msg::Input)
                .on_submit(Self::Msg::TryRun),
        )
        .padding(10)
        .into()
    }

    fn update(&mut self, msg: Self::Msg) -> Self::Task {
        match msg {
            Self::Msg::Exit => return iced::exit(),

            Self::Msg::Input(new_input) => self.input = new_input,
            Self::Msg::TryRun => {
                let split = shlex::split(&self.input);

                match split {
                    None => {
                        tracing::warn!("Failed to split {:?} into command arguments!", self.input)
                    }
                    Some(mut split) => match split.is_empty() {
                        true => tracing::warn!("Command is empty!"),
                        false => {
                            let cmd = split.remove(0);

                            match std::process::Command::new(cmd).args(split).spawn() {
                                Ok(_) => {
                                    tracing::debug!("Command spawned successfully!");
                                    return Self::Task::done(Self::Msg::Exit);
                                }
                                Err(err) => tracing::error!("Failed to run the command: {err}"),
                            }
                        }
                    },
                }
            }

            Self::Msg::IcedEvent(event) => {
                if let Event::Keyboard(event) = event
                    && let keyboard::Event::KeyPressed { key, .. } = event
                    && let Key::Named(key::Named::Escape) | Key::Character("q" | "Q") = key.as_ref()
                {
                    return Self::Task::done(Self::Msg::Exit);
                }
            }

            Self::Msg::AnchorChange(_)
            | Self::Msg::SetInputRegion(_)
            | Self::Msg::SizeChange(_)
            | Self::Msg::AnchorSizeChange(_, _)
            | Self::Msg::LayerChange(_)
            | Self::Msg::MarginChange(_)
            | Self::Msg::VirtualKeyboardPressed { .. } => {}
        }

        Self::Task::none()
    }

    fn subscription(&self) -> Self::Subscription {
        iced::event::listen().map(Self::Msg::IcedEvent)
    }

    fn title(&self) -> String {
        "Leaper Runner".into()
    }

    fn theme(&self) -> mode::LeaperModeTheme {
        self.config.theme.clone()
    }
}

impl LeaperRunner {
    pub const INPUT_ID: &'static str = "command_input";
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum LeaperRunnerMsg {
    Exit,

    Input(String),
    TryRun,

    IcedEvent(Event),
}

#[lerror]
#[lerr(prefix = "[leaper_runner]", result_name = LeaperRunnerResult)]
pub enum LeaperRunnerError {
    #[lerr(str = "[iced_layershell] {0}")]
    LayerShell(#[lerr(from, wrap = Arc)] iced_layershell::Error),

    #[lerr(str = "{0}")]
    Config(#[lerr(from)] LeaperAppModeConfigError),
}
