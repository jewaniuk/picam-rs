//! Tests for opening and querying cameras, using PICam's demo cameras so no
//! hardware is needed.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, Error, Model, params};

/// Connects, opens, queries, closes and disconnects a demo camera, as a
/// smoke test of the whole chain from `Library` through to a parameter read.
#[test]
fn demo_camera_round_trip() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "SMOKE0001";

    let test_library = common::library();

    let models =
        Camera::available_demo_models(&test_library.library).expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");

    let model_name = model
        .name(&test_library.library)
        .expect("demo model should be named");
    assert!(
        !model_name.is_empty(),
        "demo model name should not be empty"
    );

    let id = CameraId::connect_demo(&test_library.library, model, SERIAL_NUMBER)
        .expect("demo camera should connect");
    assert!(
        id.is_demo(&test_library.library)
            .expect("demo status should be queried")
    );

    // connecting a demo camera establishes its connection (picam manual 4.3.4.3)
    assert!(
        id.is_connected(&test_library.library)
            .expect("connection status should be queried")
    );
    // the manual doesn't say what these report for a demo camera, so only
    // check that picam answers
    id.is_open_elsewhere(&test_library.library)
        .expect("open-elsewhere status should be queried");
    id.firmware_details(&test_library.library)
        .expect("firmware details should be listed");

    let camera =
        Camera::open(&id, Arc::clone(&test_library.library)).expect("demo camera should open");
    assert!(
        camera
            .is_connected()
            .expect("connection status should be queried")
    );

    // picam appends ":Demo" to demo serial numbers (seen with sdk 5.15) so
    // compare against the id it returned rather than the number we asked for
    let opened_id = camera.id().expect("camera id should be queried");
    assert_eq!(opened_id.serial_number, id.serial_number);
    assert!(
        opened_id.serial_number.starts_with(SERIAL_NUMBER),
        "serial number should start with {SERIAL_NUMBER}, got {}",
        opened_id.serial_number
    );

    // every camera reports its sensor size, whatever the model
    let width: i32 = camera
        .get(params::SENSOR_ACTIVE_WIDTH)
        .expect("sensor width should be read");
    assert!(width > 0, "sensor width should be positive, got {width}");

    drop(camera);
    id.disconnect_demo(&test_library.library)
        .expect("demo camera should disconnect");
}

/// Connects as many different demo cameras as the limit of 64 allows, less
/// one for each real camera PICam lists, then checks one more is refused with
/// `Error::TooManyDemoCameras` rather than reaching PICam, which corrupts its
/// heap at that point (seen with SDK 5.15, and on PICam 5.16.3 one camera
/// sooner with a PIXIS 100BR connected), and that a demo camera already used
/// still connects. A demo camera counts once per model and serial number
/// pair, so the one refused here reuses a model with a new serial number,
/// which crashed the process before the limit was added.
#[test]
fn demo_camera_limit() {
    let test_library = common::library();
    let library = &test_library.library;

    // real cameras take places too, counted the way the library counts them
    let mut ids = Camera::available_ids(library).expect("cameras should be listed");
    ids.extend(Camera::unavailable_ids(library).expect("cameras should be listed"));
    let real_cameras = ids
        .iter()
        .filter(|id| id.is_demo(library) != Ok(true))
        .count();
    let cameras_allowed = 64 - real_cameras;

    let models = Camera::available_demo_models(library).expect("demo models should be listed");
    assert!(
        models.len() >= cameras_allowed,
        "this test needs {cameras_allowed} demo models, PICam offers {}",
        models.len()
    );

    for (index, &model) in models.iter().take(cameras_allowed).enumerate() {
        // each demo camera gets its own serial number so they never collide
        let id = CameraId::connect_demo(library, model, &format!("LIMIT{index:04}"))
            .unwrap_or_else(|e| panic!("demo {model:?} should connect: {e}"));
        id.disconnect_demo(library)
            .unwrap_or_else(|e| panic!("demo {model:?} should disconnect: {e}"));
    }

    let result = CameraId::connect_demo(library, models[0], "LIMIT9999");
    assert!(
        matches!(result, Err(Error::TooManyDemoCameras)),
        "demo camera {} should be refused, got {result:?}",
        cameras_allowed + 1
    );

    let id = CameraId::connect_demo(library, models[0], "LIMIT0000")
        .expect("a demo camera already used should still connect");
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that a demo camera answers whether it can erase residual charge,
/// and erases it if it says it can.
#[test]
fn residual_erasure_on_demo_camera() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "RESIDUAL01";

    let test_library = common::library();
    let library = &test_library.library;
    let models = Camera::available_demo_models(library).expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");
    let id =
        CameraId::connect_demo(library, model, SERIAL_NUMBER).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");

    let erasable = camera
        .can_erase_residual()
        .expect("residual erasure should be checked");
    println!("demo {model:?} can erase residual: {erasable}");
    // what picam does when asked to erase without support isn't documented,
    // so only erase when it says it can
    if erasable {
        camera.erase_residual().expect("residual should be erased");
    }

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that user state tags a camera: 0 until set, then shared by the
/// camera's model and device handles, and separate for each camera (seen with
/// sdk 5.15).
#[test]
fn user_state_tags_a_camera() {
    let test_library = common::library();
    let library = &test_library.library;

    // each camera uses its own serial number so demo cameras never collide
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, "USERSTATE1")
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    assert_eq!(camera.user_state(), Ok(0));
    camera
        .set_user_state(42)
        .expect("the user state should be set");
    assert_eq!(camera.user_state(), Ok(42));
    let models = Camera::list_open_cameras(library).expect("open cameras should be listed");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].user_state(), Ok(42));
    let devices =
        Camera::list_open_camera_devices(library).expect("open camera devices should be listed");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].user_state(), Ok(42));

    let other_id = CameraId::connect_demo(library, Model::PIXIS_100F, "USERSTATE2")
        .expect("demo camera should connect");
    let other = Camera::open(&other_id, Arc::clone(library)).expect("demo camera should open");
    assert_eq!(other.user_state(), Ok(0));

    drop(other);
    other_id
        .disconnect_demo(library)
        .expect("demo camera should disconnect");
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
