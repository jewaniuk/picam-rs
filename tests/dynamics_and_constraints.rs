//! Tests for parameter dynamics (`Camera::dynamics`,
//! `Camera::extrinsic_dynamics`) and the calls that list every constraint on
//! a parameter (`collection_constraints` and the rest), using demo cameras.

mod common;

use std::sync::Arc;

use picam_rs::{Camera, CameraId, ConstraintCategory, Dynamics, Error, Library, Model, Parameter};

/// Connects and opens a demo camera of `model`.
fn open(library: &Arc<Library>, model: Model, serial_number: &str) -> (CameraId, Camera) {
    let id =
        CameraId::connect_demo(library, model, serial_number).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    (id, camera)
}

/// Checks that dynamics are reported, and that a reading that follows the
/// hardware can change extrinsically while a plain setting can't.
#[test]
fn dynamics_are_reported() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "DYNAMIC01";

    let test_library = common::library();
    let library = &test_library.library;
    let (id, camera) = open(library, Model::PIXIS_100F, SERIAL_NUMBER);

    let all =
        Dynamics::VALUE | Dynamics::VALUE_ACCESS | Dynamics::IS_RELEVANT | Dynamics::CONSTRAINT;
    for parameter in [
        Parameter::ExposureTime,
        Parameter::AdcQuality,
        Parameter::Rois,
    ] {
        let dynamics = camera
            .dynamics(parameter)
            .unwrap_or_else(|e| panic!("{parameter:?} dynamics should be reported: {e}"));
        assert!(
            all.contains(dynamics),
            "{parameter:?}: unexpected bits in {dynamics:?}"
        );
    }
    // a setting changes only through software, a reading also through the
    // hardware (manual 7.4.10.6, seen with sdk 5.15 on this demo model)
    assert_eq!(
        camera.extrinsic_dynamics(Parameter::ExposureTime),
        Ok(Dynamics::empty())
    );
    assert_eq!(
        camera.extrinsic_dynamics(Parameter::SensorTemperatureReading),
        Ok(Dynamics::VALUE)
    );
    // a parameter the camera doesn't have (seen with sdk 5.15)
    assert_eq!(
        camera.dynamics(Parameter::AuxOutput),
        Err(Error::ParameterDoesNotExist)
    );
    assert_eq!(
        camera.extrinsic_dynamics(Parameter::AuxOutput),
        Err(Error::ParameterDoesNotExist)
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Checks that each list of constraints is non-empty and holds the capable
/// constraint the single-constraint call returns.
#[test]
fn constraint_lists_hold_the_capable_constraint() {
    let test_library = common::library();
    let library = &test_library.library;
    let capable = ConstraintCategory::Capable;

    // each camera uses its own serial number so demo cameras never collide
    let (id, camera) = open(library, Model::PIXIS_100F, "CONSTR01");
    let collection = camera
        .collection_constraints(Parameter::AdcQuality)
        .expect("collection constraints should be listed");
    let one = camera
        .collection_constraint(Parameter::AdcQuality, capable)
        .expect("the capable collection constraint should be read");
    assert!(
        collection.contains(&one),
        "{collection:?} should hold {one:?}"
    );

    let range = camera
        .range_constraints(Parameter::ExposureTime)
        .expect("range constraints should be listed");
    let one = camera
        .range_constraint(Parameter::ExposureTime, capable)
        .expect("the capable range constraint should be read");
    assert!(range.contains(&one), "{range:?} should hold {one:?}");

    let rois = camera
        .rois_constraints(Parameter::Rois)
        .expect("rois constraints should be listed");
    let one = camera
        .rois_constraint(Parameter::Rois, capable)
        .expect("the capable rois constraint should be read");
    assert!(rois.contains(&one), "{rois:?} should hold {one:?}");

    // a parameter constrained another way is refused, not given an empty
    // list (seen with sdk 5.15)
    let invalid = Some(Error::ParameterHasInvalidConstraintType);
    assert_eq!(
        camera.collection_constraints(Parameter::ExposureTime).err(),
        invalid
    );
    assert_eq!(
        camera.range_constraints(Parameter::AdcQuality).err(),
        invalid
    );
    assert_eq!(
        camera.rois_constraints(Parameter::ExposureTime).err(),
        invalid
    );
    assert_eq!(
        camera.pulse_constraints(Parameter::ExposureTime).err(),
        invalid
    );
    assert_eq!(
        camera
            .modulations_constraints(Parameter::ExposureTime)
            .err(),
        invalid
    );
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");

    // the pixis has no pulse or modulation parameters, these intensified
    // models do (seen with sdk 5.15)
    let (id, camera) = open(library, Model::PI_MAX3_1024I, "CONSTR02");
    let pulse = camera
        .pulse_constraints(Parameter::AuxOutput)
        .expect("pulse constraints should be listed");
    let one = camera
        .pulse_constraint(Parameter::AuxOutput, capable)
        .expect("the capable pulse constraint should be read");
    assert!(pulse.contains(&one), "{pulse:?} should hold {one:?}");
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");

    let (id, camera) = open(library, Model::PI_MAX4_1024IRF, "CONSTR03");
    let modulations = camera
        .modulations_constraints(Parameter::CustomModulationSequence)
        .expect("modulations constraints should be listed");
    let one = camera
        .modulations_constraint(Parameter::CustomModulationSequence, capable)
        .expect("the capable modulations constraint should be read");
    assert!(
        modulations.contains(&one),
        "{modulations:?} should hold {one:?}"
    );
    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
