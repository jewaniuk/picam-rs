//! Hardware tests for a PIXIS camera and its shutter, checking items in
//! `HARDWARE_TESTS.md` that demo cameras can't settle.
//!
//! Every test is `#[ignore]`d and also needs its own name in
//! `PICAM_RS_HARDWARE`, so nothing here runs unless it was approved, one test
//! at a time:
//!
//! ```text
//! PICAM_RS_HARDWARE=cooling cargo test -p picam-rs --test hardware_pixis -- --ignored --exact cooling --nocapture
//! ```
//!
//! Each test's doc says what it changes on the camera, how long it takes and
//! any preparation. Every test needs the PIXIS powered on with LightField
//! closed, and opens the first camera PICam finds, refusing anything that
//! isn't a PIXIS. They keep the shutter closed (`AlwaysClosed`), except
//! `pixel_order`, so it doesn't open and close with every frame. Nothing
//! here changes the shutter type, which PICam only reports, or calls
//! `erase_residual`.
//!
//! The tests assert what the crate relies on, as seen on demo cameras, so a
//! failure points at a real difference, and print what the items ask about.
//! Run with `--nocapture` and keep the output, but don't commit it: it holds
//! the camera's serial number.
//!
//! `PICAM_RS_HARDWARE_DEMO=1` rehearses a test on a demo PIXIS 100F instead.

mod common;

use std::collections::BTreeMap;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use common::Hardware;
use picam_rs::{
    AcquisitionErrors, AcquisitionState, Camera, CameraId, ConstraintCategory, Error, Library,
    Model, Parameter, Pixels, ReadoutControlMode, Roi, SensorTemperatureStatus, ShutterTimingMode,
    TimeStamps, params,
};

/// How long a test waits for one readout before giving up.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The demo camera's serial number, when rehearsing.
const DEMO_SERIAL_NUMBER: &str = "HWDEMO01";

/// The demo camera's ID once it's connected, when rehearsing.
static DEMO_ID: Mutex<Option<CameraId>> = Mutex::new(None);

/// Opens the PIXIS (or, when rehearsing, a demo PIXIS 100F), checks it is a
/// PIXIS, closes the shutter and commits.
///
/// # Panics
///
/// Panics if no camera opens, the camera isn't a PIXIS, or the commit fails.
fn open_pixis(library: &Arc<Library>, hardware: Hardware) -> Camera {
    let camera = match hardware {
        Hardware::Real => Camera::open_first(Arc::clone(library)).expect("the PIXIS should open"),
        Hardware::Demo => {
            // connected once, since picam refuses to connect it again
            let mut demo = DEMO_ID.lock().unwrap_or_else(PoisonError::into_inner);
            let id = demo.get_or_insert_with(|| {
                CameraId::connect_demo(library, Model::PIXIS_100F, DEMO_SERIAL_NUMBER)
                    .expect("the demo camera should connect")
            });
            Camera::open(id, Arc::clone(library)).expect("the demo camera should open")
        }
    };
    let id = camera.id().expect("the camera's ID should be read");
    let model = id
        .model
        .name(library)
        .expect("the model's name should be read");
    assert!(
        model.to_uppercase().contains("PIXIS"),
        "these tests are written for a PIXIS, not a {model}"
    );
    println!("camera: {model}");
    let set_point = params::SENSOR_TEMPERATURE_SET_POINT;
    println!(
        "after opening: set point {:?} (device {:?}, default {:?}), shutter {:?} {:?}, \
         active shutter {:?}",
        camera.get(set_point),
        camera.device().get(set_point),
        camera.default_value(set_point),
        camera.get(params::SHUTTER_TIMING_MODE),
        camera.device().get(params::SHUTTER_TIMING_MODE),
        camera.get(params::ACTIVE_SHUTTER)
    );
    if camera.does_parameter_exist(params::SHUTTER_TIMING_MODE) == Ok(true) {
        camera
            .set(params::SHUTTER_TIMING_MODE, ShutterTimingMode::AlwaysClosed)
            .expect("the shutter should be set to stay closed");
    }
    // the same set point for every test, so none warms or re-cools the camera
    if let Ok(wanted) = std::env::var("PICAM_RS_SET_POINT") {
        let wanted: f64 = wanted
            .parse()
            .expect("PICAM_RS_SET_POINT should be a number of °C");
        camera
            .set(set_point, wanted)
            .expect("the set point should be accepted");
    }
    commit(&camera);
    camera
}

/// Commits, expecting every parameter to be accepted.
fn commit(camera: &Camera) {
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "parameters failed validation: {failed:?}"
    );
}

/// The camera's readout stride in bytes.
fn readout_stride(camera: &Camera) -> usize {
    usize::try_from(
        camera
            .get(params::READOUT_STRIDE)
            .expect("the readout stride should be read"),
    )
    .expect("the readout stride should be positive")
}

/// Sets the readout count and exposure time and commits.
fn configure(camera: &Camera, readout_count: i64, exposure_ms: f64) {
    camera
        .set(params::READOUT_COUNT, readout_count)
        .expect("the readout count should be set");
    camera
        .set(params::EXPOSURE_TIME, exposure_ms)
        .expect("the exposure time should be set");
    commit(camera);
}

/// Makes readouts as fast as the camera allows without changing what it
/// does to the sensor: the fastest ADC speed, the shortest exposure, and
/// every row binned into one if the camera accepts that. Prints the frame
/// rate PICam expects.
fn fast_settings(camera: &Camera) {
    if let Ok(speeds) =
        camera.collection_constraint(params::ADC_SPEED, ConstraintCategory::Required)
        && let Some(fastest) = speeds.values.iter().copied().reduce(f64::max)
    {
        camera
            .set(params::ADC_SPEED, fastest)
            .expect("the fastest ADC speed should be set");
    }
    if let Ok(range) = camera.range_constraint(params::EXPOSURE_TIME, ConstraintCategory::Required)
    {
        camera
            .set(params::EXPOSURE_TIME, range.minimum)
            .expect("the shortest exposure should be set");
    }
    let width = camera
        .get(params::SENSOR_ACTIVE_WIDTH)
        .expect("the sensor width should be read");
    let height = camera
        .get(params::SENSOR_ACTIVE_HEIGHT)
        .expect("the sensor height should be read");
    let binned = vec![Roi {
        x: 0,
        width,
        x_binning: 1,
        y: 0,
        height,
        y_binning: height,
    }];
    if camera.can_set(params::ROIS, binned.clone()) == Ok(true) {
        camera
            .set(params::ROIS, binned)
            .expect("the binned region should be set");
    }
    commit(camera);
    println!(
        "fast settings: ADC {:?} MHz, exposure {:?} ms, frame rate {:?} per s",
        camera.get(params::ADC_SPEED),
        camera.get(params::EXPOSURE_TIME),
        camera.get(params::FRAME_RATE_CALCULATION)
    );
}

