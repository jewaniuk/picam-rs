//! Tests for getting and setting parameter values through `Camera`, which
//! go through each value type's sealed implementation, and for querying
//! parameter information and constraints, using PICam's demo cameras so no
//! hardware is needed.

mod common;

use std::sync::Arc;
use std::time::Duration;

use picam_rs::{
    AdcQuality, Camera, CameraId, ConstraintCategory, ConstraintScope, ConstraintSeverity,
    ConstraintType, Error, Library, Model, Parameter, Roi, SensorTemperatureStatus,
    ShutterTimingMode, ValueAccess, ValueType, params,
};

/// Connects a demo camera of the first model PICam offers.
fn connect_demo(library: &Library, serial_number: &str) -> CameraId {
    let models = Camera::available_demo_models(library).expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");
    CameraId::connect_demo(library, model, serial_number).expect("demo camera should connect")
}

/// Gets, checks and sets integer and floating-point values, and commits the
/// result.
#[test]
fn scalar_values_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0001";

    let test_library = common::library();
    let id = connect_demo(&test_library.library, SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    // the untyped fallback, with the type chosen here rather than by a handle
    let width: i32 = camera
        .get_untyped(Parameter::SensorActiveWidth)
        .expect("sensor width should be read");
    assert!(width > 0, "sensor width should be positive, got {width}");

    let exposure_time: f64 = camera
        .get(params::EXPOSURE_TIME)
        .expect("exposure time should be read");
    let default_exposure_time: f64 = camera
        .default_value(params::EXPOSURE_TIME)
        .expect("default exposure time should be read");
    assert!(
        default_exposure_time.is_finite() && default_exposure_time >= 0.0,
        "default exposure time should be a non-negative number, got {default_exposure_time}"
    );

    // a freshly opened demo camera accepts its own current value, seen with
    // sdk 5.15 but not documented
    assert!(
        camera
            .can_set(params::EXPOSURE_TIME, exposure_time)
            .expect("settability should be checked"),
        "current exposure time {exposure_time} should be settable"
    );
    camera
        .set(params::EXPOSURE_TIME, exposure_time)
        .expect("exposure time should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "no parameters should fail, got {failed:?}"
    );

    drop(camera);
    id.disconnect_demo(&test_library.library)
        .expect("demo camera should disconnect");
}

/// Gets, checks and sets the regions of interest, commits them and reads
/// them back.
#[test]
fn rois_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0002";

    let test_library = common::library();
    let id = connect_demo(&test_library.library, SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    let rois: Vec<Roi> = camera.get(params::ROIS).expect("rois should be read");
    assert!(!rois.is_empty(), "a camera should have at least one roi");

    // a freshly opened demo camera accepts its own current value, seen with
    // sdk 5.15 but not documented
    assert!(
        camera
            .can_set(params::ROIS, rois.clone())
            .expect("settability should be checked"),
        "current rois {rois:?} should be settable"
    );
    camera
        .set(params::ROIS, rois.clone())
        .expect("rois should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "no parameters should fail, got {failed:?}"
    );

    let committed: Vec<Roi> = camera.get(params::ROIS).expect("rois should be read back");
    assert_eq!(committed, rois);

    drop(camera);
    id.disconnect_demo(&test_library.library)
        .expect("demo camera should disconnect");
}

