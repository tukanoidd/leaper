use std::{sync::Arc, time::Duration};

use directories::ProjectDirs;
use iced::{
    Length,
    alignment::Horizontal,
    keyboard,
    widget::{center, column, container, text, text_input},
};
use iced_sessionlock::to_session_message;

use macros::lerror;
use mode::{
    LeaperModeMultiWindow,
    config::{LeaperAppModeConfigError, LeaperModeConfig},
};
use nonstick::{AuthnFlags, ConversationAdapter, Transaction};

pub struct LeaperLock {
    config: LeaperModeConfig,

    user_name: String,
    password: String,
}

impl LeaperModeMultiWindow for LeaperLock {
    type RunError = LeaperLockError;
    type InitArgs = String;
    type Msg = LeaperLockMsg;

    fn run() -> Result<(), Self::RunError> {
        let project_dirs =
            ProjectDirs::from("com", "tukanoid", "leaper").ok_or(Self::RunError::NoProjectDirs)?;
        let config = LeaperModeConfig::open(&project_dirs)?;

        let uid = nix::unistd::Uid::current();
        let user = nix::unistd::User::from_uid(uid)?.ok_or(LeaperLockError::NoUserFound)?;

        iced_sessionlock::build_pattern::application(Self::update, Self::view)
            .subscription(Self::subscription)
            .theme(Self::theme)
            .run_with(|| Self::init(project_dirs, config, user.name))?;

        Ok(())
    }

    fn init(
        _project_dirs: ProjectDirs,
        config: LeaperModeConfig,
        user_name: Self::InitArgs,
    ) -> (Self, Self::Task)
    where
        Self: Sized,
    {
        let lock = Self {
            config,

            user_name,
            password: String::new(),
        };
        let task = Self::Task::none();

        (lock, task)
    }

    fn view(&self, _id: iced::window::Id) -> Self::Element<'_> {
        let date_time = chrono::Local::now();
        let time_str = date_time.format("%H:%M:%S").to_string();
        let date_str = date_time.format("%A - %d/%b/%Y").to_string();

        center(
            column![
                center(
                    column![text(time_str).size(60), text(date_str).size(40)]
                        .align_x(Horizontal::Center)
                        .spacing(10)
                )
                .padding(15)
                .width(Length::Shrink)
                .height(Length::Shrink)
                .style(|theme| {
                    let mut style = container::bordered_box(theme);
                    style.background = None;
                    style.border = style.border.rounded(10.0).width(2);

                    style
                }),
                text_input("Enter you password...", &self.password)
                    .width(600.0)
                    .size(20)
                    .padding(10.0)
                    .on_input(LeaperLockMsg::EnterPassword)
                    .on_submit(LeaperLockMsg::ConfirmPassword)
                    .secure(true)
                    .style(style::text_input),
            ]
            .align_x(Horizontal::Center)
            .spacing(50),
        )
        .into()
    }

    fn update(&mut self, msg: Self::Msg) -> Self::Task {
        match msg {
            LeaperLockMsg::SecondTick => {}

            LeaperLockMsg::EnterPassword(new_pass) => self.password = new_pass,
            LeaperLockMsg::ConfirmPassword => {
                if let Err(err) = (|| {
                    let mut auth = nonstick::TransactionBuilder::new_with_service("leaper-lock")
                        .username(&self.user_name)
                        .build(
                            LeaperAuthAdapter {
                                user_name: self.user_name.clone(),
                                password: self.password.clone(),
                            }
                            .into_conversation(),
                        )?;

                    auth.authenticate(AuthnFlags::empty())?;
                    auth.account_management(AuthnFlags::empty())?;

                    LeaperLockResult::Ok(())
                })() {
                    tracing::error!("{err}");
                }

                return Self::Task::done(LeaperLockMsg::UnLock);
            }

            LeaperLockMsg::IcedEvent(ev) => match ev {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(keyboard::key::Named::Enter),
                    ..
                }) => return Self::Task::done(Self::Msg::ConfirmPassword),
                _ => {}
            },

            LeaperLockMsg::UnLock => return Self::Task::done(msg),
        }

        Self::Task::none()
    }

    fn subscription(&self) -> Self::Subscription {
        Self::Subscription::batch([
            iced::event::listen().map(LeaperLockMsg::IcedEvent),
            Self::Subscription::run_with_id(
                "second-timer",
                iced::stream::channel(1, move |mut sender| async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        if let Err(err) = sender.start_send(LeaperLockMsg::SecondTick) {
                            tracing::error!(
                                "Failed to send SecondTick message to main thread: {err}"
                            );
                        }
                    }
                }),
            ),
        ])
    }

    fn title(&self) -> String {
        "Leaper Lock".into()
    }

    fn theme(&self) -> mode::LeaperModeTheme {
        self.config.theme.clone()
    }
}

pub struct LeaperAuthAdapter {
    user_name: String,
    password: String,
}

impl nonstick::ConversationAdapter for LeaperAuthAdapter {
    fn prompt(
        &self,
        _request: impl AsRef<std::ffi::OsStr>,
    ) -> nonstick::Result<std::ffi::OsString> {
        Ok((&self.user_name).into())
    }

    fn masked_prompt(
        &self,
        _request: impl AsRef<std::ffi::OsStr>,
    ) -> nonstick::Result<std::ffi::OsString> {
        Ok((&self.password).into())
    }

    fn error_msg(&self, message: impl AsRef<std::ffi::OsStr>) {
        tracing::error!("[leaper-lock-auth] {}", message.as_ref().to_string_lossy())
    }

    fn info_msg(&self, message: impl AsRef<std::ffi::OsStr>) {
        tracing::info!("[leaper-lock-auth] {}", message.as_ref().to_string_lossy())
    }
}

#[to_session_message]
#[derive(Debug, Clone)]
pub enum LeaperLockMsg {
    SecondTick,

    EnterPassword(String),
    ConfirmPassword,

    IcedEvent(iced::Event),
}

#[lerror]
#[lerr(prefix = "[leaper-lock]", result_name = LeaperLockResult)]
pub enum LeaperLockError {
    #[lerr(str = "[iced_sessionlock] {0}")]
    SessionLock(#[lerr(from, wrap = Arc)] iced_sessionlock::Error),
    #[lerr(str = "[nonstick] {0}")]
    Nonstick(#[lerr(from)] nonstick::ErrorCode),
    #[lerr(str = "[nix] {0}")]
    Nix(#[lerr(from)] nix::Error),

    #[lerr(str = "{0}")]
    Config(#[lerr(from)] LeaperAppModeConfigError),

    #[lerr(str = "No ProjectDirs!")]
    NoProjectDirs,
    #[lerr(str = "No User found!")]
    NoUserFound,
}
