use iced::widget::{center, text_input};

use crate::app::{
    mode::{AppModeElement, AppModeTask},
    style::app_text_input_style,
};

#[derive(Default)]
pub struct Runner {
    input: String,
}

impl Runner {
    pub const INPUT_ID: &'static str = "command_input";

    pub fn update(&mut self, msg: RunnerMsg) -> AppModeTask {
        match msg {
            RunnerMsg::Input(new_input) => self.input = new_input,
            RunnerMsg::TryRun => {
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
                                    return iced::exit();
                                }
                                Err(err) => tracing::error!("Failed to run the command: {err}"),
                            }
                        }
                    },
                }
            }
        }

        AppModeTask::none()
    }

    pub fn view(&self) -> AppModeElement<'_> {
        center(
            text_input("Input command to run...", &self.input)
                .id(Self::INPUT_ID)
                .size(30)
                .padding(10)
                .style(app_text_input_style)
                .on_input(|s| RunnerMsg::Input(s).into())
                .on_submit(RunnerMsg::TryRun.into()),
        )
        .padding(10)
        .into()
    }
}

#[derive(Debug, Clone)]
pub enum RunnerMsg {
    Input(String),
    TryRun,
}
