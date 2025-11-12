use std::{sync::Arc, time::Duration};

use directories::ProjectDirs;
use iced::{
    Length,
    alignment::{Horizontal, Vertical},
    keyboard,
    widget::{button, center, column, container, row, text, text_input},
};
use iced_aw::Spinner;
use iced_fonts::{NERD_FONT, NERD_FONT_BYTES, Nerd, REQUIRED_FONT_BYTES, nerd::icon_to_string};
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

    auth_in_progress: bool,
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
            .font(REQUIRED_FONT_BYTES)
            .font(NERD_FONT_BYTES)
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

            auth_in_progress: false,
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
                row![
                    text_input("Enter you password...", &self.password)
                        .width(Length::Fill)
                        .size(20)
                        .padding(10.0)
                        .on_input_maybe(
                            (!self.auth_in_progress).then_some(LeaperLockMsg::EnterPassword)
                        )
                        .on_submit_maybe(
                            (!self.auth_in_progress).then_some(LeaperLockMsg::ConfirmPassword)
                        )
                        .secure(true)
                        .style(style::text_input),
                    button(
                        text(icon_to_string(Nerd::TriangleRight))
                            .font(NERD_FONT)
                            .size(25.0)
                            .align_x(Horizontal::Center)
                            .align_y(Vertical::Center)
                    )
                    .width(40.0)
                    .height(40.0)
                    .style(style::grid_button)
                    .on_press_maybe(
                        (!self.auth_in_progress).then_some(LeaperLockMsg::ConfirmPassword)
                    )
                ]
                .push_maybe(
                    self.auth_in_progress
                        .then(|| Spinner::new().width(20).height(20))
                )
                .width(600.0)
                .spacing(15)
                .align_y(Vertical::Center),
            ]
            .align_x(Horizontal::Center)
            .spacing(50),
        )
        .into()
    }

    fn update(&mut self, msg: Self::Msg) -> Self::Task {
        match msg {
            LeaperLockMsg::SecondTick => {}
            LeaperLockMsg::FailedLock(err) => {
                self.auth_in_progress = false;
                tracing::error!("{err}");
            }

            LeaperLockMsg::EnterPassword(new_pass) => self.password = new_pass,
            LeaperLockMsg::ConfirmPassword => {
                let auth_adapter = LeaperAuthAdapter {
                    user_name: self.user_name.clone(),
                    password: self.password.clone(),
                };
                let user_name = self.user_name.clone();

                self.auth_in_progress = true;

                return Self::Task::perform(
                    async move {
                        let mut auth =
                            nonstick::TransactionBuilder::new_with_service("leaper-lock")
                                .username(user_name)
                                .build(auth_adapter.into_conversation())?;

                        auth.authenticate(AuthnFlags::empty())?;
                        auth.account_management(AuthnFlags::empty())?;

                        LeaperLockResult::Ok(())
                    },
                    |res| match res {
                        Ok(_) => LeaperLockMsg::UnLock,
                        Err(err) => LeaperLockMsg::FailedLock(err.to_string()),
                    },
                );
            }

            LeaperLockMsg::IcedEvent(ev) => {
                if !self.auth_in_progress
                    && let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Enter),
                        ..
                    }) = ev
                {
                    return Self::Task::done(Self::Msg::ConfirmPassword);
                }
            }

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
    FailedLock(String),

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
