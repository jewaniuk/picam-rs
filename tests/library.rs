//! Tests for the PICam library lifecycle, i.e. [`Library`].

mod common;

use picam_rs::{Error, Library, Version};

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

/// Checks that PICam reports its version with no `Library`, and that the
/// installed runtime is at least the SDK the bindings were generated from.
#[test]
fn version_without_library() {
    // holds the lock so no other test's library is alive, the point here is
    // that none is needed
    let _guard = common::lock();

    let version = Library::version().expect("PICam should report its version");
    println!("PICam runtime {version}");
    let bindings = Version {
        major: 5,
        minor: 15,
        distribution: 0,
        released: 0,
    };
    assert!(
        version >= bindings,
        "the installed PICam runtime {version} is older than the 5.15 SDK the bindings came from"
    );
}

/// Checks that `Library::is_initialized` follows a `Library`'s lifetime.
#[test]
fn is_initialized_follows_library() {
    let _guard = common::lock();

    assert_eq!(Library::is_initialized(), Ok(false));
    let library = Library::new().expect("PICam library should initialize");
    assert_eq!(Library::is_initialized(), Ok(true));
    drop(library);
    assert_eq!(Library::is_initialized(), Ok(false));
}

/// Checks that a second `Library` can't be created while one is alive.
#[test]
fn second_library_fails_while_first_alive() {
    let _guard = common::lock();

    let _first = Library::new().expect("PICam library should initialize");
    let second = Library::new();
    assert_eq!(second.err(), Some(Error::LibraryAlreadyInitialized));
}
