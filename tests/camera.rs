//! Tests for opening and querying cameras, using PICam's demo cameras so no
//! hardware is needed.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, Parameter};

/// Connects, opens, queries, closes and disconnects a demo camera, as a
/// smoke test of the whole chain from `Library` through to a parameter read.
#[test]
fn demo_camera_round_trip() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "SMOKE0001";

    let test_library = common::library();

    let models = Camera::available_demo_models().expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");

    let model_name = model
        .name(&test_library.library)
        .expect("demo model should be named");
    assert!(
        !model_name.is_empty(),
        "demo model name should not be empty"
    );

    let id = CameraId::connect_demo(model, SERIAL_NUMBER).expect("demo camera should connect");
    assert!(id.is_demo().expect("demo status should be queried"));

    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");
    assert!(
        camera
            .is_connected()
            .expect("connection status should be queried")
    );

    // picam appends ":Demo" to demo serial numbers (seen with sdk 5.15) so
    // compare against the id it returned rather than the number we asked for
    let opened_id = camera.id().expect("camera id should be queried");
    assert_eq!(opened_id.serial_number, id.serial_number);
    assert!(
        opened_id.serial_number.starts_with(SERIAL_NUMBER),
        "serial number should start with {SERIAL_NUMBER}, got {}",
        opened_id.serial_number
    );

    // every camera reports its sensor size, whatever the model
    let width: i32 = camera
        .get(Parameter::SensorActiveWidth)
        .expect("sensor width should be read");
    assert!(width > 0, "sensor width should be positive, got {width}");

    drop(camera);
    id.disconnect_demo().expect("demo camera should disconnect");
}
