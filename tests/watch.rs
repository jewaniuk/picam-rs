//! Tests for parameter watches (`watch_value`, `watch_extrinsic_value`,
//! `watch_relevance`, `watch_value_access` and their `_untyped` twins, the
//! dependent constraint watches, `notify_when_status` and `Watch`), using
//! demo cameras.

mod common;

use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::time::Duration;

use picam_rs::{
    Camera, CameraId, ConstraintCategory, Error, GatingMode, Library, Model, Modulation, Parameter,
    Pulse, ReadoutControlMode, Roi, ShutterTimingMode, params,
};

/// Connects and opens a demo camera of `model`.
fn open(library: &Arc<Library>, model: Model, serial_number: &str) -> (CameraId, Camera) {
    let id =
        CameraId::connect_demo(library, model, serial_number).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    (id, camera)
}

/// Checks that each value type's watch sends the value set, before `set`
/// returns (PICam manual 7.4.9.1).
#[test]
fn value_watches_send_each_set_value() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0001";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    let (exposure_watch, exposures) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched");
    camera
        .set(params::EXPOSURE_TIME, 50.0)
        .expect("exposure time should be set");
    assert_eq!(exposures.try_recv(), Ok(50.0));
    // once per set, not once per registration
    assert_eq!(exposures.try_recv(), Err(TryRecvError::Empty));

    let (count_watch, counts) = camera
        .watch_value(params::READOUT_COUNT)
        .expect("readout count should be watched");
    camera
        .set(params::READOUT_COUNT, 5)
        .expect("readout count should be set");
    assert_eq!(counts.try_recv(), Ok(5));

    let current = camera
        .get_untyped::<i32>(Parameter::AdcQuality)
        .expect("adc quality should be read");
    let other = camera
        .collection_constraint(Parameter::AdcQuality, ConstraintCategory::Capable)
        .expect("adc quality's constraint should be read")
        .values
        .into_iter()
        .map(|value| value as i32)
        .find(|&code| code != current)
        .expect("the demo camera should allow another adc quality");
    let (quality_watch, qualities) = camera
        .watch_value(params::ADC_QUALITY)
        .expect("adc quality should be watched");
    camera
        .set_untyped::<i32>(Parameter::AdcQuality, other)
        .expect("adc quality should be set");
    let quality = qualities.try_recv().expect("adc quality should be sent");
    assert_eq!(camera.get(params::ADC_QUALITY), Ok(quality));
    assert_eq!(camera.get_untyped::<i32>(Parameter::AdcQuality), Ok(other));

    let (track_watch, tracks) = camera
        .watch_value(params::TRACK_FRAMES)
        .expect("frame tracking should be watched");
    camera
        .set(params::TRACK_FRAMES, true)
        .expect("frame tracking should be set");
    assert_eq!(tracks.try_recv(), Ok(true));

    let rois = camera.get(params::ROIS).expect("regions should be read");
    let smaller = vec![Roi {
        height: rois[0].height / 2,
        ..rois[0]
    }];
    let (rois_watch, regions) = camera
        .watch_value(params::ROIS)
        .expect("regions should be watched");
    camera
        .set(params::ROIS, smaller.clone())
        .expect("regions should be set");
    assert_eq!(regions.try_recv(), Ok(smaller));

    drop((
        exposure_watch,
        count_watch,
        quality_watch,
        track_watch,
        rois_watch,
    ));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the pulse and modulation watches, on the intensified models that
