macro_rules! register_const {
    ($(#[$meta:meta])*$name:ident, $type:ty, $value:expr) => {
        $(#[$meta])*
        pub const $name: $type = $value;
    };
    () => {};
}

macro_rules! register_mut_const {
    ($(#[$meta:meta])*$name:ident, $type:ty, $value:expr) => {
        $(#[$meta])*
        static mut $name: $type = $value;
        paste::paste! {
            $(#[$meta])*
            pub fn [<$name:lower>]() -> $type {
                unsafe { $name }
            }
        }
        paste::paste! {
            pub fn [<set_ $name:lower>](num: $type) {
                unsafe {
                    $name = num;
                }
            }
        }
    };
    () => {};
}

macro_rules! register_fn {
    ($(#[$meta:meta])*$name:ident, $type:tt, $value:expr) => {
        $(#[$meta])*
        pub fn $name() -> $type {
            unsafe { $value }
        }
    };
    () => {};
}

pub(super) use {register_const, register_fn, register_mut_const};
