//! Tests for getting and setting parameter values through `Camera`, which
//! go through each value type's sealed implementation, using PICam's demo
//! cameras so no hardware is needed.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, Parameter, Roi};

/// Connects a demo camera of the first model PICam offers.
fn connect_demo(serial_number: &str) -> CameraId {
    let models = Camera::available_demo_models().expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");
    CameraId::connect_demo(model, serial_number).expect("demo camera should connect")
}

/// Gets, checks and sets integer and floating-point values, and commits the
/// result.
#[test]
fn scalar_values_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0001";

    let test_library = common::library();
    let id = connect_demo(SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    let width: i32 = camera
        .get(Parameter::SensorActiveWidth)
        .expect("sensor width should be read");
    assert!(width > 0, "sensor width should be positive, got {width}");

    let exposure_time: f64 = camera
        .get(Parameter::ExposureTime)
        .expect("exposure time should be read");
    let _default_exposure_time: f64 = camera
        .default_value(Parameter::ExposureTime)
        .expect("default exposure time should be read");

    // the camera's own current value should always be acceptable to it
    assert!(
        camera
            .can_set(Parameter::ExposureTime, exposure_time)
            .expect("settability should be checked"),
        "current exposure time {exposure_time} should be settable"
    );
    camera
        .set(Parameter::ExposureTime, exposure_time)
        .expect("exposure time should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "no parameters should fail, got {failed:?}"
    );

    drop(camera);
    id.disconnect_demo().expect("demo camera should disconnect");
}

/// Gets, checks and sets the regions of interest, commits them and reads
/// them back.
#[test]
fn rois_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0002";

    let test_library = common::library();
    let id = connect_demo(SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    let rois: Vec<Roi> = camera.get(Parameter::Rois).expect("rois should be read");
    assert!(!rois.is_empty(), "a camera should have at least one roi");

    // the camera's own current value should always be acceptable to it
    assert!(
        camera
            .can_set(Parameter::Rois, rois.clone())
            .expect("settability should be checked"),
        "current rois {rois:?} should be settable"
    );
    camera
        .set(Parameter::Rois, rois.clone())
        .expect("rois should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "no parameters should fail, got {failed:?}"
    );

    let committed: Vec<Roi> = camera
        .get(Parameter::Rois)
        .expect("rois should be read back");
    assert_eq!(committed, rois);

    drop(camera);
    id.disconnect_demo().expect("demo camera should disconnect");
}
