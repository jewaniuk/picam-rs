//! Tests for acquiring data from demo cameras: `Camera::acquire` and
//! `Camera::start_acquisition`, into PICam's memory or a buffer the camera
//! keeps, and acquisition state watches.
//!
//! Every test uses a PIXIS 100F demo camera: one 1340 × 100 region of 16-bit
//! pixels, one frame per readout, a 100 ms exposure by default, and time
//! stamps it can turn on (seen with SDK 5.15).

mod common;

use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::thread;
use std::time::Duration;

use picam_rs::{
    Acquisition, AcquisitionErrors, AcquisitionState, Camera, CameraId, Error, Library, Model,
    Pixels, Roi, TimeStamps, params,
};

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
    let readout_stride = usize::try_from(
        camera
            .get(params::READOUT_STRIDE)
            .expect("readout stride should be read"),
    )
    .expect("readout stride should be positive");
    let frame_stride = usize::try_from(
        camera
            .get(params::FRAME_STRIDE)
            .expect("frame stride should be read"),
    )
    .expect("frame stride should be positive");

    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("demo camera should acquire");
    assert_eq!(readouts.len(), 3);
    // three readouts, without padding after the last frame (manual 6.1)
    assert_eq!(
        readouts.bytes().len(),
        2 * readout_stride + frames_per_readout * frame_stride
    );
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
    drop(readouts);

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

