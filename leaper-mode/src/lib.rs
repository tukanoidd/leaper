#![feature(associated_type_defaults)]

pub mod config;

use directories::ProjectDirs;

use config::LeaperModeConfig;

pub type LeaperModeTheme = iced::Theme;

pub trait LeaperMode {
    type RunError;

    type Task = iced::Task<Self::Msg>;
    type Subscription = iced::Subscription<Self::Msg>;

    type Renderer = iced::Renderer;
    type Element<'a>
        = iced::Element<'a, Self::Msg, LeaperModeTheme, Self::Renderer>
    where
        Self: 'a;

    type Msg: std::fmt::Debug + Clone;

    fn run() -> Result<(), Self::RunError>;

    fn init(project_dirs: ProjectDirs, config: LeaperModeConfig) -> (Self, Self::Task)
    where
        Self: Sized;

    fn view(&self) -> Self::Element<'_>;
    fn update(&mut self, msg: Self::Msg) -> Self::Task;
    fn subscription(&self) -> Self::Subscription;

    fn title(&self) -> String;
    fn theme(&self) -> LeaperModeTheme;

    fn project_dirs() -> ProjectDirs {
        ProjectDirs::from("com", "tukanoid", "leaper").unwrap()
    }
}
