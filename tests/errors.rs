//! Tests for the errors the parameter methods' `# Errors` sections name,
//! triggered on PICam's demo cameras: parameters a camera doesn't have,
//! read-only and out-of-range values, value and constraint types, reading
//! from hardware, waitable statuses, setting values online, and changes
//! during an acquisition. Each assertion's comment names the methods whose
//! docs it backs.

mod common;

use std::sync::Arc;
use std::time::Duration;

use picam_rs::{
    AdcQuality, Camera, CameraId, ConstraintCategory, Error, GatingMode, Library, Model, Parameter,
    Pulse, SensorTemperatureStatus, params,
};

/// A category this crate doesn't know, which `ConstraintCategory` keeps.
const UNKNOWN_CATEGORY: ConstraintCategory = ConstraintCategory::Unknown(99);

/// Connects and opens a demo camera.
fn open(library: &Arc<Library>, model: Model, serial_number: &str) -> (CameraId, Camera) {
    let id =
        CameraId::connect_demo(library, model, serial_number).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    (id, camera)
}

/// Checks the errors for parameters a PIXIS 100F doesn't have: an intensifier
/// camera's gating mode, intensifier gain, repetitive gate and custom
/// modulation sequence (seen with sdk 5.15).
#[test]
fn a_missing_parameter() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0001";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let missing = Some(Error::ParameterDoesNotExist);
    let gating = params::GATING_MODE;
    assert_eq!(camera.does_parameter_exist(gating), Ok(false));

    // the value methods
    assert_eq!(camera.get(gating).err(), missing);
    assert_eq!(camera.set(gating, GatingMode::Repetitive).err(), missing);
    assert_eq!(camera.default_value(gating).err(), missing);
    assert_eq!(
        camera.can_set(gating, GatingMode::Repetitive).err(),
        missing
    );
    assert_eq!(camera.device().get(gating).err(), missing);

    // the information methods
    assert_eq!(camera.can_set_online(gating).err(), missing);
    assert_eq!(camera.can_read(gating).err(), missing);
    assert_eq!(camera.can_wait_for_status(gating).err(), missing);
    assert_eq!(camera.is_relevant(gating).err(), missing);
    assert_eq!(camera.value_type(gating).err(), missing);
    assert_eq!(camera.enumerated_type(gating).err(), missing);
    assert_eq!(camera.value_access(gating).err(), missing);
    assert_eq!(camera.constraint_type(gating).err(), missing);
    assert_eq!(camera.dynamics(gating).err(), missing);
    assert_eq!(camera.extrinsic_dynamics(gating).err(), missing);

    // the constraint getters, each with a parameter of its own kind
    let capable = ConstraintCategory::Capable;
    let gain = params::INTENSIFIER_GAIN;
    let gate = params::REPETITIVE_GATE;
    let sequence = params::CUSTOM_MODULATION_SEQUENCE;
    assert_eq!(camera.collection_constraint(gating, capable).err(), missing);
    assert_eq!(camera.collection_constraints(gating).err(), missing);
    assert_eq!(camera.range_constraint(gain, capable).err(), missing);
    assert_eq!(camera.range_constraints(gain).err(), missing);
    assert_eq!(camera.pulse_constraint(gate, capable).err(), missing);
    assert_eq!(camera.pulse_constraints(gate).err(), missing);
    assert_eq!(
        camera.modulations_constraint(sequence, capable).err(),
        missing
    );
    assert_eq!(camera.modulations_constraints(sequence).err(), missing);

    // the watches
    assert_eq!(camera.watch_value(gating).err(), missing);
    assert_eq!(camera.watch_extrinsic_value(gating).err(), missing);
    assert_eq!(camera.watch_relevance(gating).err(), missing);
    assert_eq!(camera.watch_value_access(gating).err(), missing);
    assert_eq!(camera.watch_collection_constraint(gating).err(), missing);
    assert_eq!(camera.watch_range_constraint(gain).err(), missing);
    assert_eq!(camera.watch_pulse_constraint(gate).err(), missing);
    assert_eq!(camera.watch_modulations_constraint(sequence).err(), missing);

    // validation and refreshing from the device
    assert_eq!(camera.validate_parameter(gating).err(), missing);
    assert_eq!(camera.validate_dependent_parameter(gating).err(), missing);
    assert_eq!(camera.refresh_parameter_from_device(gating).err(), missing);

    // reading from hardware is refused before picam looks for the parameter
    assert_eq!(
        camera.read(gating).err(),
        Some(Error::ParameterIsNotReadable)
    );

    // and so are the status methods, since it isn't a waitable status
    let not_waitable = Some(Error::ParameterIsNotWaitableStatus);
    assert_eq!(camera.status_parameter_purview(gating).err(), not_waitable);
    assert_eq!(
        camera
            .estimate_time_to_status_parameter_value(gating, GatingMode::Repetitive)
            .err(),
        not_waitable
    );
    assert_eq!(
        camera
            .wait_for_status_parameter_value(
                gating,
                GatingMode::Repetitive,
                Some(Duration::from_millis(10))
            )
            .err(),
        not_waitable
    );
    assert_eq!(
        camera
            .notify_when_status(gating, GatingMode::Repetitive)
            .err(),
        not_waitable
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the errors for a read-only parameter and for values out of range
/// (seen with sdk 5.15).
#[test]
fn read_only_and_out_of_range_values() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0002";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    // `set`, `can_set` and `default_value`: picam refuses even asking for a
    // read-only parameter's default
    let read_only = Some(Error::ParameterValueIsReadOnly);
    assert_eq!(camera.set(params::READOUT_STRIDE, 4).err(), read_only);
    assert_eq!(camera.can_set(params::READOUT_STRIDE, 4).err(), read_only);
    assert_eq!(
        camera.default_value(params::READOUT_STRIDE).err(),
        read_only
    );

    // `set`, outside a range or a collection, while `can_set` only says no
    let invalid = Some(Error::InvalidParameterValue);
    assert_eq!(camera.set(params::EXPOSURE_TIME, -5.0).err(), invalid);
    assert_eq!(camera.can_set(params::EXPOSURE_TIME, -5.0), Ok(false));
    let unknown_quality = AdcQuality::Unknown(99);
    assert_eq!(
        camera.set(params::ADC_QUALITY, unknown_quality).err(),
        invalid
    );
    assert_eq!(
        camera.can_set(params::ADC_QUALITY, unknown_quality),
        Ok(false)
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the errors for a value or constraint of the wrong type, and an
/// unknown constraint category (seen with sdk 5.15).
#[test]
fn value_and_constraint_types() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0003";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let quality = Parameter::AdcQuality;
    let exposure = params::EXPOSURE_TIME;

    // the `_untyped` value methods and watches, with a float for an enum
    let wrong_type = Some(Error::ParameterHasInvalidValueType);
    assert_eq!(camera.get_untyped::<f64>(quality).err(), wrong_type);
    assert_eq!(camera.set_untyped::<f64>(quality, 1.0).err(), wrong_type);
    assert_eq!(
        camera.default_value_untyped::<f64>(quality).err(),
        wrong_type
    );
    assert_eq!(
        camera.can_set_untyped::<f64>(quality, 1.0).err(),
        wrong_type
    );
    assert_eq!(camera.read_untyped::<f64>(quality).err(), wrong_type);
    assert_eq!(camera.watch_value_untyped::<f64>(quality).err(), wrong_type);
    assert_eq!(
        camera.watch_extrinsic_value_untyped::<f64>(quality).err(),
        wrong_type
    );
    // `enumerated_type`, for a parameter that isn't an enum
    assert_eq!(camera.enumerated_type(exposure).err(), wrong_type);

    // the constraint getters, plurals and watches, on a range-constrained
    // parameter
    let capable = ConstraintCategory::Capable;
    let wrong_constraint = Some(Error::ParameterHasInvalidConstraintType);
    assert_eq!(
        camera.collection_constraint(exposure, capable).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.collection_constraints(exposure).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.rois_constraint(exposure, capable).err(),
        wrong_constraint
    );
    assert_eq!(camera.rois_constraints(exposure).err(), wrong_constraint);
    assert_eq!(
        camera.pulse_constraint(exposure, capable).err(),
        wrong_constraint
    );
    assert_eq!(camera.pulse_constraints(exposure).err(), wrong_constraint);
    assert_eq!(
        camera.modulations_constraint(exposure, capable).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.modulations_constraints(exposure).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.watch_collection_constraint(exposure).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.watch_rois_constraint(exposure).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.watch_pulse_constraint(exposure).err(),
        wrong_constraint
    );
    assert_eq!(
        camera.watch_modulations_constraint(exposure).err(),
        wrong_constraint
    );
    // and on a collection-constrained one
    assert_eq!(
        camera.range_constraint(quality, capable).err(),
        wrong_constraint
    );
    assert_eq!(camera.range_constraints(quality).err(), wrong_constraint);
    assert_eq!(
        camera.watch_range_constraint(quality).err(),
        wrong_constraint
    );

    // the constraint getters, with a category picam doesn't know
    let unknown = Some(Error::InvalidConstraintCategory);
    assert_eq!(
        camera
            .collection_constraint(quality, UNKNOWN_CATEGORY)
            .err(),
        unknown
    );
    assert_eq!(
        camera.range_constraint(exposure, UNKNOWN_CATEGORY).err(),
        unknown
    );
    assert_eq!(
        camera.rois_constraint(params::ROIS, UNKNOWN_CATEGORY).err(),
        unknown
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the pulse and modulation constraints' errors for an unknown
/// category, on an intensifier camera that has both (seen with sdk 5.15).
#[test]
fn pulse_and_modulation_categories() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0004";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PI_MAX4_1024IRF, SERIAL_NUMBER);

    let unknown = Some(Error::InvalidConstraintCategory);
    assert_eq!(
        camera
            .pulse_constraint(params::REPETITIVE_GATE, UNKNOWN_CATEGORY)
            .err(),
        unknown
    );
    assert_eq!(
        camera
            .modulations_constraint(params::CUSTOM_MODULATION_SEQUENCE, UNKNOWN_CATEGORY)
            .err(),
        unknown
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the errors for reading a parameter the hardware can't report, and
/// for statuses that can't be waited on or values outside a status's purview
/// (seen with sdk 5.15).
#[test]
fn reading_and_waiting() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0005";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    // `read`, for a parameter only picam's model holds
    assert_eq!(camera.can_read(params::EXPOSURE_TIME), Ok(false));
    assert_eq!(
        camera.read(params::EXPOSURE_TIME).err(),
        Some(Error::ParameterIsNotReadable)
    );

    // the status methods, for an enum parameter that isn't a waitable status
    let quality = params::ADC_QUALITY;
    let low_noise = AdcQuality::LowNoise;
    let wait = Some(Duration::from_millis(10));
    let not_waitable = Some(Error::ParameterIsNotWaitableStatus);
    assert_eq!(camera.status_parameter_purview(quality).err(), not_waitable);
    assert_eq!(
        camera
            .estimate_time_to_status_parameter_value(quality, low_noise)
            .err(),
        not_waitable
    );
    assert_eq!(
        camera
            .wait_for_status_parameter_value(quality, low_noise, wait)
            .err(),
        not_waitable
    );
    assert_eq!(
        camera.notify_when_status(quality, low_noise).err(),
        not_waitable
    );

    // the untyped ones, for a parameter whose values aren't integers, which
    // picam checks first
    let exposure = Parameter::ExposureTime;
    let wrong_type = Some(Error::ParameterHasInvalidValueType);
    assert_eq!(
        camera
            .estimate_time_to_status_parameter_value_untyped(exposure, 1)
            .err(),
        wrong_type
    );
    assert_eq!(
        camera
            .wait_for_status_parameter_value_untyped(exposure, 1, wait)
            .err(),
        wrong_type
    );
    assert_eq!(
        camera.notify_when_status_untyped(exposure, 1).err(),
        wrong_type
    );

    // the estimate and the wait, for a value outside the status's purview
    let status = Parameter::SensorTemperatureStatus;
    let outside = 99;
    let invalid = Some(Error::InvalidParameterValue);
    assert_eq!(
        camera
            .estimate_time_to_status_parameter_value_untyped(status, outside)
            .err(),
        invalid
    );
    assert_eq!(
        camera
            .wait_for_status_parameter_value_untyped(status, outside, wait)
            .err(),
        invalid
    );
    // the typed status values are all in the purview
    assert_eq!(
        camera.wait_for_status_parameter_value(
            params::SENSOR_TEMPERATURE_STATUS,
            SensorTemperatureStatus::Locked,
            wait
        ),
        Ok(())
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the errors for setting values online, outside and during an
/// acquisition, and for changes during an acquisition that picam can't make
/// online (seen with sdk 5.15).
#[test]
fn setting_online_and_while_acquiring() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0006";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    // `set_online`, outside an acquisition (manual 5.4.1.20)
    assert_eq!(camera.can_set_online(params::EXPOSURE_TIME), Ok(true));
    assert_eq!(
        camera.set_online(params::EXPOSURE_TIME, 60.0).err(),
        Some(Error::AcquisitionNotInProgress)
    );

    // long enough that the acquisition runs throughout
    camera
        .set(params::READOUT_COUNT, 1000)
        .expect("readout count should be set");
    camera
        .set(params::EXPOSURE_TIME, 50.0)
        .expect("exposure time should be set");
    assert_eq!(camera.commit_parameters(), Ok(vec![]));
    let acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    let during = acquisition.camera();

    // `set_online`, during it
    assert_eq!(during.set_online(params::EXPOSURE_TIME, 60.0), Ok(()));
    assert_eq!(
        during.set_online(params::EXPOSURE_TIME, -5.0).err(),
        Some(Error::InvalidParameterValue)
    );
    assert_eq!(
        during.set_online(params::READOUT_STRIDE, 4).err(),
        Some(Error::ParameterValueIsReadOnly)
    );
    assert_eq!(
        during.set_online(params::INTENSIFIER_GAIN, 1).err(),
        Some(Error::ParameterDoesNotExist)
    );
    assert_eq!(during.can_set_online(params::ACTIVE_WIDTH), Ok(false));
    let width = during
        .get(params::ACTIVE_WIDTH)
        .expect("active width should be read");
    assert_eq!(
        during.set_online(params::ACTIVE_WIDTH, width).err(),
        Some(Error::ParameterIsNotOnlineable)
    );
    // `set_online_untyped`, with a float for an integer
    assert_eq!(
        during
            .set_online_untyped::<f64>(Parameter::ActiveWidth, 1.0)
            .err(),
        Some(Error::ParameterHasInvalidValueType)
    );

    // `set` and `set_untyped`, for a parameter that can't be set online, while
    // one that can is still set
    let in_progress = Some(Error::AcquisitionInProgress);
    assert_eq!(
        during.set(params::ADC_QUALITY, AdcQuality::LowNoise).err(),
        in_progress
    );
    assert_eq!(
        during.set_untyped::<i32>(Parameter::AdcQuality, 1).err(),
        in_progress
    );
    assert_eq!(during.set(params::EXPOSURE_TIME, 70.0), Ok(()));
    // committing, restoring, and moving values between the model and device
    assert_eq!(during.commit_parameters().err(), in_progress);
    assert_eq!(
        during.restore_parameters_to_default_values().err(),
        in_progress
    );
    assert_eq!(during.commit_parameters_to_device().err(), in_progress);
    assert_eq!(
        during
            .refresh_parameter_from_device(Parameter::AdcQuality)
            .err(),
        in_progress
    );
    assert_eq!(during.refresh_parameters_from_device().err(), in_progress);

    acquisition.finish().expect("acquisition should finish");
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a pulse value out of range is refused on an intensifier
/// camera, as other values are (seen with sdk 5.15).
#[test]
fn a_pulse_out_of_range() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0007";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PI_MAX4_1024I, SERIAL_NUMBER);

    let backwards = Pulse {
        delay: -5.0,
        width: -5.0,
    };
    assert_eq!(
        camera.set(params::REPETITIVE_GATE, backwards).err(),
        Some(Error::InvalidParameterValue)
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
