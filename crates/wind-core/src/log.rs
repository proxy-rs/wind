#[macro_export]
macro_rules! info {
    (target: $target:expr, $($arg:tt)*) => {

		tracing::info!(target: const_str::concat!($crate::log::current_crate_name(), " ", $target), $($arg)*)
    };
    (name: $name:expr, target: $target:expr, $($arg:tt)*) => {

        tracing::info!(
            name: $name,
            target: const_str::concat!($crate::log::current_crate_name(), " ", $target),
            $($arg)*
        )
    };
    ($($arg:tt)*) => {
        tracing::info!($($arg)*)
    };
}

/// https://github.com/Gadiguibou/current_crate_name/blob/master/src/lib.rs
#[allow(non_upper_case_globals)]
pub const fn current_crate_name() -> &'static str {
	const module_path_separator: u8 = b':';
	const module_path_byte_slice: &[u8] = module_path!().as_bytes();
	const module_path_separator_index: usize = {
		let mut index = 0;
		loop {
			if index == module_path_byte_slice.len()
				|| module_path_byte_slice[index] == module_path_separator
			{
				break index;
			}
			index += 1;
		}
	};
	const new_slice: [u8; module_path_separator_index] = {
		let mut arr = [0; module_path_separator_index];
		let mut i = 0;
		while i < module_path_separator_index {
			arr[i] = module_path_byte_slice[i];
			i += 1;
		}
		arr
	};

	// SAFETY: The original string was valid UTF-8 and we sliced it at the start of
	// an ASCII byte which is a valid unicode boundary. Hence the new string is
	// still valid UTF-8.
	unsafe { std::str::from_utf8_unchecked(&new_slice) }
}