/// have such parameters.
#[test]
fn pulse_and_modulation_watches() {
    let test_library = common::library();
    let library = &test_library.library;

    // each camera uses its own serial number so demo cameras never collide
    let (id, camera) = open(library, Model::PI_MAX3_1024I, "WATCH0002");
    let pulse = camera
        .get(params::AUX_OUTPUT)
        .expect("the aux output should be read");
    let wider = Pulse {
        width: pulse.width * 2.0,
        ..pulse
    };
    let (pulse_watch, pulses) = camera
        .watch_value(params::AUX_OUTPUT)
        .expect("the aux output should be watched");
    camera
        .set(params::AUX_OUTPUT, wider)
        .expect("the aux output should be set");
    assert_eq!(pulses.try_recv(), Ok(wider));
    drop(pulse_watch);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");

    let (id, camera) = open(library, Model::PI_MAX4_1024IRF, "WATCH0003");
    let mut sequence = camera
        .get(params::CUSTOM_MODULATION_SEQUENCE)
        .expect("the modulation sequence should be read");
    let first: Modulation = sequence[0];
    sequence.push(first);
    let (sequence_watch, sequences) = camera
        .watch_value(params::CUSTOM_MODULATION_SEQUENCE)
        .expect("the modulation sequence should be watched");
    camera
        .set(params::CUSTOM_MODULATION_SEQUENCE, sequence.clone())
        .expect("the modulation sequence should be set");
    assert_eq!(sequences.try_recv(), Ok(sequence));
    drop(sequence_watch);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that two watches on one parameter both get each value, and that
/// dropping one leaves the other working.
#[test]
fn watches_share_a_registration() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0004";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    let (first_watch, first) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched");
    let (second_watch, second) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched twice");
    camera
        .set(params::EXPOSURE_TIME, 30.0)
        .expect("exposure time should be set");
    assert_eq!(first.try_recv(), Ok(30.0));
    assert_eq!(second.try_recv(), Ok(30.0));

    drop(first_watch);
    camera
        .set(params::EXPOSURE_TIME, 40.0)
        .expect("exposure time should be set");
    assert_eq!(first.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(second.try_recv(), Ok(40.0));

    drop(second_watch);
    camera
        .set(params::EXPOSURE_TIME, 60.0)
        .expect("exposure time should be set");
    assert_eq!(second.try_recv(), Err(TryRecvError::Disconnected));

    // the last drop unregistered with picam, so registering again gives
    // each value once, not twice
    let (again_watch, again) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched again");
    camera
        .set(params::EXPOSURE_TIME, 70.0)
        .expect("exposure time should be set");
    assert_eq!(again.try_recv(), Ok(70.0));
    assert_eq!(again.try_recv(), Err(TryRecvError::Empty));
    drop(again_watch);

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a relevance watch sends the change another parameter's value
/// causes: kinetics readout makes the kinetics window height relevant and the
/// active width irrelevant (seen with sdk 5.15).
#[test]
fn relevance_is_sent_when_another_parameter_changes_it() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0005";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    assert_eq!(
        camera.is_relevant(Parameter::KineticsWindowHeight),
        Ok(false)
    );
    let (relevance_watch, relevance) = camera
        .watch_relevance(Parameter::KineticsWindowHeight)
        .expect("relevance should be watched");
    let (width_watch, width_relevance) = camera
        .watch_relevance(Parameter::ActiveWidth)
        .expect("relevance should be watched");
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
        .expect("kinetics readout should be set");
    assert_eq!(relevance.try_recv(), Ok(true));
    assert_eq!(width_relevance.try_recv(), Ok(false));

    drop((relevance_watch, width_watch));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that value access watches register. Changing the readout mode
/// changes no parameter's access on this demo model (seen with sdk 5.15),
/// so nothing is sent.
#[test]
fn value_access_watch_registers() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0006";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    let (access_watch, access) = camera
        .watch_value_access(Parameter::KineticsWindowHeight)
        .expect("value access should be watched");
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
        .expect("kinetics readout should be set");
    assert_eq!(access.try_recv(), Err(TryRecvError::Empty));

    drop(access_watch);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that extrinsic watches register through the camera, on the device
