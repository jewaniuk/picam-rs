//! Tests for camera devices (`Camera::open_device`, `Camera::device`,
//! `Camera::list_open_camera_devices`, the `CameraHandle` handle methods) and
//! moving parameter values between a camera's model and its device
//! (`commit_parameters_to_device`, `refresh_parameter(s)_from_device`),
//! including during an acquisition, using PIXIS 100F demo cameras.

mod common;

use std::sync::Arc;
use std::time::Duration;

use picam_rs::{
    Camera, CameraId, ConstraintCategory, ConstraintType, Error, HandleType, Model, Parameter,
    ReadoutControlMode, Roi, ValueAccess, ValueType, params,
};

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

/// A value `parameter` can take other than its current one, from its capable
/// constraint, for the parameters set through a plain number. A range gives
/// only its minimum, so a long exposure never slows the acquisition. `None`
/// for parameters that can't be set or have no such value.
fn other_value(camera: &Camera, parameter: Parameter) -> Option<f64> {
    if camera.value_access(parameter).ok()? == ValueAccess::ReadOnly {
        return None;
    }
    let current = match camera.value_type(parameter).ok()? {
        ValueType::FloatingPoint => camera.get_untyped::<f64>(parameter).ok()?,
        ValueType::LargeInteger => camera.get_untyped::<i64>(parameter).ok()? as f64,
        ValueType::Integer | ValueType::Boolean | ValueType::Enumeration => {
            f64::from(camera.get_untyped::<i32>(parameter).ok()?)
        }
        _ => return None,
    };
    let candidates = match camera.constraint_type(parameter).ok()? {
        ConstraintType::Collection => {
            camera
                .collection_constraint(parameter, ConstraintCategory::Capable)
                .ok()?
                .values
        }
        ConstraintType::Range => {
            let range = camera
                .range_constraint(parameter, ConstraintCategory::Capable)
                .ok()?;
            vec![range.minimum]
        }
        _ => return None,
    };
    candidates.into_iter().find(|&value| value != current)
}

/// Checks that during an acquisition PICam refuses every change through the
/// device except to parameters it can set online, so the data layout the
/// acquisition views rely on can't change through a `CameraDevice`.
#[test]
fn device_changes_are_refused_while_acquiring() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DEVICE005";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let mut camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    camera
        .set(params::READOUT_COUNT, 2)
        .expect("readout count should be set");
    camera
        .set(params::EXPOSURE_TIME, 1000.0)
        .expect("exposure time should be set");
    assert_eq!(camera.commit_parameters(), Ok(vec![]));
    let readout_stride = usize::try_from(
        camera
            .get(params::READOUT_STRIDE)
            .expect("readout stride should be read"),
    )
    .expect("readout stride should be positive");
    let rois = camera.get(params::ROIS).expect("regions should be read");
    let smaller = vec![Roi {
        height: rois[0].height / 2,
        ..rois[0]
    }];
    let changes: Vec<(Parameter, ValueType, f64)> = camera
        .parameters()
        .expect("parameters should be listed")
        .into_iter()
        .filter_map(|parameter| {
            let value = other_value(&camera, parameter)?;
            Some((parameter, camera.value_type(parameter).ok()?, value))
        })
        .collect();
    assert!(!changes.is_empty());

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    {
        let during = acquisition.camera();
        let device = during.device().expect("the device should be found");
        // seen with sdk 5.15, on this and two other demo models
        let refused = Some(Error::AcquisitionInProgress);
        let (mut accepted, mut refusals) = (0, 0);
        for (parameter, value_type, value) in changes {
            let result = match value_type {
                ValueType::FloatingPoint => device.set_untyped::<f64>(parameter, value),
                ValueType::LargeInteger => device.set_untyped::<i64>(parameter, value as i64),
                _ => device.set_untyped::<i32>(parameter, value as i32),
            };
            if during.can_set_online(parameter) == Ok(true) {
                assert_eq!(result, Ok(()), "{parameter:?} can be set online");
                accepted += 1;
            } else {
                assert_eq!(result.err(), refused, "{parameter:?} can't be set online");
                refusals += 1;
            }
        }
        // both kinds were tried, exposure time is online-settable here
        assert!(accepted > 0 && refusals > 0);
        assert_eq!(device.set(params::ROIS, smaller).err(), refused);
        assert_eq!(device.commit_parameters().err(), refused);
        assert_eq!(device.restore_parameters_to_default_values().err(), refused);
        assert_eq!(during.commit_parameters_to_device().err(), refused);
        assert_eq!(
            during.refresh_parameter_from_device(Parameter::Rois).err(),
            refused
        );
        assert_eq!(during.refresh_parameters_from_device().err(), refused);
    }

    loop {
        let update = acquisition
            .wait(Some(Duration::from_secs(10)))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        for readout in update.readouts().iter() {
            assert_eq!(readout.bytes().len(), readout_stride);
        }
        if !update.running() {
            break;
        }
    }
    acquisition
        .finish()
        .expect("an ended acquisition should finish cleanly");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