/// Turns frame tracking on, if the camera has it, and commits.
fn track_frames(camera: &Camera) -> bool {
    if camera.does_parameter_exist(params::TRACK_FRAMES) != Ok(true) {
        println!("the camera can't track frames");
        return false;
    }
    camera
        .set(params::TRACK_FRAMES, true)
        .expect("frame tracking should be turned on");
    commit(camera);
    true
}

/// Reads the sensor temperature, directly from the hardware when the camera
/// allows it.
fn temperature(camera: &Camera) -> picam_rs::Result<f64> {
    if camera.can_read(params::SENSOR_TEMPERATURE_READING) == Ok(true) {
        camera.read(params::SENSOR_TEMPERATURE_READING)
    } else {
        camera.get(params::SENSOR_TEMPERATURE_READING)
    }
}

/// Reads the sensor temperature status, directly from the hardware when the
/// camera allows it, since on a PIXIS `get` keeps returning the status PICam
/// opened with.
fn temperature_status(camera: &Camera) -> picam_rs::Result<SensorTemperatureStatus> {
    if camera.can_read(params::SENSOR_TEMPERATURE_STATUS) == Ok(true) {
        camera.read(params::SENSOR_TEMPERATURE_STATUS)
    } else {
        camera.get(params::SENSOR_TEMPERATURE_STATUS)
    }
}

/// Cools the PIXIS to its set point, and checks the status estimate,
/// temperature readings and the `Locked` notification on the way (the
/// cooling items under Parameters and Watches and notifications).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** cools to the set point committed when opening, which is
/// `PICAM_RS_SET_POINT` (in °C) if given, such as your usual LightField set
/// point, or otherwise whatever PICam opened with (printed). Nothing else:
/// `cooling_locked` checks the rest once the camera is locked.
///
/// **Time:** until the status locks, at most `PICAM_RS_COOLING_MINUTES`
/// (default 30).
///
/// **Preparation:** run this first, right after powering the PIXIS on from
/// room temperature, without opening LightField first, so the camera
/// actually has to cool. If it's already locked, the test says so and the
/// lock timing is inconclusive.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn cooling() {
    let Some(hardware) = common::hardware_approved("cooling") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let camera = open_pixis(library, hardware);
    let device = camera.device();
    let set_point = params::SENSOR_TEMPERATURE_SET_POINT;
    let status = params::SENSOR_TEMPERATURE_STATUS;

    println!(
        "set point range: {:?}",
        camera.range_constraint(set_point, ConstraintCategory::Required)
    );
    println!(
        "set point committed: {:?}, device {:?}",
        camera.get(set_point),
        device.get(set_point)
    );
    println!(
        "temperature {:?} °C, status {:?} (stored {:?}), readable directly: {:?}",
        temperature(&camera),
        temperature_status(&camera),
        camera.get(status),
        camera.can_read(params::SENSOR_TEMPERATURE_READING)
    );

    let waitable: Vec<Parameter> = camera
        .parameters()
        .expect("parameters should be listed")
        .into_iter()
        .filter(|&parameter| camera.can_wait_for_status(parameter) == Ok(true))
        .collect();
    println!("parameters PICam can wait on: {waitable:?}");
    assert!(waitable.contains(&Parameter::SensorTemperatureStatus));

    let estimate =
        camera.estimate_time_to_status_parameter_value(status, SensorTemperatureStatus::Locked);
    println!("estimated time to Locked: {estimate:?}");

    let extrinsic = camera.watch_extrinsic_value(params::SENSOR_TEMPERATURE_READING);
    if let Err(e) = &extrinsic {
        println!("watching the temperature reading failed: {e}");
    }
    let (_notify_watch, locked) = camera
        .notify_when_status(status, SensorTemperatureStatus::Locked)
        .expect("a Locked notification should be set");

    let start = Instant::now();
    let minutes: u64 = std::env::var("PICAM_RS_COOLING_MINUTES")
        .ok()
        .and_then(|minutes| minutes.parse().ok())
        .unwrap_or(30);
    let limit = Duration::from_secs(60 * minutes);
    let notified = loop {
        match locked.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => break result,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                panic!("the notification ended without a result")
            }
        }
        let readings: Vec<f64> = match &extrinsic {
            Ok((_, readings)) => readings.try_iter().collect(),
            Err(_) => Vec::new(),
        };
        println!(
            "{:>5.0} s: {:?} °C, status {:?}, {} extrinsic readings {:?}",
            start.elapsed().as_secs_f64(),
            temperature(&camera),
            temperature_status(&camera),
            readings.len(),
            readings.last()
        );
        assert!(
            start.elapsed() < limit,
            "the sensor didn't lock within {minutes} min"
        );
    };
    println!(
        "Locked notification after {:.0} s (estimated {estimate:?}): {notified:?}",
        start.elapsed().as_secs_f64()
    );
    assert_eq!(notified, Ok(()));
    assert_eq!(
        temperature_status(&camera),
        Ok(SensorTemperatureStatus::Locked)
    );
}

