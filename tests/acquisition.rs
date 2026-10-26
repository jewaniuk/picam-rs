//! Tests for acquiring data from demo cameras: `Camera::acquire` and
//! `Camera::start_acquisition`.
//!
//! Every test uses a PIXIS 100F demo camera: one 1340 × 100 region of 16-bit
//! pixels, one frame per readout, a 100 ms exposure by default, and time
//! stamps it can turn on (seen with SDK 5.15).

mod common;

use std::sync::Arc;
use std::time::Duration;

use picam_rs::{Camera, CameraId, Error, Library, Model, Pixels, TimeStamps, params};

/// How long any test waits for one readout, far longer than a demo camera
/// takes.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Connects and opens a PIXIS 100F demo camera.
fn open(library: &Arc<Library>, serial_number: &str) -> (CameraId, Camera) {
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, serial_number)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    (id, camera)
}

/// Sets the readout count and exposure time, in milliseconds, and commits
/// them.
fn configure(camera: &Camera, readout_count: i64, exposure_ms: f64) {
    camera
        .set(params::READOUT_COUNT, readout_count)
        .expect("readout count should be set");
    camera
        .set(params::EXPOSURE_TIME, exposure_ms)
        .expect("exposure time should be set");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "parameters failed validation: {failed:?}"
    );
}

/// Checks that `acquire` returns the readouts asked for, laid out as the
/// camera's parameters say.
#[test]
fn acquire_returns_requested_readouts() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0001";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    let frames_per_readout = usize::try_from(
        camera
            .get(params::FRAMES_PER_READOUT)
            .expect("frames per readout should be read"),
    )
    .expect("frames per readout should be positive");
    let rois = camera.get(params::ROIS).expect("regions should be read");

    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("demo camera should acquire");
    assert_eq!(readouts.len(), 3);
    for readout in readouts.iter() {
        assert_eq!(readout.frame_count(), frames_per_readout);
        for frame in readout.frames() {
            assert_eq!(frame.roi_count(), rois.len());
            for (data, roi) in frame.rois().zip(&rois) {
                assert_eq!(data.roi(), roi);
                let pixels =
                    (roi.width / roi.x_binning) as usize * (roi.height / roi.y_binning) as usize;
                match data.pixels() {
                    Some(Pixels::Mono16(p)) => assert_eq!(p.len(), pixels),
                    Some(Pixels::Mono32(p)) => assert_eq!(p.len(), pixels),
                    None => panic!("the demo camera's pixel format should be known"),
                }
            }
        }
    }

    drop(readouts);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that acquiring with uncommitted changes is refused.
#[test]
fn acquire_needs_committed_parameters() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0002";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    camera
        .set(params::EXPOSURE_TIME, 25.0)
        .expect("exposure time should be set");

    assert_eq!(
        camera.acquire(1, Some(TIMEOUT)).err(),
        Some(Error::ParametersNotCommitted)
    );
    assert_eq!(
        camera.start_acquisition().err(),
        Some(Error::ParametersNotCommitted)
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that waiting on a started acquisition brings every readout, then
/// says it has ended.
#[test]
fn streaming_brings_every_readout() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0003";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 5, 20.0);

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    let mut total = 0;
    loop {
        let update = acquisition
            .wait(Some(TIMEOUT))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        total += update.readouts().len();
        if !update.running() {
            break;
        }
    }
    assert_eq!(total, 5);
    acquisition
        .finish()
        .expect("an ended acquisition should finish cleanly");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that `stop` ends a long acquisition early.
#[test]
fn stop_ends_acquisition_early() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0004";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    let maximum = camera
        .get(params::EXACT_READOUT_COUNT_MAXIMUM)
        .expect("exact readout count maximum should be read");
    let readout_count = maximum.min(1000);
    configure(&camera, readout_count, 20.0);

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    let mut total = 0;
    // wait for some data before stopping
    while total == 0 {
        let update = acquisition
            .wait(Some(TIMEOUT))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        total += update.readouts().len();
    }
    acquisition.stop().expect("acquisition should stop");
    loop {
        let update = acquisition
            .wait(Some(TIMEOUT))
            .expect("wait should work")
            .expect("a stopped acquisition should report within the timeout");
        total += update.readouts().len();
        if !update.running() {
            break;
        }
    }
    assert!(
        (total as i64) < readout_count,
        "stop should end the acquisition before every readout, got {total} of {readout_count}"
    );
    acquisition
        .finish()
        .expect("an ended acquisition should finish cleanly");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a wait shorter than one exposure returns `None` and the
/// acquisition carries on.
#[test]
fn wait_times_out_without_data() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0005";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 1, 2000.0);

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    assert!(
        acquisition
            .wait(Some(Duration::from_millis(1)))
            .expect("wait should work")
            .is_none(),
        "no readout should arrive within 1 ms of a 2 s exposure"
    );
    assert_eq!(acquisition.is_running(), Ok(true));
    acquisition.finish().expect("acquisition should finish");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that dropping a running acquisition ends it, so the camera can
/// acquire again.
#[test]
fn dropping_acquisition_ends_it() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0006";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 1000, 20.0);

    let acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    drop(acquisition);
    let readouts = camera
        .acquire(1, Some(TIMEOUT))
        .expect("the camera should acquire again after the drop");
    assert_eq!(readouts.len(), 1);

    drop(readouts);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that metadata sits after the regions' pixels, and is empty when
/// nothing is tracked.
#[test]
fn metadata_follows_the_pixels() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0007";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);

    {
        let readouts = camera
            .acquire(1, Some(TIMEOUT))
            .expect("demo camera should acquire");
        let readout = readouts.get(0).expect("one readout should arrive");
        let frame = readout.frame(0).expect("the readout should hold a frame");
        assert!(
            frame.metadata().is_empty(),
            "nothing is tracked, so there should be no metadata, got {} bytes",
            frame.metadata().len()
        );
    }

    camera
        .set(
            params::TIME_STAMPS,
            TimeStamps::EXPOSURE_STARTED | TimeStamps::EXPOSURE_ENDED,
        )
        .expect("time stamps should be turned on");
    let failed = camera
        .commit_parameters()
        .expect("parameters should be committed");
    assert!(
        failed.is_empty(),
        "parameters failed validation: {failed:?}"
    );
    let bit_depth = usize::try_from(
        camera
            .get(params::TIME_STAMP_BIT_DEPTH)
            .expect("time stamp bit depth should be read"),
    )
    .expect("time stamp bit depth should be positive");

    let readouts = camera
        .acquire(1, Some(TIMEOUT))
        .expect("demo camera should acquire with time stamps");
    let readout = readouts.get(0).expect("one readout should arrive");
    let frame = readout.frame(0).expect("the readout should hold a frame");
    // two time stamps, each (bit depth + 7) / 8 bytes (manual 6.1)
    assert_eq!(frame.metadata().len(), 2 * bit_depth.div_ceil(8));

    drop(readouts);
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
