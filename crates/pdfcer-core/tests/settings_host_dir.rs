//! `settings::set_store_dir` is process-global, so it is tested in its own
//! test binary, in one function.

use pdfcer_core::settings::{self, StoreDirError, StoreKind};

#[test]
fn a_host_supplied_directory_wins_and_is_fixed_once_resolved() {
    let dir = std::env::temp_dir().join(format!("pdfcer-host-dir-{}", std::process::id()));
    let other = dir.join("other");
    settings::set_store_dir(&dir).unwrap();
    settings::set_store_dir(&dir).expect("the same directory again is fine");
    assert_eq!(
        settings::set_store_dir(&other),
        Err(StoreDirError::AlreadySet(dir.clone()))
    );
    let store = settings::resolve_store();
    assert_eq!(store.kind, StoreKind::HostSupplied);
    assert_eq!(store.directory(), Some(dir.as_path()));
    assert!(matches!(
        settings::set_store_dir(&dir),
        Err(StoreDirError::AlreadyResolved { resolved: Some(d) }) if d == dir
    ));
    let _ = std::fs::remove_dir_all(&dir);
}