/// Checks a PIXIS that is already cooled and locked: waiting for `Locked`
/// once locked, and the temperature readings during an acquisition (the rest
/// of the cooling items, split from `cooling` so they can run without warming
/// the camera up first).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** 10 readouts of 100 ms with the shutter closed.
///
/// **Time:** seconds.
///
/// **Preparation:** the camera cooled and locked, by `cooling` or by
/// LightField, with `PICAM_RS_SET_POINT` at the set point it was cooled to
/// (LightField's, if LightField cooled it).
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn cooling_locked() {
    let Some(hardware) = common::hardware_approved("cooling_locked") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    let status = params::SENSOR_TEMPERATURE_STATUS;
    println!(
        "temperature {:?} °C, status {:?} (stored {:?})",
        temperature(&camera),
        temperature_status(&camera),
        camera.get(status)
    );
    assert_eq!(
        temperature_status(&camera),
        Ok(SensorTemperatureStatus::Locked),
        "cool the camera first, with `cooling` or LightField, and give the set point it was \
         cooled to as `PICAM_RS_SET_POINT`"
    );

    let start = Instant::now();
    let waited = camera.wait_for_status_parameter_value(
        status,
        SensorTemperatureStatus::Locked,
        Some(Duration::from_secs(5)),
    );
    println!(
        "waiting for Locked once locked: {waited:?} after {:.3} s",
        start.elapsed().as_secs_f64()
    );

    // temperature readings while acquiring
    let extrinsic = camera.watch_extrinsic_value(params::SENSOR_TEMPERATURE_READING);
    if let Err(e) = &extrinsic {
        println!("watching the temperature reading failed: {e}");
    }
    configure(&camera, 10, 100.0);
    let readouts = camera
        .acquire(10, Some(TIMEOUT))
        .expect("the PIXIS should acquire");
    println!("acquired {} readouts", readouts.len());
    drop(readouts);
    if let Ok((_, readings)) = &extrinsic {
        let during: Vec<f64> = readings.try_iter().collect();
        println!(
            "extrinsic readings since the watch started, including during the acquisition: \
             {during:?}"
        );
    }
    println!(
        "temperature {:?} °C, status {:?}",
        temperature(&camera),
        temperature_status(&camera)
    );
}

/// Queries a camera handle after its camera has closed, then checks whether
/// reopening the camera brings the same handle back (the one-camera part of
/// the Handles item).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** opens and closes the camera twice. **Time:** seconds.
/// **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn closed_handle() {
    let Some(hardware) = common::hardware_approved("closed_handle") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let camera = open_pixis(library, hardware);
    let handles = Camera::list_open_cameras(library).expect("open cameras should be listed");
    assert_eq!(handles.len(), 1, "only the PIXIS should be open");
    let old = format!("{:?}", handles[0]);
    println!("open handle: {old}, type {:?}", handles[0].handle_type());
    drop(camera);

    let id = handles[0].id();
    println!(
        "after closing: id {id:?}, connected {:?}",
        handles[0].is_connected()
    );
    assert!(
        matches!(id, Err(Error::InvalidHandle)),
        "a closed camera's handle should give InvalidHandle"
    );

    let camera = open_pixis(library, hardware);
    let reopened = Camera::list_open_cameras(library).expect("open cameras should be listed");
    let new = format!("{:?}", reopened[0]);
    println!(
        "reopened handle: {new}, the same value as before: {}",
        new == old
    );
    println!(
        "the old handle after reopening: id {:?}",
        handles[0].id().map(|id| id.model)
    );
    drop(camera);
}

/// Checks the parameters no demo camera has, `read` and `set_online`, the
/// common errors and a failed commit (the Parameters items, and whether
/// non-destructive readout is possible at all).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** sets an out-of-range exposure (refused), a short acquisition
/// of 5 readouts at 200 ms with an exposure change online, and kinetics
/// readout if the camera has it, which is restored afterwards.
/// **Time:** under a minute. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn parameters() {
    let Some(hardware) = common::hardware_approved("parameters") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);

    // the handles typed from the header only
    println!(
        "InactiveShutterTimingModeResult {:?}, BracketGating {:?}, InputTriggerStatus {:?}, \
         LightSource {:?}, LightSourceStatus {:?}, Age {:?}, LifeExpectancy {:?}",
        camera.get(params::INACTIVE_SHUTTER_TIMING_MODE_RESULT),
        camera.get(params::BRACKET_GATING),
        camera.get(params::INPUT_TRIGGER_STATUS),
        camera.get(params::LIGHT_SOURCE),
        camera.get(params::LIGHT_SOURCE_STATUS),
        camera.get(params::AGE),
        camera.get(params::LIFE_EXPECTANCY)
    );
    println!(
        "has non-destructive readout: {:?}",
        camera.does_parameter_exist(params::ENABLE_NONDESTRUCTIVE_READOUT)
    );

    let all = camera.parameters().expect("parameters should be listed");
    let readable: Vec<Parameter> = all
        .iter()
        .copied()
        .filter(|&parameter| camera.can_read(parameter) == Ok(true))
        .collect();
    let online: Vec<Parameter> = all
        .iter()
        .copied()
        .filter(|&parameter| camera.can_set_online(parameter) == Ok(true))
        .collect();
    println!("readable from the hardware: {readable:?}");
    println!("settable online: {online:?}");
    if readable.contains(&Parameter::SensorTemperatureReading) {
        println!(
            "read temperature: {:?}",
            camera.read(params::SENSOR_TEMPERATURE_READING)
        );
    }

    // the common errors
    if camera.does_parameter_exist(params::GATING_MODE) == Ok(false) {
        assert_eq!(
            camera.get(params::GATING_MODE).err(),
            Some(Error::ParameterDoesNotExist)
        );
    }
    assert_eq!(
        camera
            .set_untyped(Parameter::SensorTemperatureReading, 0.0_f64)
            .err(),
        Some(Error::ParameterValueIsReadOnly)
    );
    assert_eq!(
        camera.set(params::EXPOSURE_TIME, -1.0).err(),
        Some(Error::InvalidParameterValue)
    );
    let exposure_online = online.contains(&Parameter::ExposureTime);
    if exposure_online {
        assert_eq!(
            camera.set_online(params::EXPOSURE_TIME, 100.0).err(),
            Some(Error::AcquisitionNotInProgress)
        );
    }

    // set_online, and changes refused during an acquisition
    configure(&camera, 5, 200.0);
    let rois = camera
        .get(params::ROIS)
        .expect("the regions should be read");
    let smaller = vec![Roi {
        height: rois[0].height / 2,
        ..rois[0]
    }];
    let mut acquisition = camera
        .start_acquisition()
        .expect("the acquisition should start");
    let first = acquisition
        .wait(Some(TIMEOUT))
        .expect("the wait should work");
    println!(
        "first update: {} readouts",
        first.map_or(0, |u| u.readouts().len())
    );
    // a region, since an online-settable exposure may be accepted
    assert_eq!(
        acquisition.camera().set(params::ROIS, smaller).err(),
        Some(Error::AcquisitionInProgress)
    );
    if exposure_online {
        let changed = acquisition
            .camera()
            .set_online(params::EXPOSURE_TIME, 100.0);
        println!("exposure set online: {changed:?}");
        assert_eq!(changed, Ok(()));
    }
    acquisition.finish().expect("the acquisition should finish");
    println!(
        "exposure after the acquisition: {:?}, committed {:?}",
        camera.get(params::EXPOSURE_TIME),
        camera.are_parameters_committed()
    );
    commit(&camera);

    // a commit that fails validation
    let mode = camera
        .get(params::READOUT_CONTROL_MODE)
        .expect("the readout mode should be read");
    if camera.can_set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics) == Ok(true) {
        let exposure = camera
            .device()
            .get(params::EXPOSURE_TIME)
            .expect("the device's exposure should be read");
        camera
            .set(params::EXPOSURE_TIME, exposure + 23.0)
            .expect("the exposure should be set");
        camera
            .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
            .expect("kinetics should be set");
        let failed = camera.commit_parameters();
        println!(
            "kinetics with the full region: {failed:?}, committed {:?}, device exposure {:?} \
             (was {exposure}), device mode {:?}",
            camera.are_parameters_committed(),
            camera.device().get(params::EXPOSURE_TIME),
            camera.device().get(params::READOUT_CONTROL_MODE)
        );
        camera
            .set(params::READOUT_CONTROL_MODE, mode)
            .expect("the readout mode should be restored");
        camera
            .set(params::EXPOSURE_TIME, exposure)
            .expect("the exposure should be restored");
        commit(&camera);
    } else {
        println!("no kinetics readout, so no failed commit to try");
    }
    drop(camera);
}

