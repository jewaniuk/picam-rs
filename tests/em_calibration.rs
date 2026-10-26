//! Tests for EM gain calibration (`EmCalibration`). PICam refuses to open a
//! demo camera for calibration, so these check that refusal and the empty
//! list of open calibrations, and the rest is on the README's "to test"
//! list.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, EmCalibration, Error, Model};

/// Checks that a demo EM camera can't be opened for calibration, and that no
/// calibration is open.
#[test]
fn demo_cameras_refuse_calibration() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "EMCAL0001";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PRO_EM_512B, SERIAL_NUMBER)
        .expect("demo camera should connect");

    // seen with sdk 5.15, the manual doesn't say demo cameras can't be
    // calibrated
    assert_eq!(
        EmCalibration::open(&id, Arc::clone(library)).err(),
        Some(Error::DemoNotSupported)
    );
    let open =
        EmCalibration::list_open_calibrations(library).expect("open calibrations should be listed");
    assert!(open.is_empty(), "no calibration should be open");
    // the refused calibration leaves the camera free to open normally
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
