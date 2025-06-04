pub mod applications;
pub mod finder;

use miette::Diagnostic;
use thiserror::Error;

pub enum LeaperMode {
    Apps,
    Clipboard,
}

pub type ModulesResult<T> = Result<T, ModulesError>;

#[derive(Debug, Error, Diagnostic)]
pub enum ModulesError {
    #[error("Plugin '{0}' not found!")]
    FindPlugin(String),
}
