//! Tests for the errors the `# Errors` sections name, triggered on PICam's
//! demo cameras. For the parameter methods: parameters a camera doesn't
//! have, read-only and out-of-range values, value and constraint types,
//! reading from hardware, waitable statuses, setting values online, and
//! changes during an acquisition. For identifying cameras: no camera
//! connected, IDs that match none, demo cameras, handles whose camera has
//! closed, and names of values PICam doesn't know. For acquisition: a
//! readout that times out, readout counts PICam refuses, uncommitted
//! parameters for a stream, a buffer too big to allocate, and waiting after
//! the end. The comments name the methods whose docs each group of
//! assertions backs, typed and `_untyped` alike.

mod common;

use std::sync::Arc;
use std::time::Duration;

use picam_rs::{
    AdcQuality, Camera, CameraId, ComputerInterface, ConstraintCategory, EmCalibration, Error,
    GatingMode, Library, Model, Parameter, Pulse, SensorTemperatureStatus, params,
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
/// modulation sequence (seen with SDK 5.15).
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
    // and the `_untyped` twins, with a type that passes the crate's own check
    let code = Parameter::GatingMode;
    assert_eq!(camera.get_untyped::<i32>(code).err(), missing);
    assert_eq!(camera.set_untyped::<i32>(code, 0).err(), missing);
    assert_eq!(camera.default_value_untyped::<i32>(code).err(), missing);
    assert_eq!(camera.can_set_untyped::<i32>(code, 0).err(), missing);

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
    assert_eq!(camera.watch_value_untyped::<i32>(code).err(), missing);
    assert_eq!(
        camera.watch_extrinsic_value_untyped::<i32>(code).err(),
        missing
    );
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
    let not_readable = Some(Error::ParameterIsNotReadable);
    assert_eq!(camera.read(gating).err(), not_readable);
    assert_eq!(camera.read_untyped::<i32>(code).err(), not_readable);

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
/// (seen with SDK 5.15).
#[test]
fn read_only_and_out_of_range_values() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0002";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    // `set`, `can_set` and `default_value` and their `_untyped` twins: picam
    // refuses even asking for a read-only parameter's default
    let read_only = Some(Error::ParameterValueIsReadOnly);
    let stride = Parameter::ReadoutStride;
    assert_eq!(camera.set(params::READOUT_STRIDE, 4).err(), read_only);
    assert_eq!(camera.can_set(params::READOUT_STRIDE, 4).err(), read_only);
    assert_eq!(
        camera.default_value(params::READOUT_STRIDE).err(),
        read_only
    );
    assert_eq!(camera.set_untyped::<i32>(stride, 4).err(), read_only);
    assert_eq!(camera.can_set_untyped::<i32>(stride, 4).err(), read_only);
    assert_eq!(camera.default_value_untyped::<i32>(stride).err(), read_only);

    // `set`, outside a range or a collection, while `can_set` only says no
    let invalid = Some(Error::InvalidParameterValue);
    assert_eq!(camera.set(params::EXPOSURE_TIME, -5.0).err(), invalid);
    assert_eq!(
        camera
            .set_untyped::<f64>(Parameter::ExposureTime, -5.0)
            .err(),
        invalid
    );
    assert_eq!(camera.can_set(params::EXPOSURE_TIME, -5.0), Ok(false));
    assert_eq!(
        camera.can_set_untyped::<f64>(Parameter::ExposureTime, -5.0),
        Ok(false)
    );
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
/// unknown constraint category (seen with SDK 5.15).
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
/// category, on an intensifier camera that has both (seen with SDK 5.15).
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
/// (seen with SDK 5.15).
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
    // and through the typed methods, with a value the enum keeps but the
    // purview doesn't have
    let temperature = params::SENSOR_TEMPERATURE_STATUS;
    let unknown_status = SensorTemperatureStatus::Unknown(outside);
    assert_eq!(
        camera
            .estimate_time_to_status_parameter_value(temperature, unknown_status)
            .err(),
        invalid
    );
    assert_eq!(
        camera
            .wait_for_status_parameter_value(temperature, unknown_status, wait)
            .err(),
        invalid
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the errors for setting values online, outside and during an
/// acquisition, and for changes during an acquisition that picam can't make
/// online (seen with SDK 5.15).
#[test]
fn setting_online_and_while_acquiring() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0006";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    // `set_online` and its `_untyped` twin, outside an acquisition (manual
    // 5.4.1.20 to 5.4.1.22)
    let not_acquiring = Some(Error::AcquisitionNotInProgress);
    assert_eq!(camera.can_set_online(params::EXPOSURE_TIME), Ok(true));
    assert_eq!(
        camera.set_online(params::EXPOSURE_TIME, 60.0).err(),
        not_acquiring
    );
    assert_eq!(
        camera
            .set_online_untyped::<f64>(Parameter::ExposureTime, 60.0)
            .err(),
        not_acquiring
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
    // `set_online_untyped`, with a float for an integer, and its other errors
    assert_eq!(
        during
            .set_online_untyped::<f64>(Parameter::ActiveWidth, 1.0)
            .err(),
        Some(Error::ParameterHasInvalidValueType)
    );
    assert_eq!(
        during
            .set_online_untyped::<f64>(Parameter::ExposureTime, -5.0)
            .err(),
        Some(Error::InvalidParameterValue)
    );
    assert_eq!(
        during
            .set_online_untyped::<i32>(Parameter::ReadoutStride, 4)
            .err(),
        Some(Error::ParameterValueIsReadOnly)
    );
    assert_eq!(
        during
            .set_online_untyped::<i32>(Parameter::IntensifierGain, 1)
            .err(),
        Some(Error::ParameterDoesNotExist)
    );
    assert_eq!(
        during
            .set_online_untyped::<i32>(Parameter::ActiveWidth, width)
            .err(),
        Some(Error::ParameterIsNotOnlineable)
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
/// camera, as other values are (seen with SDK 5.15).
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

/// Checks `Camera::open_first` with no camera connected, when none is (seen
/// with SDK 5.15).
#[test]
fn no_camera_connected() {
    let test_library = common::library();
    let library = &test_library.library;
    let available = Camera::available_ids(library).expect("available cameras should be listed");
    // shown with `--nocapture`, in case the lab computer has cameras attached
    println!("available: {available:?}");
    if available.is_empty() {
        assert_eq!(
            Camera::open_first(Arc::clone(library)).err(),
            Some(Error::NoCamerasAvailable)
        );
    }
}

/// Checks the errors for a camera ID that matches no camera PICam knows of
/// (seen with SDK 5.15).
#[test]
fn an_unknown_camera_id() {
    // each test uses its own serial number so demo cameras never collide,
    // and this one is never connected
    const SERIAL_NUMBER: &str = "ERR0009";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId {
        model: Model::PIXIS_100F,
        computer_interface: ComputerInterface::Usb2,
        sensor_name: "none".to_string(),
        serial_number: SERIAL_NUMBER.to_string(),
    };

    // `Camera::open` and `open_device`, `EmCalibration::open`, and the
    // `CameraId` methods
    let unknown = Some(Error::InvalidCameraId);
    assert_eq!(Camera::open(&id, Arc::clone(library)).err(), unknown);
    assert_eq!(Camera::open_device(&id, Arc::clone(library)).err(), unknown);
    assert_eq!(EmCalibration::open(&id, Arc::clone(library)).err(), unknown);
    assert_eq!(id.is_connected(library).err(), unknown);
    assert_eq!(id.is_open_elsewhere(library).err(), unknown);
    assert_eq!(id.is_demo(library).err(), unknown);
    assert_eq!(id.firmware_details(library).err(), unknown);
    assert_eq!(id.disconnect_demo(library).err(), unknown);
}

/// Checks the errors for connecting, opening and disconnecting demo cameras,
/// and for erasing residual charge on one (seen with SDK 5.15).
#[test]
fn demo_cameras() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0010";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    // `connect_demo`: the same pair again, models that aren't demo cameras,
    // and a serial number too long for picam
    assert_eq!(
        CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER).err(),
        Some(Error::DemoAlreadyConnected)
    );
    let invalid_model = Some(Error::InvalidDemoModel);
    assert_eq!(
        CameraId::connect_demo(library, Model::PI_MTE_SERIES, "ERR0010S").err(),
        invalid_model
    );
    assert_eq!(
        CameraId::connect_demo(library, Model::FERGIE_QTH, "ERR0010A").err(),
        invalid_model
    );
    assert_eq!(
        CameraId::connect_demo(library, Model::PIXIS_100F, &"X".repeat(200)).err(),
        Some(Error::InvalidDemoSerialNumber)
    );

    // `Camera::open` and `open_device`, on a camera open already
    let opened = Some(Error::CameraAlreadyOpened);
    assert_eq!(Camera::open(&id, Arc::clone(library)).err(), opened);
    assert_eq!(Camera::open_device(&id, Arc::clone(library)).err(), opened);

    // `erase_residual`, which no demo camera does
    assert_eq!(camera.can_erase_residual(), Ok(false));
    assert_eq!(camera.erase_residual().err(), Some(Error::DemoNotSupported));

    // `disconnect_demo` and `Camera::open`, once it's disconnected
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
    let disconnected = Some(Error::DeviceDisconnected);
    assert_eq!(id.disconnect_demo(library).err(), disconnected);
    assert_eq!(Camera::open(&id, Arc::clone(library)).err(), disconnected);
}

/// Checks that every `CameraHandle` method refuses a handle whose camera has
/// closed, model and device alike (seen with SDK 5.15).
#[test]
fn a_closed_handle() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0011";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let model = Camera::list_open_cameras(library).expect("open cameras should be listed")[0];
    let device =
        Camera::list_open_camera_devices(library).expect("open camera devices should be listed")[0];
    drop(camera);

    let closed = Some(Error::InvalidHandle);
    for handle in [model, device] {
        assert_eq!(handle.handle_type().err(), closed);
        assert_eq!(handle.model().err(), closed);
        assert_eq!(handle.device().err(), closed);
        assert_eq!(handle.is_connected().err(), closed);
        assert_eq!(handle.is_faulted().err(), closed);
        assert_eq!(handle.id().err(), closed);
        assert_eq!(handle.user_state().err(), closed);
    }

    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the `name()` methods for values PICam doesn't know (seen with SDK
/// 5.15).
#[test]
fn names_of_unknown_values() {
    let test_library = common::library();
    let library = &test_library.library;

    let undefined = Some(Error::EnumerationValueNotDefined);
    assert_eq!(Parameter::Unknown(9999).name(library).err(), undefined);
    assert_eq!(AdcQuality::Unknown(99).name(library).err(), undefined);
}

/// Checks the errors for acquiring: a readout slower than its timeout,
/// readout counts PICam refuses, a stream with uncommitted parameters, a
/// buffer too big to allocate, and waiting once the acquisition has ended
/// (seen with SDK 5.15).
#[test]
fn acquisition_errors() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ERR0012";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let wait = Some(Duration::from_secs(5));

    // `acquire`, with a readout slower than its timeout (manual 6.4.1)
    camera
        .set(params::EXPOSURE_TIME, 500.0)
        .expect("exposure time should be set");
    assert_eq!(camera.commit_parameters(), Ok(vec![]));
    assert_eq!(
        camera.acquire(1, Some(Duration::from_millis(1))).err(),
        Some(Error::TimeOutOccurred)
    );
    camera
        .set(params::EXPOSURE_TIME, 1.0)
        .expect("exposure time should be set");
    assert_eq!(camera.commit_parameters(), Ok(vec![]));

    // `acquire`, with a count that isn't positive, or more than picam's own
    // memory holds without a buffer
    let invalid_count = Some(Error::InvalidReadoutCount);
    assert_eq!(camera.acquire(0, wait).err(), invalid_count);
    assert_eq!(camera.acquire(-1, wait).err(), invalid_count);
    let maximum = camera
        .get(params::EXACT_READOUT_COUNT_MAXIMUM)
        .expect("the exact readout count maximum should be read");
    assert_eq!(
        camera.acquire(maximum + 1, wait).err(),
        Some(Error::InvalidAcquisitionBuffer)
    );

    // `start_acquisition_stream`, with a change not yet committed (manual
    // 6.4.2)
    camera
        .set(params::EXPOSURE_TIME, 2.0)
        .expect("exposure time should be set");
    assert_eq!(
        camera.start_acquisition_stream().err(),
        Some(Error::ParametersNotCommitted)
    );
    assert_eq!(camera.commit_parameters(), Ok(vec![]));

    // `set_acquisition_buffer`, more than the os will give
    assert_eq!(
        camera.set_acquisition_buffer(usize::MAX),
        Err(Error::InsufficientMemory)
    );

    // `Acquisition::wait`, once the acquisition has ended
    camera
        .set(params::READOUT_COUNT, 1)
        .expect("readout count should be set");
    assert_eq!(camera.commit_parameters(), Ok(vec![]));
    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    loop {
        let update = acquisition
            .wait(wait)
            .expect("the acquisition should update");
        if update.is_some_and(|update| !update.running()) {
            break;
        }
    }
    assert_eq!(
        acquisition.wait(wait).err(),
        Some(Error::AcquisitionNotInProgress)
    );
    acquisition
        .finish()
        .expect("an acquisition that has ended should finish");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
