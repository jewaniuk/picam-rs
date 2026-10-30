//! Shared helpers for the integration tests.
//!
//! PICam can only be initialized once per process at a time, but the tasks
//! in one file run in parallel threads of the same process. Every test that
//! touches PICam therefore holds the global lock from [`lock`] for its whole
//! run, so these tests run one at a time, each starting from an
//! uninitialized library.

// each test file compiles this module separately and uses only some of it
#![allow(dead_code)]

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use picam_rs::Library;

/// Serializes every test that touches PICam.
static PICAM: Mutex<()> = Mutex::new(());

/// A fresh PICam library for one test, held under the global PICam lock.
///
/// Dropping it uninitializes the library and then releases the lock, so the
/// next test always starts from an uninitialized library.
pub struct TestLibrary {
    /// The library under test, clone it to open cameras.
    pub library: Arc<Library>,
    // fields drop in declaration order, keeping this after `library` means
    // the lock is only released once the library has been uninitialized
    _guard: MutexGuard<'static, ()>,
}

/// Takes the global PICam lock, for tests that manage `Library` themselves.
///
/// Hold the returned guard for the whole test.
pub fn lock() -> MutexGuard<'static, ()> {
    // a panicking test poisons the mutex but can't leave the `()` it guards
    // in a bad state, so carry on rather than failing every later test
    PICAM.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a hardware test runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hardware {
    /// The real device attached to this computer.
    Real,
    /// A demo PIXIS 100F, to rehearse a test without hardware.
    Demo,
}

/// Says whether the maintainer approved running hardware test `test`, by
/// naming it in `PICAM_RS_HARDWARE`, and what it then runs against: a demo
/// camera when `PICAM_RS_HARDWARE_DEMO` is `1`, otherwise the real device.
///
/// Otherwise it prints why the test was skipped and returns `None`, before
/// the test touches PICam at all. See `HARDWARE_TESTS.md`.
pub fn hardware_approved(test: &str) -> Option<Hardware> {
    if std::env::var("PICAM_RS_HARDWARE").as_deref() != Ok(test) {
        println!(
            "skipped `{test}`: not approved. It runs only with PICAM_RS_HARDWARE={test} \
             (see HARDWARE_TESTS.md)"
        );
        return None;
    }
    if std::env::var("PICAM_RS_HARDWARE_DEMO").as_deref() == Ok("1") {
        println!("`{test}`: rehearsing on a demo camera");
        Some(Hardware::Demo)
    } else {
        println!("`{test}`: running on real hardware");
        Some(Hardware::Real)
    }
}

/// Takes the global PICam lock and initializes a fresh library under it.
///
/// # Panics
///
/// Panics if PICam fails to initialize.
pub fn library() -> TestLibrary {
    let guard = lock();
    let library = Library::new().expect("PICam library should initialize");
    TestLibrary {
        library: Arc::new(library),
        _guard: guard,
    }
}
