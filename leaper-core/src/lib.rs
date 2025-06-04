pub mod config;
pub mod modules;
pub mod state;

#[macro_export]
macro_rules! err_from_wrapped {
    ($ty:ty {
        $($name:ident: $which:ty [$wrap:ident]),+
        $(,)?
    }) => {
        $(
            impl From<$which> for $ty {
                fn from(value: $which) -> $ty {
                    <$ty>::$name($crate::err_from_wrapped!(
                        @wrap $wrap value
                    ))
                }
            }
        )+
    };

    (@wrap Arc $val:expr) => {std::sync::Arc::new($val)};
    (@wrap Box $val:expr) => {Box::new($val)};
}