/// handle it keeps, and through the device itself, sharing one registration
/// for the same parameter. The demo camera sends no readings (seen with sdk
/// 5.15).
#[test]
fn extrinsic_watch_registers() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0007";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    let (camera_watch, from_camera) = camera
        .watch_extrinsic_value(params::SENSOR_TEMPERATURE_READING)
        .expect("the temperature reading should be watched");
    let device = camera.device().expect("the device should be found");
    let (status_watch, statuses) = device
        .watch_extrinsic_value(params::SENSOR_TEMPERATURE_STATUS)
        .expect("the temperature status should be watched");
    // the same key as the camera's watch, since the camera keeps this device's handle
    let (device_watch, from_device) = device
        .watch_extrinsic_value(params::SENSOR_TEMPERATURE_READING)
        .expect("the temperature reading should be watched through the device");
    let wait = Duration::from_millis(500);
    assert_eq!(
        from_camera.recv_timeout(wait),
        Err(RecvTimeoutError::Timeout)
    );
    assert_eq!(statuses.recv_timeout(wait), Err(RecvTimeoutError::Timeout));
    assert_eq!(
        from_device.recv_timeout(wait),
        Err(RecvTimeoutError::Timeout)
    );

    // dropping one leaves the other's channel open
    drop(camera_watch);
    assert_eq!(from_camera.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(from_device.try_recv(), Err(TryRecvError::Empty));
    drop((device_watch, status_watch));
    assert_eq!(from_device.try_recv(), Err(TryRecvError::Disconnected));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a watch through the device gets the values set through the
/// device, which PICam reports with the device's handle.
#[test]
fn device_value_watch() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0010";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let device = camera.device().expect("the device should be found");

    let (exposure_watch, exposures) = device
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched through the device");
    device
        .set(params::EXPOSURE_TIME, 35.0)
        .expect("exposure time should be set through the device");
    assert_eq!(exposures.try_recv(), Ok(35.0));
    assert_eq!(exposures.try_recv(), Err(TryRecvError::Empty));

    drop(exposure_watch);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a receiver can be read on another thread.
#[test]
fn receiver_moves_to_another_thread() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0008";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    let (exposure_watch, exposures) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched");
    std::thread::scope(|scope| {
        let reader = scope.spawn(move || exposures.recv_timeout(Duration::from_secs(10)));
        camera
            .set(params::EXPOSURE_TIME, 25.0)
            .expect("exposure time should be set");
        assert_eq!(reader.join().expect("the reader shouldn't panic"), Ok(25.0));
    });

    drop(exposure_watch);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks the errors for a wrong enum and a parameter the camera doesn't
/// have.
#[test]
fn watch_errors() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0009";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    assert!(matches!(
        camera.watch_value_untyped::<ShutterTimingMode>(Parameter::AdcQuality),
        Err(Error::EnumeratedTypeMismatch { .. })
    ));
    // seen with sdk 5.15
    assert!(matches!(
        camera.watch_value(params::AUX_OUTPUT),
        Err(Error::ParameterDoesNotExist)
    ));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a watch can outlive its camera: closing the camera clears it,
