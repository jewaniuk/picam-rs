//! Tests for decoding frame metadata from demo cameras: time stamps and
//! frame numbers on a PIXIS 100F, gate tracking on a PI-MAX4 1024i and
//! modulation tracking on a PI-MAX4 1024i RF, in data PICam hands out and in
//! the crate's copies.
//!
//! Every demo model only allows 64-bit metadata values and a time stamp
//! resolution of 10 MHz (seen with SDK 5.15).

mod common;

use std::sync::Arc;
use std::time::Duration;

use picam_rs::{
    Camera, CameraId, GateTracking, GatingMode, Library, Model, Modulation, ModulationTracking,
    Parameter, Pulse, ReadoutControlMode, TimeStamps, params,
};

/// How long any test waits for one readout, far longer than a demo camera
/// takes.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Connects and opens a demo camera of `model`.
fn open(library: &Arc<Library>, model: Model, serial_number: &str) -> (CameraId, Camera) {
    let id =
        CameraId::connect_demo(library, model, serial_number).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    (id, camera)
}

/// Commits the camera's parameters, which should all be accepted.
fn commit(camera: &Camera) {
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "parameters failed validation: {failed:?}"
    );
}

/// Checks that time stamps and frame numbers decode, with the camera's
/// resolution and the exposure between them.
#[test]
fn time_stamps_and_frame_numbers() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "MET0001";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    camera
        .set(
            params::TIME_STAMPS,
            TimeStamps::EXPOSURE_STARTED | TimeStamps::EXPOSURE_ENDED,
        )
        .expect("time stamps should be turned on");
    camera
        .set(params::TRACK_FRAMES, true)
        .expect("frame tracking should be turned on");
    camera
        .set(params::EXPOSURE_TIME, 50.0)
        .expect("exposure time should be set");
    commit(&camera);
    let resolution = camera
        .get(params::TIME_STAMP_RESOLUTION)
        .expect("time stamp resolution should be read");

    // twice, since frame numbers start again in every acquisition (seen with
    // sdk 5.15)
    for _ in 0..2 {
        let readouts = camera
            .acquire(3, Some(TIMEOUT))
            .expect("demo camera should acquire");
        assert_eq!(readouts.len(), 3);
        let mut last_start = None;
        for (number, readout) in (1_i64..).zip(readouts.iter()) {
            let frame = readout.frame(0).expect("the readout should hold a frame");
            // two time stamps and a frame number, 8 bytes each
            assert_eq!(frame.metadata_bytes().len(), 24);
            let metadata = frame.metadata();
            assert_eq!(metadata.frame_number, Some(number));

            let started = metadata
                .exposure_started
                .expect("the exposure start should be stamped");
            let ended = metadata
                .exposure_ended
                .expect("the exposure end should be stamped");
            assert_eq!(started.resolution(), resolution);
            assert_eq!(ended.resolution(), resolution);
            let exposure = ended.as_secs_f64() - started.as_secs_f64();
            assert!(
                (exposure - 0.050).abs() < 0.001,
                "the stamps should be 50 ms apart, got {exposure} s"
            );
            if let Some(last_start) = last_start {
                assert!(started.ticks() > last_start, "starts should increase");
            }
            last_start = Some(started.ticks());

            // a PIXIS has no gate or modulation tracking
            assert_eq!((metadata.gate_delay, metadata.gate_width), (None, None));
            assert_eq!(metadata.modulation_duration, None);
            assert_eq!(metadata.modulation_frequency, None);
            assert_eq!(metadata.modulation_phase, None);
            assert_eq!(metadata.modulation_output_signal_frequency, None);
        }
    }

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that frame numbers count frames rather than readouts, carrying on
/// across readouts, with kinetics putting several frames in each.
#[test]
fn frame_numbers_count_frames() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "MET0002";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
        .expect("kinetics should be set");
    let window = camera
        .get(params::KINETICS_WINDOW_HEIGHT)
        .expect("kinetics window height should be read");
    // kinetics needs the region inside the window
    let mut rois = camera.get(params::ROIS).expect("regions should be read");
    rois.truncate(1);
    rois[0].y = 0;
    rois[0].height = window;
    rois[0].y_binning = 1;
    camera
        .set(params::ROIS, rois)
        .expect("the region should be set");
    camera
        .set(params::TRACK_FRAMES, true)
        .expect("frame tracking should be turned on");
    commit(&camera);
    let frames_per_readout = i64::from(
        camera
            .get(params::FRAMES_PER_READOUT)
            .expect("frames per readout should be read"),
    );
    assert!(
        frames_per_readout > 1,
        "kinetics should put several frames in a readout"
    );

    let readouts = camera
        .acquire(2, Some(TIMEOUT))
        .expect("demo camera should acquire");
    let mut numbers = Vec::new();
    for readout in readouts.iter() {
        for frame in readout.frames() {
            numbers.push(
                frame
                    .metadata()
                    .frame_number
                    .expect("frames should be numbered"),
            );
        }
    }
    let expected: Vec<i64> = (1..=2 * frames_per_readout).collect();
    assert_eq!(numbers, expected);

    drop(readouts);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that the crate's copies, in a stream and from the camera's buffer,
