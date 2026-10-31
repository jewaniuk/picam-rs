//! PICam library lifecycle.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::camera::Camera;
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
    /// The cameras that count towards PICam's limit, see `MAX_CAMERAS`. Also
    /// keeps `Library` from being constructed outside this module.
    cameras: Mutex<Cameras>,
}

/// The cameras, as model and serial number, that count towards PICam's limit
/// in one library session, see `Library::MAX_CAMERAS`.
#[derive(Default)]
struct Cameras {
    /// The demo cameras `connect_demo` has tried so far.
    demo: HashSet<(Model, String)>,
    /// Every real camera PICam has listed since the library started, kept
    /// even once it's gone.
    real: HashSet<(Model, String)>,
}

impl Library {
    /// The most cameras one `Library` connects: the different demo cameras
    /// it tries, where a demo camera is a model and serial number pair,
    /// counting attempts that fail, plus every real camera PICam has listed.
    ///
    /// With PICam 5.15 and 5.16, connecting a 65th different pair in one library
    /// session corrupts PICam's heap inside `Picam_ConnectDemoCamera` and
    /// kills the process, whether the pairs differ by model, serial number or
    /// both, and even if each was disconnected before the next. Trying a pair
    /// already tried doesn't count again, and uninitializing and
    /// reinitializing the library resets the count. This was found with demo
    /// cameras and reproduced with raw PICam calls, not documented. With a
    /// PIXIS 100BR connected, PICam 5.16.3 crashed on the 64th pair instead,
    /// so real cameras take places too. So
    /// [`CameraId::connect_demo`](crate::CameraId::connect_demo) also counts
    /// every real camera PICam has listed since the library started,
    /// available or open elsewhere, and returns
    /// [`Error::TooManyDemoCameras`] instead of crossing the limit.
    pub(crate) const MAX_CAMERAS: usize = 64;

    /// Initializes the PICam library.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::LibraryAlreadyInitialized`] if a `Library` is
    /// already active elsewhere in the process (seen with SDK 5.15), or an
    /// error if PICam is unable to initialize or to list its cameras.
    pub fn new() -> Result<Self> {
        tracing::debug!("initializing PICam library");
        picam_call!(sys::Picam_InitializeLibrary())?;
        // dropped on an error below, which uninitializes picam again
        let library = Self {
            cameras: Mutex::default(),
        };
        // a camera picam found as it started counts towards its limit, even
        // if it's gone before the first demo camera
        library.note_real_cameras()?;
        Ok(library)
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

    /// Locks the cameras counted towards PICam's limit.
    fn cameras(&self) -> MutexGuard<'_, Cameras> {
        // the sets can't be left half-updated, so a panic elsewhere while
        // they were locked is no reason to stop
        self.cameras.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records every real camera PICam lists now, available or open
    /// elsewhere, as counting towards its limit.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to list its cameras.
    fn note_real_cameras(&self) -> Result<()> {
        let mut ids = Camera::available_ids(self)?;
        ids.extend(Camera::unavailable_ids(self)?);
        let real: Vec<_> = ids
            .into_iter()
            // one picam can't tell apart counts as real, since counting one
            // too many only refuses a demo camera early
            .filter(|id| id.is_demo(self) != Ok(true))
            .map(|id| (id.model, id.serial_number))
            .collect();
        self.cameras().real.extend(real);
        Ok(())
    }

    /// Records that the demo camera `model`, `serial_number` is about to be
    /// connected, refusing a new one once the demo cameras tried and the real
    /// cameras listed reach `MAX_CAMERAS`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooManyDemoCameras`] if this demo camera is new and
    /// the limit has been reached, or an error if PICam is unable to list its
    /// cameras.
    pub(crate) fn note_demo_camera(&self, model: Model, serial_number: &str) -> Result<()> {
        let camera = (model, serial_number.to_string());
        if self.cameras().demo.contains(&camera) {
            return Ok(());
        }
        // a real camera that appeared since the last look counts too
        self.note_real_cameras()?;
        let mut cameras = self.cameras();
        // another thread may have tried the same pair meanwhile
        if cameras.demo.contains(&camera) {
            return Ok(());
        }
        if cameras.demo.len() + cameras.real.len() >= Self::MAX_CAMERAS {
            return Err(Error::TooManyDemoCameras);
        }
        cameras.demo.insert(camera);
        Ok(())
    }
}

impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the counts are enough, the sets themselves can hold 64 entries
        let cameras = self.cameras();
        f.debug_struct("Library")
            .field("demo_cameras_tried", &cameras.demo.len())
            .field("real_cameras_seen", &cameras.real.len())
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