/// Compares the camera's model with its device, checks that a value set
/// through the device takes effect on the hardware, and that the device
/// commits the model's pending values (the Camera devices items).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** exposure times of 50 and 500 ms, 3 readouts at each, and
/// kinetics readout if the camera has it, which is restored afterwards.
/// **Time:** under a minute. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn camera_device() {
    let Some(hardware) = common::hardware_approved("camera_device") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);

    {
        let device = camera.device();
        assert_eq!(
            camera.get(params::EXPOSURE_TIME),
            device.get(params::EXPOSURE_TIME)
        );
        assert_eq!(camera.get(params::ADC_SPEED), device.get(params::ADC_SPEED));
        assert_eq!(
            camera.get(params::SHUTTER_TIMING_MODE),
            device.get(params::SHUTTER_TIMING_MODE)
        );

        // the model's pending values, committed through the device
        camera
            .set(params::EXPOSURE_TIME, 77.0)
            .expect("the exposure should be set");
        println!(
            "with a pending value: device says committed {:?}",
            device.are_parameters_committed()
        );
        println!("device commit: {:?}", device.commit_parameters());
        println!(
            "afterwards: device exposure {:?}, camera committed {:?}",
            device.get(params::EXPOSURE_TIME),
            camera.are_parameters_committed()
        );
    }

    // a value set through the device reaches the hardware
    let mut timed = Vec::new();
    for exposure in [50.0, 500.0] {
        camera
            .device()
            .set(params::EXPOSURE_TIME, exposure)
            .expect("the device should take the exposure");
        println!(
            "device set {exposure} ms: model {:?}, committed {:?}",
            camera.get(params::EXPOSURE_TIME),
            camera.are_parameters_committed()
        );
        camera
            .set(params::READOUT_COUNT, 3)
            .expect("the readout count should be set");
        commit(&camera);
        let start = Instant::now();
        let readouts = camera
            .acquire(3, Some(TIMEOUT))
            .expect("the PIXIS should acquire");
        assert_eq!(readouts.len(), 3);
        drop(readouts);
        timed.push(start.elapsed());
        println!("3 readouts at {exposure} ms took {:?}", start.elapsed());
    }
    assert!(
        timed[1] > timed[0] + Duration::from_millis(900),
        "the longer exposure set through the device should take longer"
    );

    // a failed commit to the device
    let mode = camera
        .get(params::READOUT_CONTROL_MODE)
        .expect("the readout mode should be read");
    if camera.can_set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics) == Ok(true) {
        camera
            .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
            .expect("kinetics should be set");
        println!(
            "kinetics committed to the device: {:?}, device mode {:?}",
            camera.commit_parameters_to_device(),
            camera.device().get(params::READOUT_CONTROL_MODE)
        );
        camera
            .refresh_parameter_from_device(Parameter::ReadoutControlMode)
            .expect("the readout mode should be refreshed");
        println!(
            "after refreshing: mode {:?}, committed {:?}",
            camera.get(params::READOUT_CONTROL_MODE),
            camera.are_parameters_committed()
        );
        camera
            .set(params::READOUT_CONTROL_MODE, mode)
            .expect("the readout mode should be restored");
        commit(&camera);
    } else {
        println!("no kinetics readout, so no failed device commit to try");
    }
    drop(camera);
}

/// Checks that changes are refused during an acquisition, through the model
/// and the device, that `stop` between waits ends the acquisition promptly,
/// and what `READOUT_COUNT` holds after `acquire(n)` (Acquisition items).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** 3 readouts at 20 ms, then an acquisition of 50 readouts at
/// 20 ms, stopped part-way. During it, it tries to change the region (half
/// the rows) through the camera and its device, the exposure (to the same
/// 20 ms) through both, and to commit. It expects PICam to refuse all but
/// the exposure. If PICam accepts a region change on the device, it takes
/// effect on the camera at once, until the next test opens the camera.
/// **Time:** seconds. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn during_acquisition() {
    let Some(hardware) = common::hardware_approved("during_acquisition") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    configure(&camera, 1, 20.0);
    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("the PIXIS should acquire");
    assert_eq!(readouts.len(), 3);
    drop(readouts);
    let count = camera.get(params::READOUT_COUNT);
    println!("READOUT_COUNT after acquire(3): {count:?}");
    assert_eq!(count, Ok(3));

    configure(&camera, 50, 20.0);
    let rois = camera
        .get(params::ROIS)
        .expect("the regions should be read");
    let mut acquisition = camera
        .start_acquisition()
        .expect("the acquisition should start");
    acquisition
        .wait(Some(TIMEOUT))
        .expect("the wait should work");
    let refused = Some(Error::AcquisitionInProgress);
    let during = acquisition.camera();
    let smaller = vec![Roi {
        height: rois[0].height / 2,
        ..rois[0]
    }];
    // a region, since an online-settable exposure may be accepted
    assert_eq!(during.set(params::ROIS, smaller.clone()).err(), refused);
    println!(
        "model exposure change during the acquisition: {:?}",
        during.set(params::EXPOSURE_TIME, 20.0)
    );
    assert_eq!(during.commit_parameters().err(), refused);
    let device = during.device();
    assert_eq!(device.set(params::ROIS, smaller).err(), refused);
    assert_eq!(device.commit_parameters().err(), refused);
    assert_eq!(during.commit_parameters_to_device().err(), refused);
    println!(
        "device exposure change during the acquisition (online-settable: {:?}): {:?}",
        during.can_set_online(params::EXPOSURE_TIME),
        device.set(params::EXPOSURE_TIME, 20.0)
    );

    let start = Instant::now();
    acquisition.stop().expect("the acquisition should stop");
    let mut readouts = 0;
    loop {
        let update = acquisition
            .wait(Some(Duration::from_secs(5)))
            .expect("the wait should work")
            .expect("an update should arrive within 5 s of stopping");
        readouts += update.readouts().len();
        if !update.running() {
            break;
        }
    }
    println!(
        "stopped {:?} after asking, with {readouts} more readouts",
        start.elapsed()
    );
    assert!(start.elapsed() < Duration::from_secs(5));
    acquisition.finish().expect("the acquisition should finish");
    drop(camera);
}

