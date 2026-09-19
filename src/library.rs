//! PICam library lifecycle.

use crate::error::{Result, picam_call};
use crate::sys;

/// An open handle to the PICam library itself.
///
/// PICam requires `Picam_InitializeLibrary` to be called before anything
/// else, and `Picam_UninitializeLibrary` once you're done. `Library` turns
/// that requirement into something the type system enforces: you can't
/// obtain a `Camera` (or anything else) without a live `Library`, and
/// initialization/uninitialization become `new`/`Drop`.
pub struct Library {
    // a private field, so `Library` can't be constructed from outside this module
    _private: (),
}

impl Library {
    /// Initializes the PICam library.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::LibraryAlreadyInitialized`] if a `Library` is
    /// already active elsewhere in the process.
    pub fn new() -> Result<Self> {
        tracing::debug!("initializing PICam library");
        picam_call!(sys::Picam_InitializeLibrary())?;
        Ok(Self { _private: () })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // nowhere to hand a `Result` back to from `Drop`, so just report it
        if let Err(e) = picam_call!(sys::Picam_UninitializeLibrary()) {
            tracing::error!("failed to uninitialize PICam library: {e}");
        }
    }
}
