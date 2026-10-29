//! Camera and accessory discovery: watching for hardware becoming available
//! or unavailable (`picam_advanced.h` and `picam_accessory.h`, PICam manual
//! 7.3.1, 7.3.2, 7.4.1 and 7.4.2).

use std::sync::mpsc::Receiver;

use crate::accessory::{Accessory, AccessoryId};
use crate::camera::{Camera, CameraId};
use crate::convert::define_picam_enum;
use crate::error::{Result, picam_call};
use crate::library::Library;
use crate::sys;
use crate::watch::{self, Event, Kind, Watch};

define_picam_enum!(
    /// What discovery reports happened to a camera or accessory (PICam manual
    /// 7.1.1.1).
    DiscoveryAction,
    sys::PicamDiscoveryAction,
    PicamEnumeratedType_DiscoveryAction,
    /// It's now available for use.
    Found = PicamDiscoveryAction_Found,
    /// It's no longer available for use. PICam may report this again while it
    /// stays unavailable (manual 7.4.1.1).
    Lost = PicamDiscoveryAction_Lost,
    /// It has critically malfunctioned.
    Faulted = PicamDiscoveryAction_Faulted,
);

/// A change in a camera's availability, as [`Camera::watch_discovery`] sends
/// it (PICam manual 7.3.1.1).
#[derive(Debug, Clone)]
pub struct CameraDiscovery {
    /// The camera.
    pub id: CameraId,
    /// What happened to it.
    pub action: DiscoveryAction,
    /// Whether the camera is open in this process.
    pub open_in_this_process: bool,
}

/// A change in an accessory's availability, as [`Accessory::watch_discovery`]
/// sends it (PICam manual 7.3.2.1).
#[derive(Debug, Clone)]
pub struct AccessoryDiscovery {
    /// The accessory.
    pub id: AccessoryId,
    /// What happened to it.
    pub action: DiscoveryAction,
    /// Whether the accessory is open in this process.
    pub open_in_this_process: bool,
}

impl Camera {
    /// Watches for cameras becoming available or unavailable while discovery
    /// runs (PICam manual 7.4.1.1): connected or disconnected, powered on or
    /// off, opened or closed in another process, or faulted. Events arrive
    /// from PICam's own thread, one at a time. Dropping the library clears
    /// every discovery watch, and their receivers report the channel
    /// disconnected.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to register its callback.
    pub fn watch_discovery(library: &Library) -> Result<(Watch, Receiver<CameraDiscovery>)> {
        let _ = library; // only taken to prove picam is initialized
        watch::subscribe_discovery(Kind::CameraDiscovery, |event| match event {
            Event::CameraDiscovery(discovery) => Some(discovery.clone()),
            _ => None,
        })
    }

    /// Starts discovering cameras in the background (PICam manual 7.4.1.3).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::AlreadyDiscoveringCameras`] if discovery is
    /// already running (seen with SDK 5.15), or an error if PICam is unable
    /// to start.
    pub fn start_discovery(library: &Library) -> Result<()> {
        let _ = library; // only taken to prove picam is initialized
        picam_call!(sys::PicamAdvanced_DiscoverCameras())
    }

    /// Stops discovering cameras (PICam manual 7.4.1.4).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::NotDiscoveringCameras`] if discovery isn't
    /// running (seen with SDK 5.15), or an error if PICam is unable to
    /// stop.
    pub fn stop_discovery(library: &Library) -> Result<()> {
        let _ = library; // only taken to prove picam is initialized
        picam_call!(sys::PicamAdvanced_StopDiscoveringCameras())
    }

    /// Reports whether camera discovery is running (PICam manual 7.4.1.5).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn is_discovering(library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
        let mut discovering: sys::pibln = 0;
        picam_call!(sys::PicamAdvanced_IsDiscoveringCameras(&mut discovering))?;
        Ok(discovering != 0)
    }
}

impl Accessory {
    /// Watches for accessories becoming available or unavailable while
    /// discovery runs (PICam manual 7.4.2.1): connected or disconnected,
    /// powered on or off, opened or closed in another process, or faulted.
    /// Events arrive from PICam's own thread, one at a time. Dropping the
    /// library clears every discovery watch, and their receivers report the
    /// channel disconnected.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to register its callback.
    pub fn watch_discovery(library: &Library) -> Result<(Watch, Receiver<AccessoryDiscovery>)> {
        let _ = library; // only taken to prove picam is initialized
        watch::subscribe_discovery(Kind::AccessoryDiscovery, |event| match event {
            Event::AccessoryDiscovery(discovery) => Some(discovery.clone()),
            _ => None,
        })
    }

    /// Starts discovering accessories in the background (PICam manual
    /// 7.4.2.3).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::AlreadyDiscoveringAccessories`] if discovery
    /// is already running (seen with SDK 5.15), or an error if PICam is
    /// unable to start.
    pub fn start_discovery(library: &Library) -> Result<()> {
        let _ = library; // only taken to prove picam is initialized
        picam_call!(sys::PicamAccessory_DiscoverAccessories())
    }

    /// Stops discovering accessories (PICam manual 7.4.2.4).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::NotDiscoveringAccessories`] if discovery
    /// isn't running (seen with SDK 5.15), or an error if PICam is unable to
    /// stop.
    pub fn stop_discovery(library: &Library) -> Result<()> {
        let _ = library; // only taken to prove picam is initialized
        picam_call!(sys::PicamAccessory_StopDiscoveringAccessories())
    }

    /// Reports whether accessory discovery is running (PICam manual 7.4.2.5).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn is_discovering(library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
        let mut discovering: sys::pibln = 0;
        picam_call!(sys::PicamAccessory_IsDiscoveringAccessories(
            &mut discovering
        ))?;
        Ok(discovering != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::assert_round_trips;

    #[test]
    fn discovery_action_round_trips() {
        assert_round_trips!(DiscoveryAction, sys::PicamDiscoveryAction, 3);
    }
}
