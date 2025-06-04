use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

macro_rules! builtins {
    ($($name:ident $({$($tt:tt)*})?),+ $(,)?) => {
        pastey::paste! {
            #[derive(Debug, Clone, SmartDefault, Serialize, Deserialize)]
            #[serde(default)]
            pub struct Builtins {
                $(pub [< $name:snake >]: $name),+
            }
        }

        $crate::config_modules![$($name $({$($tt)*})?),+];
    };
}

builtins![Finder {
    ignore_gitignore: bool = true,
    preview_images: bool = true,
}];
