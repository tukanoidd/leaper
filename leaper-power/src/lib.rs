use std::sync::Arc;

use directories::ProjectDirs;
use iced::{
    alignment::Horizontal,
    widget::{button, center, column, row, text},
};
use iced_fonts::{NERD_FONT, Nerd, nerd::icon_to_string};
use iced_layershell::{
    build_pattern::MainSettings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::{LayerShellSettings, Settings, StartMode},
    to_layer_message,
};
use logind_zbus::{manager::ManagerProxy, session::SessionProxy};
use zbus::{Connection, connection};

use macros::lerror;
use mode::{
    LeaperMode, LeaperModeTheme,
    config::{ActionMethod, LeaperAppModeConfigError, LeaperModeConfig},
};

macro_rules! logind_fns {
    (
        $(
            $root:ident => [
                $(
                    $fn:ident [$context:literal] $((
                        $($param:expr),+
                        $(,)?
                    ))?
                ),+
                $(,)?
            ]
        ),+
        $(,)?
    ) => {
        $($(
            async fn $fn(connection: Option<Connection>) -> LeaperPowerResult<()> {
                let Some(connection) = connection else {
                    return Err(LeaperPowerError::NoDBusConnection);
                };

                Ok(Self::$root(&connection)
                    .await?
                    .$fn($($($param),+)?)
                    .await?)
            }
        )+)+
    }
}

#[derive(Default)]
pub struct LeaperPower {
    config: LeaperModeConfig,
    connection: Option<Connection>,
}

impl LeaperMode for LeaperPower {
    type RunError = LeaperPowerError;

    type Msg = LeaperPowerMsg;

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
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                layer: Layer::Overlay,
                exclusive_zone: -1,
                size: None,
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

        iced_layershell::build_pattern::application(Self::title, Self::update, Self::view)
            .settings(settings)
            .theme(Self::theme)
            .subscription(Self::subscription)
            .font(iced_fonts::REQUIRED_FONT_BYTES)
            .font(iced_fonts::NERD_FONT_BYTES)
            .run_with(move || Self::init(project_dirs, config))?;

        Ok(())
    }

    fn init(_project_dirs: ProjectDirs, config: LeaperModeConfig) -> (Self, Self::Task)
    where
        Self: Sized,
    {
        let power = Self {
            config,
            connection: None,
        };
        let task = Self::Task::done(LeaperPowerMsg::ConnectZbus);

        (power, task)
    }

    fn update(&mut self, msg: LeaperPowerMsg) -> Self::Task {
        match msg {
            LeaperPowerMsg::Exit => iced::exit(),
            LeaperPowerMsg::ConnectZbus => {
                Self::Task::perform(LeaperPower::zbus_connect(), |res| {
                    LeaperPowerMsg::ZbusConnected(res)
                })
            }
            LeaperPowerMsg::ZbusConnected(connection) => match connection {
                Ok(connection) => {
                    self.connection = Some(connection);
                    Self::Task::none()
                }
                Err(e) => {
                    tracing::error!("{}", e);
                    Self::Task::done(LeaperPowerMsg::Exit)
                }
            },
            LeaperPowerMsg::Lock => Self::action_task(
                "Lock",
                self.config.power.actions.lock.clone(),
                self.connection.clone(),
                Self::lock,
            ),
            LeaperPowerMsg::LogOut => Self::action_task(
                "Log Out",
                self.config.power.actions.log_out.clone(),
                self.connection.clone(),
                Self::terminate,
            ),
            LeaperPowerMsg::Hibernate => Self::action_task(
                "Hibernate",
                self.config.power.actions.hibernate.clone(),
                self.connection.clone(),
                Self::hibernate,
            ),
            LeaperPowerMsg::Reboot => Self::action_task(
                "Reboot",
                self.config.power.actions.reboot.clone(),
                self.connection.clone(),
                Self::reboot,
            ),
            LeaperPowerMsg::Shutdown => Self::action_task(
                "Shutdown",
                self.config.power.actions.shutdown.clone(),
                self.connection.clone(),
                Self::power_off,
            ),
            LeaperPowerMsg::ActionResult(result) => {
                if let Err(err) = result {
                    tracing::error!("Failed to perform logind action: {err}");
                }

                Self::Task::done(LeaperPowerMsg::Exit)
            }

            LeaperPowerMsg::AnchorChange(_)
            | LeaperPowerMsg::SetInputRegion(_)
            | LeaperPowerMsg::AnchorSizeChange(_, _)
            | LeaperPowerMsg::LayerChange(_)
            | LeaperPowerMsg::MarginChange(_)
            | LeaperPowerMsg::SizeChange(_)
            | LeaperPowerMsg::VirtualKeyboardPressed { .. } => Self::Task::none(),
        }
    }

    fn view(&self) -> Self::Element<'_> {
        let power_btn = |icon: Nerd, str: &'static str, msg: LeaperPowerMsg| {
            button(center(
                column![
                    text(icon_to_string(icon)).font(NERD_FONT).size(80),
                    text(str).size(30)
                ]
                .align_x(Horizontal::Center)
                .spacing(10),
            ))
            .width(200)
            .height(200)
            .on_press(msg)
        };

        center(
            row![
                power_btn(Nerd::AccountLock, "Lock", LeaperPowerMsg::Lock),
                power_btn(Nerd::Logout, "Log Out", LeaperPowerMsg::LogOut),
                power_btn(Nerd::Snowflake, "Hibernate", LeaperPowerMsg::Hibernate),
                power_btn(Nerd::RotateLeft, "Reboot", LeaperPowerMsg::Reboot),
                power_btn(Nerd::Power, "Shutdown", LeaperPowerMsg::Shutdown)
            ]
            .spacing(20),
        )
        .into()
    }

    fn subscription(&self) -> Self::Subscription {
        Self::Subscription::none()
    }

    fn title(&self) -> String {
        "Leaper Power Menu".into()
    }

    fn theme(&self) -> LeaperModeTheme {
        self.config.theme.clone()
    }
}