/// Checks a finite acquisition into a buffer smaller than it, a count above
/// `EXACT_READOUT_COUNT_MAXIMUM` with a buffer, and clearing the readout
/// count online (Acquisition items).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** buffers of 2 and 4 readouts, and short acquisitions at
/// 20 ms: 5 readouts, one past the exact count maximum (stopped after its
/// first readout), and, if the camera can clear its count, one of 10 that
/// runs past 20 readouts before it's stopped.
/// **Time:** under 2 minutes. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn buffers() {
    let Some(hardware) = common::hardware_approved("buffers") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    let stride = readout_stride(&camera);

    // 5 readouts into room for 2
    configure(&camera, 5, 20.0);
    camera
        .set_acquisition_buffer(2 * stride)
        .expect("the buffer should be set");
    match camera.start_acquisition() {
        Err(e) => println!("5 readouts into a 2-readout buffer: refused, {e}"),
        Ok(mut acquisition) => {
            let mut readouts = 0;
            let mut errors = Vec::new();
            loop {
                match acquisition.wait(Some(TIMEOUT)) {
                    Ok(Some(update)) => {
                        readouts += update.readouts().len();
                        if !update.running() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        errors.push(e);
                        break;
                    }
                }
            }
            println!(
                "5 readouts into a 2-readout buffer: started, {readouts} readouts, errors {errors:?}"
            );
            let _ = acquisition.finish();
        }
    }

    // more than the exact count maximum, with a buffer
    let maximum = camera
        .get(params::EXACT_READOUT_COUNT_MAXIMUM)
        .expect("the maximum should be read");
    camera
        .set_acquisition_buffer(4 * stride)
        .expect("the buffer should be set");
    configure(&camera, maximum + 1, 20.0);
    match camera.start_acquisition() {
        Err(e) => println!("{} readouts with a buffer: refused, {e}", maximum + 1),
        Ok(mut acquisition) => {
            let first = acquisition.wait(Some(TIMEOUT)).map(|u| u.is_some());
            println!(
                "{} readouts with a buffer: started, first update {first:?}",
                maximum + 1
            );
            let _ = acquisition.stop();
            let _ = acquisition.finish();
        }
    }

    // clearing the readout count online
    let can_clear = camera.can_clear_readout_count_online();
    println!("can clear the readout count online: {can_clear:?}");
    if can_clear == Ok(true) {
        configure(&camera, 10, 20.0);
        let mut acquisition = camera
            .start_acquisition()
            .expect("the acquisition should start");
        acquisition
            .wait(Some(TIMEOUT))
            .expect("the wait should work");
        let cleared = acquisition.clear_readout_count_online();
        println!("cleared: {cleared:?}");
        assert_eq!(cleared, Ok(true));
        let mut readouts = 0;
        while readouts < 20 {
            let update = acquisition
                .wait(Some(TIMEOUT))
                .expect("the wait should work")
                .expect("a readout should arrive");
            readouts += update.readouts().len();
            assert!(update.running(), "a cleared count should keep running");
        }
        println!("still running after {readouts} readouts, past the old count of 10");
        acquisition.stop().expect("the acquisition should stop");
        acquisition.finish().expect("the acquisition should finish");
        println!(
            "afterwards: READOUT_COUNT {:?}, committed {:?}",
            camera.get(params::READOUT_COUNT),
            camera.are_parameters_committed()
        );
        let readouts = camera
            .acquire(3, Some(TIMEOUT))
            .expect("the PIXIS should acquire");
        println!("acquire(3) after clearing: {} readouts", readouts.len());
        assert_eq!(readouts.len(), 3);
    }
    drop(camera);
}

/// Leaks a running acquisition into the camera's buffer, then closes the
/// camera, which frees the buffer, to check PICam has stopped writing to it
/// (an Acquisition item).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** an acquisition at 20 ms that runs until the camera closes,
/// then reopens the camera and acquires once. **Time:** seconds.
/// **Preparation:** none. Run it last: nobody knows what PICam does when a
/// camera closes mid-acquisition, so the test program might crash, or the
/// camera might be left acquiring and need a power cycle, which also loses
/// the cooling.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn closing_while_acquiring() {
    let Some(hardware) = common::hardware_approved("closing_while_acquiring") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    let stride = readout_stride(&camera);
    configure(&camera, 0, 20.0);
    camera
        .set_acquisition_buffer(4 * stride)
        .expect("the buffer should be set");
    let mut acquisition = camera
        .start_acquisition()
        .expect("the acquisition should start");
    acquisition
        .wait(Some(TIMEOUT))
        .expect("the wait should work");
    // leaked on purpose, so the acquisition is still running when the
    // camera closes
    std::mem::forget(acquisition);
    drop(camera);
    println!("closed while acquiring");
    thread::sleep(Duration::from_secs(3));

    let mut camera = open_pixis(library, hardware);
    configure(&camera, 1, 20.0);
    let readouts = camera
        .acquire(1, Some(TIMEOUT))
        .expect("the reopened PIXIS should acquire");
    assert_eq!(readouts.len(), 1);
    println!("reopened and acquired");
    drop(readouts);
    drop(camera);
}