/// Checks that PICam refuses to commit parameter changes while an
/// acquisition runs, which the views rely on: they slice every update with
/// the layout read when the acquisition started.
#[test]
fn commit_is_refused_while_acquiring() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0008";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 3, 20.0);
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

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    // picam refuses the change itself, not just the commit (seen with sdk
    // 5.15), so nothing can be pending to commit
    assert_eq!(
        acquisition.camera().set(params::ROIS, smaller).err(),
        Some(Error::AcquisitionInProgress)
    );
    assert_eq!(
        acquisition.camera().commit_parameters().err(),
        Some(Error::AcquisitionInProgress)
    );
    loop {
        let update = acquisition
            .wait(Some(TIMEOUT))
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

/// The committed readout stride in bytes. Read it after committing, since
/// metadata such as time stamps lengthens it.
fn readout_stride(camera: &Camera) -> usize {
    usize::try_from(
        camera
            .get(params::READOUT_STRIDE)
            .expect("readout stride should be read"),
    )
    .expect("readout stride should be positive")
}

/// Waits until more than 5 readouts, each `stride` bytes, have arrived from
/// an acquisition whose readout count of 5 was cleared online.
fn run_past_five(acquisition: &mut Acquisition<'_>, stride: usize) {
    let mut total = 0;
    while total <= 5 {
        let update = acquisition
            .wait(Some(TIMEOUT))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        for readout in update.readouts().iter() {
            assert_eq!(readout.bytes().len(), stride);
        }
        total += update.readouts().len();
        assert!(
            update.running() || total > 5,
            "the acquisition should run past its count of 5, ended after {total}"
        );
    }
}

/// Checks that a buffer is set, used, reported and cleared.
#[test]
fn acquisition_buffer_is_set_and_cleared() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0009";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 1, 20.0);
    let stride = readout_stride(&camera);

    assert_eq!(camera.acquisition_buffer_len(), Ok(0));
    camera
        .set_acquisition_buffer(3 * stride)
        .expect("the buffer should be set");
    assert_eq!(camera.acquisition_buffer_len(), Ok(3 * stride));
    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("demo camera should acquire into the buffer");
    assert_eq!(readouts.len(), 3);
    // copied out of the buffer, laid out as the camera's parameters say
    for readout in readouts.iter() {
        assert_eq!(readout.bytes().len(), stride);
    }
    drop(readouts);

    camera
        .clear_acquisition_buffer()
        .expect("the buffer should be cleared");
    assert_eq!(camera.acquisition_buffer_len(), Ok(0));
    camera
        .clear_acquisition_buffer()
        .expect("clearing with no buffer should do nothing");
    camera
        .set_acquisition_buffer(stride)
        .expect("the buffer should be set");
    camera
        .set_acquisition_buffer(0)
        .expect("a length of 0 should clear the buffer");
    assert_eq!(camera.acquisition_buffer_len(), Ok(0));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that PICam takes a buffer too small for a readout, then refuses to
/// acquire into it.
#[test]
fn buffer_smaller_than_a_readout_is_refused() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0010";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 1, 20.0);

    // picam only checks the size when acquiring (seen with sdk 5.15)
    camera
        .set_acquisition_buffer(1)
        .expect("picam should take the buffer");
    assert_eq!(camera.acquisition_buffer_len(), Ok(1));
    assert!(matches!(
        camera.acquire(1, Some(TIMEOUT)),
        Err(Error::InvalidAcquisitionBuffer)
    ));
    assert!(matches!(
        camera.start_acquisition(),
        Err(Error::InvalidAcquisitionBuffer)
    ));

    // `acquire` needs room for every readout, so it never wraps (seen with
    // sdk 5.15)
    let stride = readout_stride(&camera);
    camera
        .set_acquisition_buffer(2 * stride)
        .expect("the buffer should be set");
    assert!(matches!(
        camera.acquire(3, Some(TIMEOUT)),
        Err(Error::InvalidAcquisitionBuffer)
    ));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that acquiring until stopped, or more readouts than the camera
/// counts exactly, needs a buffer, so PICam's own memory never wraps while
/// `wait` views it in place.
#[test]
fn readout_count_zero_needs_a_buffer() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0011";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    // the count sets and commits, and only starting is refused (seen with
    // sdk 5.15)
    configure(&camera, 0, 20.0);
    assert!(matches!(
        camera.start_acquisition(),
        Err(Error::InvalidAcquisitionBuffer)
    ));
    let maximum = camera
        .get(params::EXACT_READOUT_COUNT_MAXIMUM)
        .expect("exact readout count maximum should be read");
    configure(&camera, maximum + 1, 20.0);
    assert!(matches!(
        camera.start_acquisition(),
        Err(Error::InvalidAcquisitionBuffer)
    ));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that an acquisition into a buffer runs until stopped, going round
/// the buffer more than once.
#[test]
fn acquisition_runs_until_stopped_with_a_buffer() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0012";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 0, 10.0);
    let stride = readout_stride(&camera);
    camera
        .set_acquisition_buffer(4 * stride)
        .expect("the buffer should be set");

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    let mut total = 0;
    // three times what the buffer holds, so it wraps
    while total < 12 {
        let update = acquisition
            .wait(Some(TIMEOUT))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        assert!(update.running(), "it should run until stopped");
        for readout in update.readouts().iter() {
            assert_eq!(readout.bytes().len(), stride);
            let frame = readout.frame(0).expect("the readout should hold a frame");
            let roi = frame.roi(0).expect("the frame should hold a region");
            assert_eq!(roi.bytes().len(), roi.width() * roi.height() * 2);
        }
        total += update.readouts().len();
    }
    assert_eq!(
        acquisition.camera().has_acquisition_buffer_overrun(),
        Ok(false)
    );
    acquisition
        .finish()
        .expect("a stopped acquisition should finish cleanly");

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that an update held while PICam overruns the buffer keeps its
/// data, since it's a copy, and that the overrun is reported rather than
/// handed out.
#[test]
fn overrun_is_reported_not_handed_out() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0013";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    configure(&camera, 0, 5.0);
    let stride = readout_stride(&camera);
    camera
        .set_acquisition_buffer(2 * stride)
        .expect("the buffer should be set");

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    let update = loop {
        let update = acquisition
            .wait(Some(TIMEOUT))
            .expect("wait should work")
            .expect("a demo readout should arrive within the timeout");
        if !update.readouts().is_empty() {
            break update;
        }
    };
    let copy = update.readouts().bytes().to_vec();
    // long enough for picam to go round the 2-readout buffer many times,
    // writing over this update's readout in picam's memory (seen with sdk
    // 5.15)
    thread::sleep(Duration::from_secs(2));
    assert!(
        update.readouts().bytes() == copy,
        "a held update should keep its data"
    );
    drop(update);
    assert_eq!(
        acquisition.camera().has_acquisition_buffer_overrun(),
        Ok(true)
    );
    let next = acquisition.wait(Some(TIMEOUT));
    assert!(
        matches!(
            next,
            Err(Error::AcquisitionFailed(errors)) if errors.contains(AcquisitionErrors::DATA_LOST)
        ),
        "the overrun should be reported as lost data"
    );
    acquisition
        .finish()
        .expect("a failed acquisition should finish cleanly");

    // the flag clears when the next acquisition starts (seen with sdk 5.15)
    let readouts = camera
        .acquire(1, Some(TIMEOUT))
        .expect("demo camera should acquire again");
    assert_eq!(readouts.len(), 1);
    drop(readouts);
    assert_eq!(camera.has_acquisition_buffer_overrun(), Ok(false));

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that clearing the readout count online keeps an acquisition with a
/// buffer going past its count, and that it's refused without a buffer, since
/// PICam's own memory would then become circular (seen with SDK 5.15).
#[test]
fn readout_count_is_cleared_online() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0014";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    assert_eq!(camera.can_clear_readout_count_online(), Ok(true));
    configure(&camera, 5, 20.0);
    let stride = readout_stride(&camera);
    camera
        .set_acquisition_buffer(8 * stride)
        .expect("the buffer should be set");

    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    assert_eq!(acquisition.clear_readout_count_online(), Ok(true));
    run_past_five(&mut acquisition, stride);
    acquisition
        .finish()
        .expect("a stopped acquisition should finish cleanly");

    camera
        .clear_acquisition_buffer()
        .expect("the buffer should be cleared");
    // the device keeps the cleared count, though `READOUT_COUNT` still reads
    // 5 and the parameters read as committed, so without a buffer the next
    // start is refused until they're committed again (seen with sdk 5.15)
    assert_eq!(camera.get(params::READOUT_COUNT), Ok(5));
    assert_eq!(camera.are_parameters_committed(), Ok(true));
    assert!(matches!(
        camera.start_acquisition(),
        Err(Error::InvalidAcquisitionBuffer)
    ));
    // `acquire` runs its own count instead, into picam's own memory, and
    // leaves `READOUT_COUNT` at that count afterwards (seen with sdk 5.15)
    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("acquire should take its own count");
    assert_eq!(readouts.len(), 3);
    drop(readouts);
    assert_eq!(camera.get(params::READOUT_COUNT), Ok(3));
    configure(&camera, 5, 20.0);
    let mut acquisition = camera
        .start_acquisition()
        .expect("acquisition should start");
    assert!(matches!(
        acquisition.clear_readout_count_online(),
        Err(Error::InvalidAcquisitionBuffer)
    ));
    // the refused clear leaves the count, so it ends by itself after 5
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

/// Checks the acquisition state watches on a demo camera, which says it can
/// detect neither state (seen with SDK 5.15): they still register, each
/// sends only its own state, and both disconnect when the camera closes.
#[test]
fn acquisition_state_watches() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "ACQ0015";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, mut camera) = open(library, SERIAL_NUMBER);
    for state in [
        AcquisitionState::ReadoutStarted,
        AcquisitionState::ReadoutEnded,
    ] {
        assert_eq!(camera.can_watch_acquisition_state(state), Ok(false));
    }
    let (_started_watch, started) = camera
        .watch_acquisition_state(AcquisitionState::ReadoutStarted)
        .expect("picam should take the watch");
    let (_ended_watch, ended) = camera
        .watch_acquisition_state(AcquisitionState::ReadoutEnded)
        .expect("picam should take the watch");
    configure(&camera, 3, 20.0);

    let readouts = camera
        .acquire(3, Some(TIMEOUT))
        .expect("demo camera should acquire");
    assert_eq!(readouts.len(), 3);
    drop(readouts);
    // the demo sends nothing when this test runs alone, but after other
    // acquisitions in the same process it sends one `ReadoutEnded` per
    // readout, with both counters 0 (seen with sdk 5.15), so either is
    // accepted
    for (updates, state) in [
        (&started, AcquisitionState::ReadoutStarted),
        (&ended, AcquisitionState::ReadoutEnded),
    ] {
        let updates: Vec<_> = updates.try_iter().collect();
        assert!(updates.len() <= 3, "at most one {state:?} per readout");
        assert!(updates.iter().all(|update| update.state == state));
    }

    drop(camera);
    assert_eq!(started.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(ended.try_recv(), Err(TryRecvError::Disconnected));
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