/// so its receiver disconnects and dropping it afterwards is harmless, and a
/// watch on the reopened camera gets each value once. The same holds for a
/// watch leaked with `mem::forget`.
#[test]
fn watch_outlives_its_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0011";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let (exposure_watch, exposures) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched");
    let (reading_watch, readings) = camera
        .watch_extrinsic_value(params::SENSOR_TEMPERATURE_READING)
        .expect("the temperature reading should be watched");
    drop(camera);
    assert_eq!(exposures.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(readings.try_recv(), Err(TryRecvError::Disconnected));
    drop((exposure_watch, reading_watch));

    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should reopen");
    let (leaked_watch, leaked) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched");
    std::mem::forget(leaked_watch);
    drop(camera);
    assert_eq!(leaked.try_recv(), Err(TryRecvError::Disconnected));

    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should reopen");
    let (again_watch, again) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched again");
    camera
        .set(params::EXPOSURE_TIME, 45.0)
        .expect("exposure time should be set");
    assert_eq!(again.try_recv(), Ok(45.0));
    assert_eq!(again.try_recv(), Err(TryRecvError::Empty));

    drop(again_watch);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that watches can stay alive through an acquisition, and still
/// work after it. The demo camera sends no temperature readings (seen with
/// sdk 5.15).
#[test]
fn extrinsic_watch_lives_through_an_acquisition() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0012";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    camera
        .set(params::READOUT_COUNT, 2)
        .expect("readout count should be set");
    camera
        .set(params::EXPOSURE_TIME, 20.0)
        .expect("exposure time should be set");
    assert_eq!(camera.commit_parameters(), Ok(vec![]));

    let (reading_watch, readings) = camera
        .watch_extrinsic_value(params::SENSOR_TEMPERATURE_READING)
        .expect("the temperature reading should be watched");
    let (exposure_watch, exposures) = camera
        .watch_value(params::EXPOSURE_TIME)
        .expect("exposure time should be watched");

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    loop {
        let update = acquisition
            .wait(Some(Duration::from_secs(10)))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        if !update.running() {
            break;
        }
    }
    acquisition
        .finish()
        .expect("an ended acquisition should finish cleanly");
    assert_eq!(readings.try_recv(), Err(TryRecvError::Empty));

    camera
        .set(params::EXPOSURE_TIME, 30.0)
        .expect("exposure time should be set");
    assert_eq!(exposures.try_recv(), Ok(30.0));

    drop((reading_watch, exposure_watch));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that the dependent constraint watches send the new constraint when
/// setting another parameter changes it, before that `set` returns, equal to
/// the required constraint read afterwards (PICam manual 7.4.11 and 7.4.12).
/// On a PIXIS 100F, the active height bounds the ROIs and kinetics readout
/// changes the allowed trigger responses. On a PI-MAX4, sequential gating
/// caps the accumulations and DIF readout changes the aux output's pulse
/// (all seen with sdk 5.15).
#[test]
fn dependent_constraint_watches() {
    let test_library = common::library();
    let library = &test_library.library;

    // each camera uses its own serial number so demo cameras never collide
    let (id, camera) = open(library, Model::PIXIS_100F, "WATCH0013");
    let (rois_watch, rois) = camera
        .watch_rois_constraint(Parameter::Rois)
        .expect("the rois constraint should be watched");
    let (response_watch, responses) = camera
        .watch_collection_constraint(Parameter::TriggerResponse)
        .expect("the trigger response constraint should be watched");
    let height = camera
        .get(params::ACTIVE_HEIGHT)
        .expect("the active height should be read");
    camera
        .set(params::ACTIVE_HEIGHT, height / 2)
        .expect("the active height should be set");
    assert_eq!(
        rois.try_recv(),
        Ok(camera
            .rois_constraint(Parameter::Rois, ConstraintCategory::Required)
            .expect("the rois constraint should be read"))
    );
    assert_eq!(rois.try_recv(), Err(TryRecvError::Empty));
    // the trigger responses don't depend on the active height
    assert_eq!(responses.try_recv(), Err(TryRecvError::Empty));
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
        .expect("kinetics readout should be set");
    assert_eq!(
        responses.try_recv(),
        Ok(camera
            .collection_constraint(Parameter::TriggerResponse, ConstraintCategory::Required)
            .expect("the trigger response constraint should be read"))
    );
    assert_eq!(responses.try_recv(), Err(TryRecvError::Empty));
    drop((rois_watch, response_watch));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");

    let (id, camera) = open(library, Model::PI_MAX4_1024IRF, "WATCH0014");
    let (accumulations_watch, accumulations) = camera
        .watch_range_constraint(Parameter::Accumulations)
        .expect("the accumulations constraint should be watched");
    let (aux_watch, aux) = camera
        .watch_pulse_constraint(Parameter::AuxOutput)
        .expect("the aux output constraint should be watched");
    let (sequence_watch, sequences) = camera
        .watch_modulations_constraint(Parameter::CustomModulationSequence)
        .expect("the modulation sequence constraint should be watched");
    camera
        .set(params::GATING_MODE, GatingMode::Sequential)
        .expect("sequential gating should be set");
    assert_eq!(
        accumulations.try_recv(),
        Ok(camera
            .range_constraint(Parameter::Accumulations, ConstraintCategory::Required)
            .expect("the accumulations constraint should be read"))
    );
    assert_eq!(accumulations.try_recv(), Err(TryRecvError::Empty));
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Dif)
        .expect("dif readout should be set");
    assert_eq!(
        aux.try_recv(),
        Ok(camera
            .pulse_constraint(Parameter::AuxOutput, ConstraintCategory::Required)
            .expect("the aux output constraint should be read"))
    );
    assert_eq!(aux.try_recv(), Err(TryRecvError::Empty));
    // no demo setting changes the modulation sequence's constraint, so this
    // only checks it registers (seen with sdk 5.15)
    assert_eq!(sequences.try_recv(), Err(TryRecvError::Empty));
    drop(sequence_watch);
    assert_eq!(sequences.try_recv(), Err(TryRecvError::Disconnected));
    drop((accumulations_watch, aux_watch));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a status notification sends one result once the status
/// reaches the value, then disconnects, and that a second one on the same
/// status and value registers again and fires too. The demo's sensor
/// temperature is always locked (seen with sdk 5.15).
#[test]
fn status_notification() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0015";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let status = Parameter::SensorTemperatureStatus;
    assert_eq!(camera.can_wait_for_status(status), Ok(true));
    let locked: i32 = camera
        .get_untyped(status)
        .expect("the temperature status should be read");

    let wait = Duration::from_secs(2);
    let (first_watch, first) = camera
        .notify_when_status(status, locked)
        .expect("the notification should be set");
    assert_eq!(first.recv_timeout(wait), Ok(Ok(())));
    assert_eq!(first.try_recv(), Err(TryRecvError::Disconnected));
    let (second_watch, second) = camera
        .notify_when_status(status, locked)
        .expect("the notification should be set again");
    assert_eq!(second.recv_timeout(wait), Ok(Ok(())));
    assert_eq!(second.try_recv(), Err(TryRecvError::Disconnected));

    // dropping watches that already fired does nothing
    drop((first_watch, second_watch));
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that dropping a status notification before it fires cancels it:
/// the demo's sensor temperature never unlocks (seen with sdk 5.15).
#[test]
fn status_notification_cancels_on_drop() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0016";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    let status = Parameter::SensorTemperatureStatus;
    let locked: i32 = camera
        .get_untyped(status)
        .expect("the temperature status should be read");

    // unlocked is one below locked (picam.h)
    let (unlocked_watch, unlocked) = camera
        .notify_when_status(status, locked - 1)
        .expect("the notification should be set");
    assert_eq!(
        unlocked.recv_timeout(Duration::from_millis(500)),
        Err(RecvTimeoutError::Timeout)
    );
    drop(unlocked_watch);
    assert_eq!(unlocked.try_recv(), Err(TryRecvError::Disconnected));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that constraint watches and status notifications are refused on a
/// parameter of the wrong kind (seen with sdk 5.15).
#[test]
fn constraint_and_status_watch_errors() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "WATCH0017";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    assert!(matches!(
        camera.watch_rois_constraint(Parameter::ExposureTime),
        Err(Error::ParameterHasInvalidConstraintType)
    ));
    assert!(matches!(
        camera.watch_collection_constraint(Parameter::ExposureTime),
        Err(Error::ParameterHasInvalidConstraintType)
    ));
    assert!(matches!(
        camera.watch_range_constraint(Parameter::AdcQuality),
        Err(Error::ParameterHasInvalidConstraintType)
    ));
    assert!(matches!(
        camera.notify_when_status(Parameter::ExposureTime, 0),
        Err(Error::ParameterHasInvalidValueType)
    ));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