/// carry the metadata too.
#[test]
fn copies_carry_metadata() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "MET0003";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);
    camera
        .set(params::TRACK_FRAMES, true)
        .expect("frame tracking should be turned on");
    camera
        .set(params::EXPOSURE_TIME, 20.0)
        .expect("exposure time should be set");
    camera
        .set(params::READOUT_COUNT, 3)
        .expect("readout count should be set");
    commit(&camera);

    let (stream, updates) = camera
        .start_acquisition_stream()
        .expect("demo camera should start streaming");
    let mut numbers = Vec::new();
    for update in updates {
        let update = update.expect("every update should arrive whole");
        for readout in update.readouts().iter() {
            let frame = readout.frame(0).expect("the readout should hold a frame");
            numbers.push(
                frame
                    .metadata()
                    .frame_number
                    .expect("frames should be numbered"),
            );
        }
    }
    stream.finish().expect("the stream should end cleanly");
    assert_eq!(numbers, [1, 2, 3]);

    // room for 4 readouts, run until stopped
    let stride = usize::try_from(
        camera
            .get(params::READOUT_STRIDE)
            .expect("readout stride should be read"),
    )
    .expect("readout stride should be positive");
    camera
        .set_acquisition_buffer(4 * stride)
        .expect("the buffer should be set");
    camera
        .set(params::READOUT_COUNT, 0)
        .expect("readout count should be set");
    commit(&camera);
    let mut acquisition = camera
        .start_acquisition()
        .expect("demo camera should start acquiring");
    numbers.clear();
    for _ in 0..20 {
        if numbers.len() >= 5 {
            break;
        }
        let Some(update) = acquisition
            .wait(Some(TIMEOUT))
            .expect("every update should arrive whole")
        else {
            continue;
        };
        for readout in update.readouts().iter() {
            let frame = readout.frame(0).expect("the readout should hold a frame");
            numbers.push(
                frame
                    .metadata()
                    .frame_number
                    .expect("frames should be numbered"),
            );
        }
    }
    acquisition
        .finish()
        .expect("the acquisition should end cleanly");
    assert!(
        numbers.len() >= 5,
        "5 readouts should arrive, got {numbers:?}"
    );
    let expected: Vec<i64> = (1..).take(numbers.len()).collect();
    assert_eq!(numbers, expected);

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that gate tracking gives each readout's gate with sequential
/// gating, and adds nothing with repetitive gating, where PICam says it
/// isn't relevant.
#[test]
fn gate_tracking_follows_sequential_gates() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "MET0004";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PI_MAX4_1024I, SERIAL_NUMBER);
    camera
        .set(params::GATING_MODE, GatingMode::Sequential)
        .expect("sequential gating should be set");
    camera
        .set(
            params::SEQUENTIAL_STARTING_GATE,
            Pulse {
                delay: 100.0,
                width: 50.0,
            },
        )
        .expect("the starting gate should be set");
    camera
        .set(
            params::SEQUENTIAL_ENDING_GATE,
            Pulse {
                delay: 1000.0,
                width: 500.0,
            },
        )
        .expect("the ending gate should be set");
    camera
        .set(params::SEQUENTIAL_GATE_STEP_COUNT, 4)
        .expect("the step count should be set");
    camera
        .set(
            params::GATE_TRACKING,
            GateTracking::DELAY | GateTracking::WIDTH,
        )
        .expect("gate tracking should be turned on");
    commit(&camera);

    let readouts = camera
        .acquire(4, Some(TIMEOUT))
        .expect("demo camera should acquire");
    let mut gates = Vec::new();
    for readout in readouts.iter() {
        let frame = readout.frame(0).expect("the readout should hold a frame");
        // a delay and a width, 8 bytes each
        assert_eq!(frame.metadata_bytes().len(), 16);
        let metadata = frame.metadata();
        gates.push((metadata.gate_delay, metadata.gate_width));
    }
    assert_eq!(
        gates,
        [
            (Some(100.0), Some(50.0)),
            (Some(400.0), Some(200.0)),
            (Some(700.0), Some(350.0)),
            (Some(1000.0), Some(500.0)),
        ]
    );
    drop(readouts);

    camera
        .set(params::GATING_MODE, GatingMode::Repetitive)
        .expect("repetitive gating should be set");
    commit(&camera);
    assert_eq!(
        camera
            .get(params::GATE_TRACKING)
            .expect("gate tracking should be read"),
        GateTracking::DELAY | GateTracking::WIDTH,
        "gate tracking should still be turned on"
    );
    assert!(
        !camera
            .is_relevant(Parameter::GateTracking)
            .expect("relevance should be read"),
        "gate tracking shouldn't be relevant with repetitive gating"
    );
    let readouts = camera
        .acquire(1, Some(TIMEOUT))
        .expect("demo camera should acquire");
    let readout = readouts.get(0).expect("one readout should arrive");
    let frame = readout.frame(0).expect("the readout should hold a frame");
    assert!(frame.metadata_bytes().is_empty());
    let metadata = frame.metadata();
    assert_eq!((metadata.gate_delay, metadata.gate_width), (None, None));

    drop(readouts);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that modulation tracking gives each readout's entry of a custom
