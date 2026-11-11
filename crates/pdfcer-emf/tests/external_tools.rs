//! Locks for external programs that the tests launch and that cannot run as
//! two instances at once.

use std::sync::{Mutex, MutexGuard};

static INKSCAPE: Mutex<()> = Mutex::new(());

/// Serialises Inkscape launches: a second concurrent instance dies on
/// Inkscape's single-instance D-Bus registration (`Gio::DBus::Error`). All
/// of this crate's integration tests share one process; cargo runs this
/// binary and `pdfcer-render`'s one after the other, never together.
pub fn inkscape_lock() -> MutexGuard<'static, ()> {
    INKSCAPE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
