//! Tests for getting and setting parameter values through `Camera`, which
//! go through each value type's sealed implementation, and for querying
//! parameter information and constraints, using PICam's demo cameras so no
//! hardware is needed.

mod common;

use std::sync::Arc;

use picam_rs::{
    Camera, CameraId, ConstraintCategory, ConstraintScope, ConstraintSeverity, ConstraintType,
    Parameter, Roi, ValueAccess, ValueType,
};

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
    let default_exposure_time: f64 = camera
        .default_value(Parameter::ExposureTime)
        .expect("default exposure time should be read");
    assert!(
        default_exposure_time.is_finite() && default_exposure_time >= 0.0,
        "default exposure time should be a non-negative number, got {default_exposure_time}"
    );

    // a freshly opened demo camera accepts its own current value, seen with
    // sdk 5.15 but not documented
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

    // a freshly opened demo camera accepts its own current value, seen with
    // sdk 5.15 but not documented
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

/// Queries value types, constraint types, access and constraints, which all
/// come back as wrapper enums rather than raw PICam values.
#[test]
fn parameter_info_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0003";

    let test_library = common::library();
    let id = connect_demo(SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    // value and constraint types from the PICam manual, appendix A table A-2
    assert_eq!(
        camera
            .value_type(Parameter::ExposureTime)
            .expect("value type should be queried"),
        ValueType::FloatingPoint
    );
    assert_eq!(
        camera
            .value_type(Parameter::Rois)
            .expect("value type should be queried"),
        ValueType::Rois
    );
    assert_eq!(
        camera
            .constraint_type(Parameter::ExposureTime)
            .expect("constraint type should be queried"),
        ConstraintType::Range
    );
    assert_eq!(
        camera
            .constraint_type(Parameter::Rois)
            .expect("constraint type should be queried"),
        ConstraintType::Rois
    );

    // the manual doesn't settle the exact access level, only that it's a
    // value picam defines
    let access = camera
        .value_access(Parameter::ExposureTime)
        .expect("value access should be queried");
    assert!(
        !matches!(access, ValueAccess::Unknown(_)),
        "value access should be a known value, got {access:?}"
    );

    let range = camera
        .range_constraint(Parameter::ExposureTime, ConstraintCategory::Capable)
        .expect("exposure time range constraint should be read");
    assert!(
        !matches!(range.scope, ConstraintScope::Unknown(_)),
        "scope should be a known value, got {:?}",
        range.scope
    );
    assert!(
        !matches!(range.severity, ConstraintSeverity::Unknown(_)),
        "severity should be a known value, got {:?}",
        range.severity
    );

    let rois = camera
        .rois_constraint(Parameter::Rois, ConstraintCategory::Capable)
        .expect("rois constraint should be read");
    assert!(
        !matches!(rois.scope, ConstraintScope::Unknown(_)),
        "scope should be a known value, got {:?}",
        rois.scope
    );
    assert!(
        !matches!(rois.severity, ConstraintSeverity::Unknown(_)),
        "severity should be a known value, got {:?}",
        rois.severity
    );

    drop(camera);
    id.disconnect_demo().expect("demo camera should disconnect");
}