/// sequence, while gate tracking, which PICam says isn't relevant with
/// modulation on, adds nothing.
#[test]
fn modulation_tracking_follows_the_sequence() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "MET0005";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, Model::PI_MAX4_1024IRF, SERIAL_NUMBER);
    let sequence = vec![
        Modulation {
            duration: 1.0,
            frequency: 100.0,
            phase: 5.0,
            output_signal_frequency: 100.0,
        },
        Modulation {
            duration: 2.0,
            frequency: 100.0,
            phase: 25.0,
            output_signal_frequency: 100.0,
        },
    ];
    camera
        .set(params::ENABLE_MODULATION, true)
        .expect("modulation should be turned on");
    camera
        .set(params::GATING_MODE, GatingMode::Custom)
        .expect("custom gating should be set");
    camera
        .set(params::CUSTOM_MODULATION_SEQUENCE, sequence.clone())
        .expect("the custom sequence should be set");
    camera
        .set(params::MODULATION_TRACKING, ModulationTracking::all())
        .expect("modulation tracking should be turned on");
    camera
        .set(
            params::GATE_TRACKING,
            GateTracking::DELAY | GateTracking::WIDTH,
        )
        .expect("gate tracking should be turned on");
    commit(&camera);

    let readouts = camera
        .acquire(2, Some(TIMEOUT))
        .expect("demo camera should acquire");
    assert_eq!(readouts.len(), 2);
    for (entry, readout) in sequence.iter().zip(readouts.iter()) {
        let frame = readout.frame(0).expect("the readout should hold a frame");
        // four modulation values, 8 bytes each, and nothing for the gate
        assert_eq!(frame.metadata_bytes().len(), 32);
        let metadata = frame.metadata();
        assert_eq!(metadata.modulation_duration, Some(entry.duration));
        assert_eq!(metadata.modulation_frequency, Some(entry.frequency));
        assert_eq!(metadata.modulation_phase, Some(entry.phase));
        assert_eq!(
            metadata.modulation_output_signal_frequency,
            Some(entry.output_signal_frequency)
        );
        assert_eq!((metadata.gate_delay, metadata.gate_width), (None, None));
    }

    drop(readouts);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