/// Queries value types, constraint types, access and constraints, which all
/// come back as wrapper enums rather than raw PICam values.
#[test]
fn parameter_info_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0003";

    let test_library = common::library();
    let id = connect_demo(&test_library.library, SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    // value and constraint types from the PICam manual, appendix A tables A-2
    // and A-3. for rois, a-3 gives a region of interest constraint while the
    // text of a-2 gives range, which looks like a slip in the manual
    assert_eq!(
        camera
            .value_type(params::EXPOSURE_TIME)
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
            .constraint_type(params::EXPOSURE_TIME)
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
        .range_constraint(params::EXPOSURE_TIME, ConstraintCategory::Capable)
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
    id.disconnect_demo(&test_library.library)
        .expect("demo camera should disconnect");
}

/// Flips an on/off parameter, commits it and reads it back, which goes
/// through `bool`'s get, can-set and set.
#[test]
fn bool_values_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0004";

    let test_library = common::library();
    let id = connect_demo(&test_library.library, SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    // which on/off parameters a demo camera has, and which can be flipped,
    // isn't documented, so take the first one that can
    let candidates = [
        params::CORRECT_PIXEL_BIAS,
        params::INVERT_OUTPUT_SIGNAL,
        params::ANTICIPATE_TRIGGER,
        params::NORMALIZE_ORIENTATION,
        params::DISABLE_DATA_FORMATTING,
        params::TRACK_FRAMES,
        params::CORRECT_FOR_DARK_CURRENT,
        params::DESPECKLE,
        params::CLEAN_SERIAL_REGISTER,
        params::CLEAN_BEFORE_EXPOSURE,
        params::CLEAN_UNTIL_TRIGGER,
        params::DISABLE_COOLING_FAN,
    ];
    let flippable = candidates.into_iter().find_map(|handle| {
        if !camera.does_parameter_exist(handle).ok()? {
            return None;
        }
        let current = camera.get(handle).ok()?;
        camera
            .can_set(handle, !current)
            .ok()?
            .then_some((handle, current))
    });
    let (handle, current) =
        flippable.expect("the demo camera should have an on/off parameter that can be flipped");

    camera
        .set(handle, !current)
        .expect("on/off parameter should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "no parameters should fail, got {failed:?}"
    );
    assert_eq!(
        camera
            .get(handle)
            .expect("on/off parameter should be read back"),
        !current,
        "{handle:?} should have flipped"
    );

    drop(camera);
    id.disconnect_demo(&test_library.library)
        .expect("demo camera should disconnect");
}

/// Changes an enum parameter, commits it and reads it back, which goes
/// through an enum's get, can-set and set.
#[test]
fn enum_values_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0005";

    let test_library = common::library();
    let id = connect_demo(&test_library.library, SERIAL_NUMBER);
    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");

    let current = camera
        .get(params::SHUTTER_TIMING_MODE)
        .expect("shutter timing mode should be read");
    // which modes a demo camera accepts isn't documented, so take the first
    // other one it does
    let other = [
        ShutterTimingMode::Normal,
        ShutterTimingMode::AlwaysClosed,
        ShutterTimingMode::AlwaysOpen,
        ShutterTimingMode::OpenBeforeTrigger,
    ]
    .into_iter()
    .filter(|&mode| mode != current)
    .find(|&mode| {
        camera
            .can_set(params::SHUTTER_TIMING_MODE, mode)
            .unwrap_or(false)
    })
    .expect("the demo camera should accept another shutter timing mode");

    camera
        .set(params::SHUTTER_TIMING_MODE, other)
        .expect("shutter timing mode should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "no parameters should fail, got {failed:?}"
    );
    assert_eq!(
        camera
            .get(params::SHUTTER_TIMING_MODE)
            .expect("shutter timing mode should be read back"),
        other
    );

    drop(camera);
    id.disconnect_demo(&test_library.library)
        .expect("demo camera should disconnect");
}

/// Lists, estimates and waits for a status through the typed status methods
/// and their untyped twins. The demo's sensor temperature is always locked
/// (seen with sdk 5.15).
#[test]
fn status_values_through_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "PARAM0006";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    let status = params::SENSOR_TEMPERATURE_STATUS;
    assert_eq!(camera.can_wait_for_status(status), Ok(true));
    let locked: i32 = camera
        .get_untyped(Parameter::SensorTemperatureStatus)
        .expect("the temperature status should be read");

    let purview = camera
        .status_parameter_purview(status)
        .expect("the purview should be listed");
    let codes = camera
        .status_parameter_purview_untyped(Parameter::SensorTemperatureStatus)
        .expect("the purview codes should be listed");
    assert_eq!(purview.len(), codes.len());
    assert!(codes.contains(&locked));
    assert!(purview.contains(&SensorTemperatureStatus::Locked));
    assert!(purview.contains(&SensorTemperatureStatus::Unlocked));

    // already locked, and the demo can't say when it would unlock (seen with
    // sdk 5.15)
    assert_eq!(
        camera.estimate_time_to_status_parameter_value(status, SensorTemperatureStatus::Locked),
        Ok(Some(Duration::ZERO))
    );
    assert_eq!(
        camera.estimate_time_to_status_parameter_value(status, SensorTemperatureStatus::Unlocked),
        Ok(None)
    );

    assert_eq!(
        camera.wait_for_status_parameter_value(
            status,
            SensorTemperatureStatus::Locked,
            Some(Duration::from_secs(1))
        ),
        Ok(())
    );
    assert_eq!(
        camera.wait_for_status_parameter_value(
            status,
            SensorTemperatureStatus::Unlocked,
            Some(Duration::from_millis(100))
        ),
        Err(Error::TimeOutOccurred)
    );
    assert_eq!(
        camera.wait_for_status_parameter_value_untyped(
            Parameter::SensorTemperatureStatus,
            locked,
            Some(Duration::from_secs(1))
        ),
        Ok(())
    );

    // an enum parameter that isn't a waitable status compiles, and picam
    // refuses it, as it does a parameter that isn't an enum
    assert_eq!(
        camera.wait_for_status_parameter_value(
            params::ADC_QUALITY,
            AdcQuality::LowNoise,
            Some(Duration::from_millis(100))
        ),
        Err(Error::ParameterIsNotWaitableStatus)
    );
    assert_eq!(
        camera.wait_for_status_parameter_value_untyped(
            Parameter::ExposureTime,
            0,
            Some(Duration::from_millis(100))
        ),
        Err(Error::ParameterHasInvalidValueType)
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
