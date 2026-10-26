//! Tests for parameter validation (`Camera::validate_parameter`,
//! `validate_parameters` and `validate_dependent_parameter`), using a PIXIS
//! 100F demo camera, whose full-sensor region doesn't fit kinetics readout.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, ConstraintScope, Model, Parameter, ReadoutControlMode, params};

/// Checks that validation passes on a freshly opened camera, then reports a
/// pending readout mode that the camera's regions don't fit, from all three
/// calls.
#[test]
fn validation_reports_an_invalid_combination() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "VALID0001";

    let test_library = common::library();
    let library = &test_library.library;
    let id = CameraId::connect_demo(library, Model::PIXIS_100F, SERIAL_NUMBER)
        .expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");

    let clean = camera
        .validate_parameters()
        .expect("parameters should be validated");
    assert!(clean.is_valid, "a fresh camera should validate: {clean:?}");
    assert!(clean.results.is_empty());
    let single = camera
        .validate_parameter(Parameter::ExposureTime)
        .expect("exposure time should be validated");
    assert!(single.is_valid);
    assert_eq!(single.failed_parameter, None);

    // kinetics readout constrains the regions, and the default full-sensor
    // region breaks that constraint (seen with sdk 5.15)
    camera
        .set(params::READOUT_CONTROL_MODE, ReadoutControlMode::Kinetics)
        .expect("kinetics readout should be accepted as a pending value");

    let all = camera
        .validate_parameters()
        .expect("parameters should be validated");
    assert!(!all.is_valid);
    let rois = all
        .results
        .iter()
        .find(|result| result.failed_parameter == Some(Parameter::Rois))
        .unwrap_or_else(|| panic!("the regions should fail validation: {all:?}"));
    assert_eq!(
        rois.failed_error_constraint_scope,
        Some(ConstraintScope::Dependent)
    );
    assert!(
        rois.error_constraining_parameters
            .contains(&Parameter::ReadoutControlMode),
        "the readout mode should constrain the regions: {rois:?}"
    );

    let dependent = camera
        .validate_dependent_parameter(Parameter::ReadoutControlMode)
        .expect("the readout mode's dependents should be validated");
    assert!(!dependent.is_valid);
    assert_eq!(
        dependent.constraining_parameter,
        Parameter::ReadoutControlMode
    );
    assert!(
        dependent
            .failed_dependent_parameters
            .iter()
            .any(|failed| failed.parameter == Parameter::Rois),
        "the regions should be a failed dependent: {dependent:?}"
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
