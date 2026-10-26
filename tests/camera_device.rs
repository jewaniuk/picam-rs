//! Tests for camera devices (`Camera::open_device`, `Camera::device`,
//! `Camera::list_open_camera_devices`, the `CameraHandle` handle methods) and
//! moving parameter values between a camera's model and its device
//! (`commit_parameters_to_device`, `refresh_parameter(s)_from_device`), using
//! PIXIS 100F demo cameras.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, Error, HandleType, Model, Parameter, ReadoutControlMode, params};

/// Checks that `open_device` opens a camera with a model and a device
/// handle, which lead to each other, and that dropping the camera closes
/// both.
#[test]
fn open_device_opens_the_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DEVICE001";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open_device(&id, Arc::clone(library)).expect("demo camera should open");
    assert_eq!(
        camera.id().expect("camera id should be read").serial_number,
        id.serial_number
    );

    let models = Camera::list_open_cameras(library).expect("open cameras should be listed");
    assert_eq!(models.len(), 1);
    let model = models[0];
    assert_eq!(model.handle_type(), Ok(HandleType::CameraModel));
    let devices =
        Camera::list_open_camera_devices(library).expect("open camera devices should be listed");
    assert_eq!(devices.len(), 1);
    let device = devices[0];
    assert_eq!(device.handle_type(), Ok(HandleType::CameraDevice));

    let from_device = device.model().expect("the device's model should be found");
    assert_eq!(from_device.handle_type(), Ok(HandleType::CameraModel));
    assert_eq!(
        from_device
            .id()
            .expect("model id should be read")
            .serial_number,
        id.serial_number
    );
    let from_model = model.device().expect("the model's device should be found");
    assert_eq!(from_model.handle_type(), Ok(HandleType::CameraDevice));
    assert_eq!(
        from_model
            .id()
            .expect("device id should be read")
            .serial_number,
        id.serial_number
    );

    // a camera already open doesn't open again (seen with sdk 5.15)
    assert_eq!(
        Camera::open_device(&id, Arc::clone(library)).err(),
        Some(Error::CameraAlreadyOpened)
    );

    drop(camera);
    assert_eq!(Camera::list_open_cameras(library).map(|v| v.len()), Ok(0));
    assert_eq!(
        Camera::list_open_camera_devices(library).map(|v| v.len()),
        Ok(0)
    );
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that the device holds the committed values, not pending ones,
/// and that a value set through the device reaches the model.
#[test]
fn device_holds_the_committed_values() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DEVICE002";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    let device = camera.device().expect("the device should be found");

    let committed = camera
        .get(params::EXPOSURE_TIME)
        .expect("exposure time should be read");
    let pending = committed + 10.0;
    camera
        .set(params::EXPOSURE_TIME, pending)
        .expect("exposure time should be set");
    assert_eq!(device.get(params::EXPOSURE_TIME), Ok(committed));

    assert_eq!(camera.commit_parameters(), Ok(vec![]));
    assert_eq!(device.get(params::EXPOSURE_TIME), Ok(pending));

    // setting a device value sets the model's too (manual 5.4.1.3)
    let direct = pending + 10.0;
    device
        .set(params::EXPOSURE_TIME, direct)
        .expect("exposure time should be set on the device");
    assert_eq!(camera.get(params::EXPOSURE_TIME), Ok(direct));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that refreshing from the device discards pending values, one
/// parameter or all of them, and when the camera then reports its
/// parameters committed.
#[test]
fn refresh_discards_pending_values() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DEVICE003";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");

    let committed = camera
        .get(params::EXPOSURE_TIME)
        .expect("exposure time should be read");

    camera
        .set(params::EXPOSURE_TIME, committed + 10.0)
        .expect("exposure time should be set");
    assert_eq!(camera.are_parameters_committed(), Ok(false));
    camera
        .refresh_parameter_from_device(Parameter::ExposureTime)
        .expect("exposure time should be refreshed");
    assert_eq!(camera.get(params::EXPOSURE_TIME), Ok(committed));
    // the value is back, but the camera still reports uncommitted parameters
    // until a full refresh or a commit (seen with sdk 5.15)
    assert_eq!(camera.are_parameters_committed(), Ok(false));
    camera
        .refresh_parameters_from_device()
        .expect("parameters should be refreshed");
    assert_eq!(camera.are_parameters_committed(), Ok(true));

    camera
        .set(params::EXPOSURE_TIME, committed + 20.0)
        .expect("exposure time should be set");
    assert_eq!(camera.are_parameters_committed(), Ok(false));
    camera
        .refresh_parameters_from_device()
        .expect("parameters should be refreshed");
    assert_eq!(camera.get(params::EXPOSURE_TIME), Ok(committed));
    assert_eq!(camera.are_parameters_committed(), Ok(true));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that committing to the device configures it with the model's
/// values, and that a failed commit leaves the device as it was.
#[test]
fn commit_to_device() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DEVICE004";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    let device = camera.device().expect("the device should be found");

    let pending = camera
        .get(params::EXPOSURE_TIME)
        .expect("exposure time should be read")
        + 10.0;
    camera
        .set(params::EXPOSURE_TIME, pending)
        .expect("exposure time should be set");
    camera
        .commit_parameters_to_device()
        .expect("parameters should be committed to the device");
    assert_eq!(device.get(params::EXPOSURE_TIME), Ok(pending));
    assert_eq!(camera.are_parameters_committed(), Ok(true));

    // kinetics readout doesn't fit the full-sensor region, see
    // `tests/validation.rs`, and the commit fails with the singular error
    // (seen with sdk 5.15)
    let mode = device
        .get(params::READOUT_CONTROL_MODE)
        .expect("readout mode should be read");
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
        .expect("kinetics readout should be accepted as a pending value");
    assert_eq!(
        camera.commit_parameters_to_device(),
        Err(Error::InvalidParameterValue)
    );
    assert_eq!(device.get(params::READOUT_CONTROL_MODE), Ok(mode));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
