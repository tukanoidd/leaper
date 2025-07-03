use std::sync::Arc;

use crate::config::Config;

pub mod apps;
pub mod power;
pub mod runner;

macro_rules! app_mode {
    (
        $(| all_upd: ($($upd_arg:ident: $upd_arg_ty:ty),+);)?
        $(| all_view: ($($view_arg:ident: $view_arg_ty:ty),+);)?

        $(
            $name:ident {
                $(update: ($($mode_upd_arg:ident),+);)?
                $(view: ($($mode_view_arg:ident),+);)?
            }
        ),+
        $(,)?
    ) => {
        pastey::paste! {
            pub type AppModeElement<'a> = iced::Element<
                'a,
                AppModeMsg,
                $crate::app::AppTheme,
                $crate::app::AppRenderer
            >;
            pub type AppModeTask<Msg = AppModeMsg> = iced::Task<Msg>;

            pub enum AppMode {
                $($name([< $name:snake >]::$name)),+
            }

            impl AppMode {
                pub fn view(&self $($(, $view_arg: $view_arg:ty)+)?) -> AppModeElement<'_> {
                    match self {
                        $(Self::$name(mode) => mode.view($($($mode_view_arg),+)?).map(Into::into)),+
                    }
                }

                pub fn update(&mut self, msg: AppModeMsg $($(, $upd_arg:$upd_arg_ty)+)?) -> $crate::app::AppTask {
                    match (self, msg) {
                        (_, AppModeMsg::Exit {
                            app_search_stop_sender

                        }) => $crate::app::AppTask::done($crate::app::AppMsg::Exit {
                            app_search_stop_sender
                        }),
                        $((Self::$name(mode), AppModeMsg::$name(msg)) => mode.update(msg $($(, $mode_upd_arg)+)?).map(Into::into),)+
                        _ => {
                            tracing::trace!("[WARN] Trying to do an action for a wrong mode! (Better fix this to ensure this doesnt happen at all)");
                            AppModeTask::none()
                        }
                    }
                }
            }

            impl From<$crate::cli::AppMode> for AppMode {
                fn from(value: $crate::cli::AppMode) -> Self {
                    match value {
                        $($crate::cli::AppMode::$name => Self::$name(Default::default())),+
                    }
                }
            }

            impl<M> From<M> for $crate::app::AppMsg where M: Into<AppModeMsg> {
                fn from(value: M) -> Self {
                    Self::Mode(value.into())
                }
            }

            #[derive(Debug, Clone, derive_more::From)]
            pub enum AppModeMsg {
                Exit {
                    app_search_stop_sender: std::sync::Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>
                },
                $($name([< $name:snake >]::[< $name Msg >])),+
            }
        }
    };
}

app_mode![
    | all_upd: (config: Arc<Config>);

    Apps {},
    Runner {},
    Power {
        update: (config);
    }
];
