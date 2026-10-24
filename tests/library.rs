//! Tests for the PICam library lifecycle, i.e. [`Library`].

mod common;

use picam_rs::{Error, Library};

/// Checks that PICam can be initialized again after being uninitialized in
/// the same process, which the one-library-per-test approach in `common`
/// depends on.
#[test]
fn reinitialize_after_drop() {
    let _guard = common::lock();

    let first = Library::new().expect("PICam library should initialize");
    drop(first);

    // if this fails, PICam can't be re-initialized within one process, and
    // the tests need to share a single library instead (see DESIGN.md)
    let second = Library::new();
    assert!(
        second.is_ok(),
        "PICam failed to re-initialize after uninitializing: {:?}",
        second.err()
    );
}

/// Checks that a second `Library` can't be created while one is alive.
#[test]
fn second_library_fails_while_first_alive() {
    let _guard = common::lock();

    let _first = Library::new().expect("PICam library should initialize");
    let second = Library::new();
    assert_eq!(second.err(), Some(Error::LibraryAlreadyInitialized));
}
