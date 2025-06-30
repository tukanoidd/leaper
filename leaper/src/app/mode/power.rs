use std::sync::Arc;

use iced::{
    alignment::Horizontal,
    widget::{button, center, column, row, text},
};
use iced_fonts::{NERD_FONT, Nerd, nerd::icon_to_string};
use logind_zbus::{manager::ManagerProxy, session::SessionProxy};
use zbus::{Connection, connection};

use crate::{
    LeaperError, LeaperResult,
    app::mode::{AppModeElement, AppModeMsg, AppModeTask},
    config::{ActionMethod, Config},
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
            async fn $fn(connection: Option<Connection>) -> LeaperResult<()> {
                let Some(connection) = connection else {
                    return Err(LeaperError::NoDBusConnection);
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
pub struct Power {
    connection: Option<Connection>,
}

impl Power {
    pub fn update(&mut self, msg: PowerMsg, config: Arc<Config>) -> AppModeTask {
        match msg {
            PowerMsg::ConnectZbus => AppModeTask::perform(Power::zbus_connect(), |res| {
                PowerMsg::ZbusConnected(res).into()
            }),
            PowerMsg::ZbusConnected(connection) => match connection {
                Ok(connection) => {
                    self.connection = Some(connection);
                    AppModeTask::none()
                }
                Err(e) => {
                    tracing::error!("{}", e);
                    AppModeTask::done(AppModeMsg::Exit)
                }
            },

            PowerMsg::Lock => Self::action_task(
                "Lock",
                config.power.actions.lock.clone(),
                self.connection.clone(),
                Self::lock,
            ),
            PowerMsg::LogOut => Self::action_task(
                "Log Out",
                config.power.actions.log_out.clone(),
                self.connection.clone(),
                Self::terminate,
            ),
            PowerMsg::Hibernate => Self::action_task(
                "Hibernate",
                config.power.actions.hibernate.clone(),
                self.connection.clone(),
                Self::hibernate,
            ),
            PowerMsg::Reboot => Self::action_task(
                "Reboot",
                config.power.actions.reboot.clone(),
                self.connection.clone(),
                Self::reboot,
            ),
            PowerMsg::Shutdown => Self::action_task(
                "Shutdown",
                config.power.actions.shutdown.clone(),
                self.connection.clone(),
                Self::power_off,
            ),

            PowerMsg::ActionResult(result) => {
                if let Err(err) = result {
                    tracing::error!("Failed to perform logind action: {err}");
                }

                AppModeTask::done(AppModeMsg::Exit)
            }
        }
    }

    pub fn view(&self) -> AppModeElement<'_> {
        let power_btn = |icon: Nerd, str: &'static str, msg: AppModeMsg| {
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
                power_btn(Nerd::AccountLock, "Lock", PowerMsg::Lock.into()),
                power_btn(Nerd::Logout, "Log Out", PowerMsg::LogOut.into()),
                power_btn(Nerd::Snowflake, "Hibernate", PowerMsg::Hibernate.into()),
                power_btn(Nerd::RotateLeft, "Reboot", PowerMsg::Reboot.into()),
                power_btn(Nerd::Power, "Shutdown", PowerMsg::Shutdown.into())
            ]
            .spacing(20),
        )
        .into()
    }

    pub async fn zbus_connect() -> LeaperResult<Connection> {
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
    ) -> AppModeTask
    where
        DF: Future<Output = LeaperResult<()>> + Send + 'static,
    {
        match method {
            ActionMethod::Dbus => AppModeTask::perform(dbus_fn(connection), |res| {
                PowerMsg::ActionResult(res).into()
            }),
            ActionMethod::Cmd(args) => AppModeTask::perform(Self::cmd(action, args), |res| {
                PowerMsg::ActionResult(res).into()
            }),
        }
    }

    async fn get_logind_manager(connection: &'_ Connection) -> LeaperResult<ManagerProxy<'_>> {
        Ok(ManagerProxy::new(connection).await?)
    }

    async fn get_logind_session(connection: &'_ Connection) -> LeaperResult<SessionProxy<'_>> {
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

    async fn cmd(action: impl Into<String>, args: Vec<String>) -> LeaperResult<()> {
        let program = args
            .first()
            .ok_or_else(|| LeaperError::ActionCMDEmpty(action.into()))?;

        let mut cmd = tokio::process::Command::new(program);

        if args.len() > 1 {
            cmd.args(&args[1..]);
        }

        let mut process = cmd.spawn().map_err(Arc::new)?;
        process.wait().await.map_err(Arc::new)?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum PowerMsg {
    ConnectZbus,
    ZbusConnected(LeaperResult<Connection>),

    Lock,
    LogOut,
    Hibernate,
    Reboot,
    Shutdown,

    ActionResult(LeaperResult<()>),
}
