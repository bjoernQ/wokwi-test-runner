use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! timing {
    ($($arg:tt)*) => {{
        if $crate::trace::enabled() {
            eprintln!($($arg)*);
        }
    }};
}
