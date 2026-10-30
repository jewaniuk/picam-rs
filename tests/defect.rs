//! Tests for sensor defect maps (`Camera::pixel_defect_map`,
//! `star_defect_map` and `star_defect_maps`). Demo cameras have no defects,
//! so these check the calls succeed with the model handle `Camera` holds and
//! return empty maps, and real defect data is in `HARDWARE_TESTS.md`.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, Model};

/// Checks that a demo camera's defect maps are read and empty.
#[test]
fn demo_camera_has_empty_defect_maps() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DEFECT0001";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");

    let pixels = camera
        .pixel_defect_map()
        .expect("the pixel defect map should be read");
    assert!(pixels.column_defects.is_empty());
    assert!(pixels.row_defects.is_empty());
    assert!(pixels.point_defects.is_empty());

    let star = camera
        .star_defect_map()
        .expect("the applicable star defect map should be read");
    assert!(star.defects.is_empty());
    // a demo camera has one, empty, star defect map (seen with sdk 5.15)
    let stars = camera
        .star_defect_maps()
        .expect("every star defect map should be read");
    assert_eq!(stars, vec![star]);

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
