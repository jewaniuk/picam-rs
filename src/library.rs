//! PICam library lifecycle.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::error::{Error, Result, picam_call};
use crate::model::Model;
use crate::sys;
use crate::watch;

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
    /// The most different demo cameras one `Library` tries to connect, where
    /// a demo camera is a model and serial number pair, counting attempts
    /// that fail.
    ///
    /// With PICam 5.15, connecting a 65th different pair in one library
    /// session corrupts PICam's heap inside `Picam_ConnectDemoCamera` and
    /// kills the process, whether the pairs differ by model, serial number or
    /// both, and even if each was disconnected before the next. Trying a pair
    /// already tried doesn't count again, and uninitializing and
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
    /// already active elsewhere in the process (seen with SDK 5.15), or an
    /// error if PICam is unable to initialize.
    pub fn new() -> Result<Self> {
        tracing::debug!("initializing PICam library");
        picam_call!(sys::Picam_InitializeLibrary())?;
        Ok(Self {
            demo_cameras: Mutex::new(HashSet::new()),
        })
    }

    /// Reports the installed PICam runtime's version.
    ///
    /// It works without an initialized library (PICam manual 3.2.1), so it
    /// takes no `&Library`.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to report its version.
    pub fn version() -> Result<Version> {
        let (mut major, mut minor, mut distribution, mut released) = (0, 0, 0, 0);
        picam_call!(sys::Picam_GetVersion(
            &mut major,
            &mut minor,
            &mut distribution,
            &mut released
        ))?;
        Ok(Version {
            major,
            minor,
            distribution,
            released,
        })
    }

    /// Reports whether PICam is initialized in this process, whether by a
    /// `Library` or by anything else.
    ///
    /// It works without an initialized library (PICam manual 3.2.2), so it
    /// takes no `&Library`.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to report it.
    pub fn is_initialized() -> Result<bool> {
        let mut initialized: sys::pibln = 0;
        picam_call!(sys::Picam_IsLibraryInitialized(&mut initialized))?;
        Ok(initialized != 0)
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
        // discovery watches belong to the library rather than to any camera
        watch::release(std::ptr::null_mut());
        // nowhere to hand a `Result` back to from `Drop`, so just report it
        if let Err(e) = picam_call!(sys::Picam_UninitializeLibrary()) {
            tracing::error!("failed to uninitialize PICam library: {e}");
        }
    }
}

/// A PICam version, as [`Library::version`] reports it.
///
/// Versions compare field by field, so a later release compares greater.
/// `Display` writes it the way PICam's manual does, as `5.15.11.2503`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    /// The major version, raised for major features or breaking changes.
    pub major: i32,
    /// The minor version, raised for minor features.
    pub minor: i32,
    /// The distribution version, raised for bug-fix releases.
    pub distribution: i32,
    /// The release date as `YYMM` (2503 is March 2025), or 0 for a beta
    /// release.
    pub released: i32,
}

impl Version {
    /// Reports whether this is a beta release, which PICam marks with a
    /// release date of 0 (PICam manual 3.2.1).
    pub fn is_beta(self) -> bool {
        self.released == 0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.distribution, self.released
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: i32, minor: i32, distribution: i32, released: i32) -> Version {
        Version {
            major,
            minor,
            distribution,
            released,
        }
    }

    #[test]
    fn displays_like_the_manual() {
        assert_eq!(version(5, 15, 11, 2503).to_string(), "5.15.11.2503");
        assert_eq!(version(5, 1, 2, 0).to_string(), "5.1.2.0");
    }

    #[test]
    fn later_versions_compare_greater() {
        assert!(version(5, 9, 0, 2001) < version(5, 15, 0, 2001));
        assert!(version(5, 15, 11, 2503) < version(6, 0, 0, 2601));
        assert!(version(5, 15, 10, 2412) < version(5, 15, 11, 2503));
    }

    #[test]
    fn zero_release_date_is_beta() {
        assert!(version(5, 1, 2, 0).is_beta());
        assert!(!version(5, 15, 11, 2503).is_beta());
    }
}