/// Holds one update viewed in PICam's own memory while many more readouts
/// arrive, to check that memory isn't reused during a counted acquisition,
/// then streams without a buffer and checks frame numbers run on (the
/// PICam-memory items under Acquisition and Streamed acquisitions).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** fast settings (fastest ADC speed, shortest exposure, rows
/// binned if accepted), frame tracking, and acquisitions of up to 400
/// readouts. **Time:** under 2 minutes. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn pixel_memory() {
    let Some(hardware) = common::hardware_approved("pixel_memory") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    fast_settings(&camera);
    let tracking = track_frames(&camera);
    let maximum = camera
        .get(params::EXACT_READOUT_COUNT_MAXIMUM)
        .expect("the maximum should be read");
    let count = maximum.min(400);
    camera
        .set(params::READOUT_COUNT, count)
        .expect("the readout count should be set");
    commit(&camera);

    let mut acquisition = camera
        .start_acquisition()
        .expect("the acquisition should start");
    let mut seen = 0;
    {
        let update = loop {
            let update = acquisition
                .wait(Some(TIMEOUT))
                .expect("the wait should work")
                .expect("a readout should arrive");
            if !update.readouts().is_empty() {
                break update;
            }
        };
        seen += update.readouts().len();
        let copy = update.readouts().bytes().to_vec();
        thread::sleep(Duration::from_secs(3));
        assert!(
            update.readouts().bytes() == copy,
            "an update in PICam's memory changed while it was held"
        );
        println!("an update held for 3 s kept its data");
    }
    loop {
        match acquisition.wait(Some(TIMEOUT)) {
            Ok(Some(update)) => {
                seen += update.readouts().len();
                if !update.running() {
                    break;
                }
            }
            other => panic!(
                "the acquisition should end cleanly: {:?}",
                other.map(|u| u.is_some())
            ),
        }
    }
    println!("{seen} of {count} readouts arrived");
    acquisition.finish().expect("the acquisition should finish");

    // a stream copying out of PICam's memory
    let (stream, updates) = camera
        .start_acquisition_stream()
        .expect("the stream should start");
    let mut frame_numbers = Vec::new();
    let mut errors = Vec::new();
    // bounded, in case the camera stops responding
    loop {
        match updates.recv_timeout(TIMEOUT) {
            Ok(Ok(update)) => {
                for readout in update.readouts().iter() {
                    for frame in readout.frames() {
                        frame_numbers.extend(frame.metadata().frame_number);
                    }
                }
            }
            Ok(Err(e)) => errors.push(e),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("no stream update within {TIMEOUT:?}"),
        }
    }
    stream.finish().expect("the stream should finish");
    let consecutive = frame_numbers.windows(2).all(|pair| pair[1] == pair[0] + 1);
    println!(
        "stream: {} frames, numbers consecutive {consecutive}, errors {errors:?}",
        frame_numbers.len()
    );
    if tracking {
        assert!(consecutive, "streamed frame numbers should run on");
    }
    drop(camera);
}

/// Overruns a 2-readout buffer on purpose, then checks the flag, `DATA_LOST`
/// and the flag clearing, how a slow reader fares, and whether a fast stream
/// overruns or batches readouts (the overrun items under Acquisition and
/// Streamed acquisitions).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** fast settings (fastest ADC speed, shortest exposure, rows
/// binned if accepted), frame tracking, buffers of 2 and 3 readouts, and
/// acquisitions that run until stopped, for a few seconds each.
/// **Time:** under 2 minutes. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn overrun() {
    let Some(hardware) = common::hardware_approved("overrun") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    fast_settings(&camera);
    let tracking = track_frames(&camera);
    let stride = readout_stride(&camera);
    camera
        .set(params::READOUT_COUNT, 0)
        .expect("the readout count should be set");
    commit(&camera);

    // 2 s without waiting overruns a 2-readout buffer
    camera
        .set_acquisition_buffer(2 * stride)
        .expect("the buffer should be set");
    let mut acquisition = camera
        .start_acquisition()
        .expect("the acquisition should start");
    acquisition
        .wait(Some(TIMEOUT))
        .expect("the wait should work");
    thread::sleep(Duration::from_secs(2));
    let overrun = acquisition.camera().has_acquisition_buffer_overrun();
    println!("overrun flag after 2 s: {overrun:?}");
    assert_eq!(overrun, Ok(true));
    let next = acquisition.wait(Some(TIMEOUT));
    println!("next wait: {:?}", next.as_ref().map(|u| u.is_some()));
    assert!(
        matches!(&next, Err(Error::AcquisitionFailed(errors)) if errors.contains(AcquisitionErrors::DATA_LOST)),
        "the overrun should be reported as lost data"
    );
    drop(next);
    let _ = acquisition.finish();
    camera
        .clear_acquisition_buffer()
        .expect("the buffer should be cleared");
    camera
        .set(params::READOUT_COUNT, 1)
        .expect("the readout count should be set");
    commit(&camera);
    let readouts = camera
        .acquire(1, Some(TIMEOUT))
        .expect("the PIXIS should acquire");
    drop(readouts);
    let cleared = camera.has_acquisition_buffer_overrun();
    println!("overrun flag after the next acquisition: {cleared:?}");
    assert_eq!(cleared, Ok(false));

    // a reader slower than the camera, 3-readout buffer
    camera
        .set(params::READOUT_COUNT, 0)
        .expect("the readout count should be set");
    commit(&camera);
    camera
        .set_acquisition_buffer(3 * stride)
        .expect("the buffer should be set");
    let mut acquisition = camera
        .start_acquisition()
        .expect("the acquisition should start");
    let start = Instant::now();
    let (mut kept, mut lost) = (0, 0);
    let mut frame_numbers = Vec::new();
    while start.elapsed() < Duration::from_secs(5) {
        match acquisition.wait(Some(TIMEOUT)) {
            Ok(Some(update)) => {
                kept += update.readouts().len();
                for readout in update.readouts().iter() {
                    for frame in readout.frames() {
                        frame_numbers.extend(frame.metadata().frame_number);
                    }
                }
            }
            Ok(None) => {}
            Err(Error::AcquisitionFailed(errors))
                if errors.contains(AcquisitionErrors::DATA_LOST) =>
            {
                lost += 1;
                break;
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
        thread::sleep(Duration::from_millis(50));
    }
    let increasing = frame_numbers.windows(2).all(|pair| pair[1] > pair[0]);
    println!(
        "slow reader: {kept} readouts kept, {lost} lost-data errors, frame numbers increasing {increasing}"
    );
    if tracking {
        assert!(increasing, "kept frames should never go backwards");
    }
    let _ = acquisition.stop();
    let _ = acquisition.finish();

    // a stream at full speed, 2-readout buffer
    camera
        .set_acquisition_buffer(2 * stride)
        .expect("the buffer should be set");
    let (mut stream, updates) = camera
        .start_acquisition_stream()
        .expect("the stream should start");
    let start = Instant::now();
    let mut per_update: BTreeMap<usize, usize> = BTreeMap::new();
    let mut stream_lost = 0;
    let mut stopped = false;
    loop {
        if !stopped && start.elapsed() > Duration::from_secs(3) {
            stream.stop().expect("the stream should stop");
            stopped = true;
        }
        match updates.recv_timeout(TIMEOUT) {
            Ok(Ok(update)) => *per_update.entry(update.readouts().len()).or_default() += 1,
            Ok(Err(Error::AcquisitionFailed(errors)))
                if errors.contains(AcquisitionErrors::DATA_LOST) =>
            {
                stream_lost += 1;
            }
            Ok(Err(e)) => println!("stream error: {e}"),
            Err(_) => break,
        }
    }
    println!(
        "stream: updates by readouts per update {per_update:?}, lost-data errors {stream_lost}"
    );
    stream.finish().expect("the stream should finish");
    drop(camera);
}

/// Turns on time stamps and frame tracking and checks where the metadata
/// sits, how the stamps and numbers count, and which bit depths and
/// resolutions the camera offers (the Frame metadata items for a PIXIS).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** time stamps and frame tracking, and 2 acquisitions of 5
/// readouts at 100 ms. **Time:** seconds. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn metadata() {
    let Some(hardware) = common::hardware_approved("metadata") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    let stamps = camera.does_parameter_exist(params::TIME_STAMPS) == Ok(true);
    if stamps {
        camera
            .set(
                params::TIME_STAMPS,
                TimeStamps::EXPOSURE_STARTED | TimeStamps::EXPOSURE_ENDED,
            )
            .expect("time stamps should be turned on");
    }
    let tracking = track_frames(&camera);
    for parameter in [
        Parameter::TimeStampResolution,
        Parameter::TimeStampBitDepth,
        Parameter::FrameTrackingBitDepth,
    ] {
        println!(
            "{parameter:?}: {:?}",
            camera.collection_constraint(parameter, ConstraintCategory::Required)
        );
    }
    configure(&camera, 5, 100.0);
    println!(
        "frame size {:?}, frame stride {:?}, time stamps {stamps}, frame tracking {tracking}",
        camera.get(params::FRAME_SIZE),
        camera.get(params::FRAME_STRIDE)
    );

    for _ in 0..2 {
        let readouts = camera
            .acquire(5, Some(TIMEOUT))
            .expect("the PIXIS should acquire");
        let mut numbers = Vec::new();
        for readout in readouts.iter() {
            let frame = readout.frame(0).expect("the readout should hold a frame");
            let metadata = frame.metadata();
            println!(
                "frame {:?}: {} metadata bytes, started {:?} s, ended {:?} s",
                metadata.frame_number,
                frame.metadata_bytes().len(),
                metadata.exposure_started.map(|t| t.as_secs_f64()),
                metadata.exposure_ended.map(|t| t.as_secs_f64())
            );
            numbers.extend(metadata.frame_number);
        }
        if tracking {
            assert_eq!(
                numbers,
                vec![1, 2, 3, 4, 5],
                "frame numbers should count from 1"
            );
        }
    }
    drop(camera);
}

