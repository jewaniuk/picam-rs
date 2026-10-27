//! Tests for camera and accessory discovery (`watch_discovery`,
//! `start_discovery`, `stop_discovery` and `is_discovering`), using demo
//! cameras.

mod common;

use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use picam_rs::{
    Accessory, Camera, CameraDiscovery, CameraId, DiscoveryAction, Error, Library, Model,
};

/// Waits for the next event about the camera with `serial_number`, skipping
/// events about any other.
fn next_event(events: &Receiver<CameraDiscovery>, serial_number: &str) -> CameraDiscovery {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .expect("a discovery event should arrive");
        if event.id.serial_number == serial_number {
            return event;
        }
    }
}

/// Checks that discovery reports a demo camera connecting and disconnecting
/// while it runs, and that starting or stopping it twice is refused (seen
/// with sdk 5.15).
#[test]
fn camera_discovery_reports_demo_cameras() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DISCOVER01";

    let test_library = common::library();
    let library = &test_library.library;
    let (discovery_watch, events) =
        Camera::watch_discovery(library).expect("discovery should be watched");
    assert_eq!(Camera::is_discovering(library), Ok(false));
    Camera::start_discovery(library).expect("discovery should start");
    assert_eq!(Camera::is_discovering(library), Ok(true));
    assert_eq!(
        Camera::start_discovery(library),
        Err(Error::AlreadyDiscoveringCameras)
    );

    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let found = next_event(&events, &id.serial_number);
    assert_eq!(found.action, DiscoveryAction::Found);
    assert!(!found.open_in_this_process);
    let serial_number = id.serial_number.clone();
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
    let lost = next_event(&events, &serial_number);
    assert_eq!(lost.action, DiscoveryAction::Lost);

    Camera::stop_discovery(library).expect("discovery should stop");
    assert_eq!(Camera::is_discovering(library), Ok(false));
    assert_eq!(
        Camera::stop_discovery(library),
        Err(Error::NotDiscoveringCameras)
    );
    drop(discovery_watch);
}

/// Checks that dropping the library clears its discovery watches, so their
/// receivers disconnect and dropping them afterwards is harmless, and that a
/// later library can watch again.
#[test]
fn dropping_the_library_clears_discovery_watches() {
    let _guard = common::lock();

    let library = Library::new().expect("PICam library should initialize");
    let (discovery_watch, events) =
        Camera::watch_discovery(&library).expect("discovery should be watched");
    drop(library);
    assert_eq!(events.try_recv().err(), Some(TryRecvError::Disconnected));
    drop(discovery_watch);

    let library = Library::new().expect("PICam library should initialize again");
    let (again_watch, again) =
        Camera::watch_discovery(&library).expect("discovery should be watched again");
    assert_eq!(again.try_recv().err(), Some(TryRecvError::Empty));
    drop(again_watch);
    drop(library);
}

/// Checks that accessory discovery starts and stops. No demo accessories
/// exist, so no events arrive (seen with sdk 5.15).
#[test]
fn accessory_discovery_runs() {
    let test_library = common::library();
    let library = &test_library.library;
    let (discovery_watch, events) =
        Accessory::watch_discovery(library).expect("accessory discovery should be watched");
    Accessory::start_discovery(library).expect("accessory discovery should start");
    assert_eq!(Accessory::is_discovering(library), Ok(true));
    assert_eq!(
        Accessory::start_discovery(library),
        Err(Error::AlreadyDiscoveringAccessories)
    );
    assert_eq!(
        events.recv_timeout(Duration::from_millis(500)).err(),
        Some(RecvTimeoutError::Timeout)
    );
    Accessory::stop_discovery(library).expect("accessory discovery should stop");
    assert_eq!(Accessory::is_discovering(library), Ok(false));
    assert_eq!(
        Accessory::stop_discovery(library),
        Err(Error::NotDiscoveringAccessories)
    );
    drop(discovery_watch);
}
