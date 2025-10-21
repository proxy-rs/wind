pub use const_str::concat;
pub use tracing;

#[macro_export]
macro_rules! info {
    (target: $target:expr, $($arg:tt)*) => {

		$crate::log::tracing::info!(target: $crate::log::concat!($crate::extract_crate_name!(), " ", $target), $($arg)*)
    };
    (name: $name:expr, target: $target:expr, $($arg:tt)*) => {

		$crate::log::tracing::info!(
            name: $name,
            target: $crate::log::concat!($crate::extract_crate_name!(), " ", $target),
            $($arg)*
        )
    };
    ($($arg:tt)*) => {
		$crate::log::tracing::info!($($arg)*)
    };
}

#[macro_export]
macro_rules! warn {
    (target: $target:expr, $($arg:tt)*) => {

		$crate::log::tracing::warn!(target: $crate::log::concat!($crate::extract_crate_name!(), " ", $target), $($arg)*)
    };
    (name: $name:expr, target: $target:expr, $($arg:tt)*) => {

		$crate::log::tracing::warn!(
            name: $name,
            target: $crate::log::concat!($crate::extract_crate_name!(), " ", $target),
            $($arg)*
        )
    };
    ($($arg:tt)*) => {
		$crate::log::tracing::warn!($($arg)*)
    };
}

#[macro_export]
macro_rules! error {
    (target: $target:expr, $($arg:tt)*) => {

		$crate::log::tracing::error!(target: $crate::log::concat!($crate::extract_crate_name!(), " ", $target), $($arg)*)
    };
    (name: $name:expr, target: $target:expr, $($arg:tt)*) => {

		$crate::log::tracing::error!(
            name: $name,
            target: $crate::log::concat!($crate::extract_crate_name!(), " ", $target),
            $($arg)*
        )
    };
    ($($arg:tt)*) => {
		$crate::log::tracing::error!($($arg)*)
    };
}

/// https://github.com/Gadiguibou/current_crate_name/blob/master/src/lib.rs
#[macro_export]
macro_rules! extract_crate_name {
	() => {{
		const MODULE_PATH_SEPARATOR: u8 = b':';
		const MODULE_PATH_BYTE_SLICE: &[u8] = module_path!().as_bytes();
		const MODULE_PATH_SEPARATOR_INDEX: usize = {
			let mut index = 0;
			loop {
				if index == MODULE_PATH_BYTE_SLICE.len() || MODULE_PATH_BYTE_SLICE[index] == MODULE_PATH_SEPARATOR {
					break index;
				}
				index += 1;
			}
		};
		const NEW_SLICE: [u8; MODULE_PATH_SEPARATOR_INDEX] = {
			let mut arr = [0; MODULE_PATH_SEPARATOR_INDEX];
			let mut i = 0;
			while i < MODULE_PATH_SEPARATOR_INDEX {
				arr[i] = MODULE_PATH_BYTE_SLICE[i];
				i += 1;
			}
			arr
		};

		// SAFETY: The original string was valid UTF-8 and we sliced it at the start of
		// an ASCII byte which is a valid unicode boundary. Hence the new string is
		// still valid UTF-8.
		unsafe { std::str::from_utf8_unchecked(&NEW_SLICE) }
	}};
}