/// Watches the ADC speed's constraint while changing the ADC quality, every
/// parameter's access while changing modes, and the acquisition states the
/// camera can detect (Watches and notifications items).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** the ADC quality and readout mode in the camera's model only,
/// each put back before the model is committed again, then 3 readouts at
/// 20 ms.
/// **Time:** seconds. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn watches() {
    let Some(hardware) = common::hardware_approved("watches") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);

    let all = camera.parameters().expect("parameters should be listed");
    let access: Vec<_> = all
        .iter()
        .filter_map(|&parameter| {
            camera
                .watch_value_access(parameter)
                .ok()
                .map(|(watch, events)| (parameter, watch, events))
        })
        .collect();
    let (_speed_watch, speeds) = camera
        .watch_collection_constraint(Parameter::AdcSpeed)
        .expect("the ADC speed's constraint should be watched");

    let quality = camera
        .get(params::ADC_QUALITY)
        .expect("the ADC quality should be read");
    let qualities = camera
        .collection_constraint(params::ADC_QUALITY, ConstraintCategory::Required)
        .map(|c| c.values)
        .unwrap_or_default();
    let current = camera
        .get_untyped::<i32>(Parameter::AdcQuality)
        .expect("the ADC quality's code should be read");
    for value in qualities {
        // constraints list enum values as their codes
        let other = value as i32;
        if other == current || camera.set_untyped(Parameter::AdcQuality, other).is_err() {
            continue;
        }
        let sent: Vec<_> = speeds.try_iter().map(|c| c.values).collect();
        let required = camera
            .collection_constraint(params::ADC_SPEED, ConstraintCategory::Required)
            .map(|c| c.values);
        println!("ADC quality {other:?}: constraints sent {sent:?}, Required now {required:?}");
        if let (Some(last), Ok(required)) = (sent.last(), &required) {
            assert_eq!(last, required, "the sent constraint should match Required");
        }
    }
    camera
        .set(params::ADC_QUALITY, quality)
        .expect("the ADC quality should be restored");

    let mode = camera
        .get(params::READOUT_CONTROL_MODE)
        .expect("the readout mode should be read");
    for value in camera
        .collection_constraint(params::READOUT_CONTROL_MODE, ConstraintCategory::Required)
        .map(|c| c.values)
        .unwrap_or_default()
    {
        let _ = camera.set_untyped(Parameter::ReadoutControlMode, value as i32);
    }
    camera
        .set(params::READOUT_CONTROL_MODE, mode)
        .expect("the readout mode should be restored");
    for (parameter, _, events) in &access {
        let changes: Vec<_> = events.try_iter().collect();
        if !changes.is_empty() {
            println!("access of {parameter:?} changed: {changes:?}");
        }
    }
    println!("watched access of {} parameters", access.len());
    drop(access);
    commit(&camera);

    let mut states = Vec::new();
    for state in [
        AcquisitionState::ReadoutStarted,
        AcquisitionState::ReadoutEnded,
    ] {
        println!(
            "can watch {state:?}: {:?}",
            camera.can_watch_acquisition_state(state)
        );
        if let Ok(watched) = camera.watch_acquisition_state(state) {
            states.push((state, watched));
        }
    }
    configure(&camera, 3, 20.0);
    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("the PIXIS should acquire");
    drop(readouts);
    thread::sleep(Duration::from_millis(500));
    for (state, (_, updates)) in &states {
        let updates: Vec<_> = updates.try_iter().collect();
        println!("{state:?} updates: {updates:?}");
    }
    drop(states);
    drop(camera);
}

