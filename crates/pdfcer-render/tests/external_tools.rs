//! Locks for external programs that the tests launch and that cannot run as
//! two instances at once.

use std::sync::{Mutex, MutexGuard};

static INKSCAPE: Mutex<()> = Mutex::new(());

/// Serialises Inkscape launches: a second concurrent instance dies on
/// Inkscape's single-instance D-Bus registration (`Gio::DBus::Error`). All
/// integration tests share one process, so two oracle tests would otherwise
/// overlap.
pub fn inkscape_lock() -> MutexGuard<'static, ()> {
    INKSCAPE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
