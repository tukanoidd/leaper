pub mod builtins;

#[macro_export]
macro_rules! config_modules {
    ($($name:ident {$($tt:tt)+}),+ $(,)?) => {
        $(
            $crate::config_modules![@fields $name {$($tt)*}];
        )+
    };

    (@fields $name:ident {
        $($field:ident: $field_ty:ty $(= $default_val:expr)?),*
        $(,)?
    }) => {
        #[derive(
            Debug,
            Clone,
            smart_default::SmartDefault,
            serde::Serialize,
            serde::Deserialize
        )]
        #[serde(default)]
        pub struct $name {
            $($(#[default($default_val)])? pub $field: $field_ty),*
        }
    };
}
