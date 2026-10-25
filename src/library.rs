//! PICam library lifecycle.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::error::{Error, Result, picam_call};
use crate::model::Model;
use crate::sys;

/// An open handle to the PICam library itself.
///
/// PICam requires `Picam_InitializeLibrary` to be called before anything
/// else, and `Picam_UninitializeLibrary` once you're done. `Library` turns
/// that requirement into something the type system enforces: you can't
/// obtain a `Camera` (or anything else) without a live `Library`, and
/// initialization/uninitialization become `new`/`Drop`.
pub struct Library {
    /// The demo cameras, as model and serial number, that `connect_demo` has
    /// tried so far, see `MAX_DEMO_CAMERAS`. Also keeps `Library` from being
    /// constructed outside this module.
    demo_cameras: Mutex<HashSet<(Model, String)>>,
}

impl Library {
    /// The most different demo cameras one `Library` connects, where a demo
    /// camera is a model and serial number pair.
    ///
    /// With PICam 5.15, connecting a 65th different pair in one library
    /// session corrupts PICam's heap inside `Picam_ConnectDemoCamera` and
    /// kills the process, whether the pairs differ by model, serial number or
    /// both, and even if each was disconnected before the next. Reconnecting
    /// a pair already used doesn't count again, and uninitializing and
    /// reinitializing the library resets the count. This was found with demo
    /// cameras and reproduced with raw PICam calls, not documented, so
    /// [`CameraId::connect_demo`](crate::CameraId::connect_demo) returns
    /// [`Error::TooManyDemoCameras`] instead of crossing it.
    pub(crate) const MAX_DEMO_CAMERAS: usize = 64;

    /// Initializes the PICam library.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::LibraryAlreadyInitialized`] if a `Library` is
    /// already active elsewhere in the process.
    pub fn new() -> Result<Self> {
        tracing::debug!("initializing PICam library");
        picam_call!(sys::Picam_InitializeLibrary())?;
        Ok(Self {
            demo_cameras: Mutex::new(HashSet::new()),
        })
    }

    /// Records that the demo camera `model`, `serial_number` is about to be
    /// connected, refusing a new one once `MAX_DEMO_CAMERAS` have been.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooManyDemoCameras`] if this demo camera is new and
    /// the limit has been reached.
    pub(crate) fn note_demo_camera(&self, model: Model, serial_number: &str) -> Result<()> {
        // the set can't be left half-updated, so a panic elsewhere while it
        // was locked is no reason to stop
        let mut demo_cameras = self
            .demo_cameras
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let camera = (model, serial_number.to_string());
        if demo_cameras.contains(&camera) {
            return Ok(());
        }
        if demo_cameras.len() >= Self::MAX_DEMO_CAMERAS {
            return Err(Error::TooManyDemoCameras);
        }
        demo_cameras.insert(camera);
        Ok(())
    }
}

impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the count is enough, the set itself can hold 64 entries
        let demo_cameras_tried = self
            .demo_cameras
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        f.debug_struct("Library")
            .field("demo_cameras_tried", &demo_cameras_tried)
            .finish()
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