/// Reads the camera's defect maps and asks whether it can erase residual
/// charge, without erasing (Defect maps and residual charge items).
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** nothing. **Time:** seconds. **Preparation:** none.
#[test]
#[ignore = "hardware: needs a PIXIS, see HARDWARE_TESTS.md"]
fn defect_maps() {
    let Some(hardware) = common::hardware_approved("defect_maps") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let camera = open_pixis(library, hardware);
    match camera.pixel_defect_map() {
        Ok(map) => println!(
            "pixel defects: {} columns, {} rows, {} points",
            map.column_defects.len(),
            map.row_defects.len(),
            map.point_defects.len()
        ),
        Err(e) => println!("pixel defect map: {e}"),
    }
    match camera.star_defect_maps() {
        Ok(maps) => println!(
            "star defect maps: {:?}",
            maps.iter()
                .map(|m| (m.id, m.defects.len()))
                .collect::<Vec<_>>()
        ),
        Err(e) => println!("star defect maps: {e}"),
    }
    println!(
        "applicable star defect map: {:?}",
        camera.star_defect_map().map(|m| (m.id, m.defects.len()))
    );
    println!(
        "can erase residual charge: {:?}",
        camera.can_erase_residual()
    );
    drop(camera);
}

/// Acquires one unbinned frame of light through the spectrometer and
/// compares how much it varies along x and along y, to check pixels are
/// stored row by row (an Acquisition item). A spectrum varies along the
/// wavelength axis, x, and little along the slit, y.
///
/// **Opening:** like every test here, it opens the camera, sets the shutter
/// to stay closed, and commits every parameter PICam opened with, including
/// the temperature set point, or `PICAM_RS_SET_POINT` if given. Give the
/// same `PICAM_RS_SET_POINT` to every run, so none warms or re-cools the
/// camera.
///
/// **Changes:** the shutter to `Normal`, so it opens for the exposure, the
/// full sensor unbinned, an exposure of `PICAM_RS_EXPOSURE_MS` (default
/// 100 ms), 1 readout, then the shutter back to `AlwaysClosed`.
/// **Time:** seconds. **Preparation:** light into the spectrometer, such as
/// room light or a lamp at the entrance slit, at a level that doesn't
/// saturate. The shutter opens and closes once.
#[test]
#[ignore = "hardware: needs a PIXIS and light, see HARDWARE_TESTS.md"]
fn pixel_order() {
    let Some(hardware) = common::hardware_approved("pixel_order") else {
        return;
    };
    let test_library = common::library();
    let library = &test_library.library;
    let mut camera = open_pixis(library, hardware);
    let width = camera
        .get(params::SENSOR_ACTIVE_WIDTH)
        .expect("the sensor width should be read");
    let height = camera
        .get(params::SENSOR_ACTIVE_HEIGHT)
        .expect("the sensor height should be read");
    camera
        .set(
            params::ROIS,
            vec![Roi {
                x: 0,
                width,
                x_binning: 1,
                y: 0,
                height,
                y_binning: 1,
            }],
        )
        .expect("the full unbinned sensor should be set");
    let exposure: f64 = std::env::var("PICAM_RS_EXPOSURE_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .unwrap_or(100.0);
    if camera.does_parameter_exist(params::SHUTTER_TIMING_MODE) == Ok(true) {
        camera
            .set(params::SHUTTER_TIMING_MODE, ShutterTimingMode::Normal)
            .expect("the shutter should be set to open for exposures");
    }
    configure(&camera, 1, exposure);
    // a closing delay too short for the shutter would smear the frame
    println!(
        "shutter: active {:?}, external type {:?}, internal type {:?}, opening delay {:?} ms, \
         closing delay {:?} ms",
        camera.get(params::ACTIVE_SHUTTER),
        camera.get(params::EXTERNAL_SHUTTER_TYPE),
        camera.get(params::INTERNAL_SHUTTER_TYPE),
        camera.get(params::SHUTTER_OPENING_DELAY),
        camera.get(params::SHUTTER_CLOSING_DELAY)
    );

    let (columns, rows, maximum) = {
        let readouts = camera
            .acquire(1, Some(TIMEOUT))
            .expect("the PIXIS should acquire");
        let readout = readouts.get(0).expect("one readout");
        let frame = readout.frame(0).expect("one frame");
        let roi = frame.rois().next().expect("one region");
        let (w, h) = (roi.width(), roi.height());
        let pixels: Vec<f64> = match roi.pixels().expect("pixels should decode") {
            Pixels::Mono16(p) => p.iter().map(|&v| f64::from(v)).collect(),
            Pixels::Mono32(p) => p.iter().map(|&v| f64::from(v)).collect(),
        };
        assert_eq!(pixels.len(), w * h);
        // the mean of each column, and of each row, assuming rows are stored
        // one after another
        let columns: Vec<f64> = (0..w)
            .map(|x| (0..h).map(|y| pixels[y * w + x]).sum::<f64>() / h as f64)
            .collect();
        let rows: Vec<f64> = (0..h)
            .map(|y| pixels[y * w..(y + 1) * w].iter().sum::<f64>() / w as f64)
            .collect();
        let maximum = pixels.iter().copied().fold(0.0, f64::max);
        (columns, rows, maximum)
    };
    if camera.does_parameter_exist(params::SHUTTER_TIMING_MODE) == Ok(true) {
        camera
            .set(params::SHUTTER_TIMING_MODE, ShutterTimingMode::AlwaysClosed)
            .expect("the shutter should be set to stay closed");
        commit(&camera);
    }

    let spread = |values: &[f64]| {
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
    };
    println!("brightest pixel {maximum} (65535 is saturated)");
    println!(
        "column means vary by {:.1}, row means by {:.1} (row by row means the first is much larger)",
        spread(&columns),
        spread(&rows)
    );
    let step = (columns.len() / 20).max(1);
    println!(
        "column means, every {step}th: {:?}",
        columns
            .iter()
            .step_by(step)
            .map(|v| v.round())
            .collect::<Vec<_>>()
    );
    println!(
        "row means: {:?}",
        rows.iter().map(|v| v.round()).collect::<Vec<_>>()
    );
    drop(camera);
}