impl LeaperPower {
    async fn cmd(action: impl Into<String>, args: Vec<String>) -> LeaperPowerResult<()> {
        let program = args
            .first()
            .ok_or_else(|| LeaperPowerError::ActionCMDEmpty(action.into()))?;

        let mut cmd = tokio::process::Command::new(program);

        if args.len() > 1 {
            cmd.args(&args[1..]);
        }

        let mut process = cmd.spawn().map_err(Arc::new)?;
        process.wait().await.map_err(Arc::new)?;

        Ok(())
    }

    pub async fn zbus_connect() -> LeaperPowerResult<Connection> {
        Ok(connection::Builder::system()?
            .internal_executor(false)
            .build()
            .await?)
    }

    fn action_task<DF>(
        action: &'static str,
        method: ActionMethod,
        connection: Option<Connection>,
        dbus_fn: impl Fn(Option<Connection>) -> DF,
    ) -> <Self as LeaperMode>::Task
    where
        DF: Future<Output = LeaperPowerResult<()>> + Send + 'static,
    {
        match method {
            ActionMethod::Dbus => <Self as LeaperMode>::Task::perform(dbus_fn(connection), |res| {
                LeaperPowerMsg::ActionResult(res)
            }),
            ActionMethod::Cmd(args) => {
                <Self as LeaperMode>::Task::perform(Self::cmd(action, args), |res| {
                    LeaperPowerMsg::ActionResult(res)
                })
            }
        }
    }

    async fn get_logind_manager(connection: &'_ Connection) -> LeaperPowerResult<ManagerProxy<'_>> {
        Ok(ManagerProxy::new(connection).await?)
    }

    async fn get_logind_session(connection: &'_ Connection) -> LeaperPowerResult<SessionProxy<'_>> {
        Ok(SessionProxy::new(connection).await?)
    }

    logind_fns![
        get_logind_session => [
            lock["Failed to lock the session"],
            terminate["Failed to terminate the session"],
        ],
        get_logind_manager => [
            hibernate["Failed to hibernate"](false),
            reboot["Failed to reboot"](false),
            power_off["Failed to power off"](false),
        ],
    ];
}

#[to_layer_message]
#[derive(Debug, Clone)]
pub enum LeaperPowerMsg {
    Exit,

    ConnectZbus,
    ZbusConnected(LeaperPowerResult<Connection>),

    Lock,
    LogOut,
    Hibernate,
    Reboot,
    Shutdown,

    ActionResult(LeaperPowerResult<()>),
}

#[lerror]
#[lerr(prefix = "[leaper-power]", result_name = LeaperPowerResult)]
pub enum LeaperPowerError {
    #[lerr(str = "[std::io] {0}")]
    IO(#[lerr(from, wrap = Arc)] std::io::Error),

    #[lerr(str = "Layershell error: {0}")]
    LayerShell(#[lerr(from, wrap = Arc)] iced_layershell::Error),
    #[lerr(str = "Failed to connect to session bus: {0}")]
    ZBus(#[lerr(from)] zbus::Error),

    #[lerr(str = "{0}")]
    Config(#[lerr(from)] LeaperAppModeConfigError),

    #[lerr(str = "No ProjectDirs!")]
    NoProjectDirs,
    #[lerr(str = "Empty cmd args list for action {0}")]
    ActionCMDEmpty(String),
    #[lerr(str = "No dbus connection!")]
    NoDBusConnection,
}
