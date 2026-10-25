//! Checks every typed handle in `params` against PICam itself, on every demo
//! camera model: where a model has the parameter, PICam must report the value
//! type the header gave it, and getting, defaulting and checking a value of
//! the handle's Rust type must not fail with `ParameterHasInvalidValueType`.
//!
//! The handles come from the same generated list the crate is built from,
//! read here with a test-only `define_parameters!`, so no second list can
//! drift out of step with it.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use picam_rs::{
    AdcQuality, Camera, CameraId, EnumeratedType, Error, Library, Parameter, PicamValue,
    ShutterTimingMode, TypedParameter, ValueType, params,
};

/// What checking the handles found, across every demo model.
#[derive(Default)]
struct Findings {
    /// Every parameter with a handle, in header order.
    all: Vec<Parameter>,
    /// The parameters at least one demo model has.
    seen: HashSet<Parameter>,
    /// Every mismatch found, one line each.
    problems: Vec<String>,
}

/// Checks one handle on `camera`, if the camera has its parameter.
fn check<T: PicamValue>(
    camera: &Camera,
    handle: TypedParameter<T>,
    value_type: ValueType,
    enumerated: Option<EnumeratedType>,
    findings: &mut Findings,
) {
    let parameter = handle.parameter();
    match camera.does_parameter_exist(handle) {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            findings
                .problems
                .push(format!("{parameter:?}: existence check failed: {e}"));
            return;
        }
    }
    findings.seen.insert(parameter);

    match camera.value_type(handle) {
        Ok(actual) if actual == value_type => {}
        other => findings.problems.push(format!(
            "{parameter:?}: value type {other:?}, header says {value_type:?}"
        )),
    }

    // for an enum parameter, the enum its handle uses must be PICam's
    if let Some(expected) = enumerated {
        match camera.enumerated_type(handle) {
            Ok(actual) if actual == expected => {}
            other => findings.problems.push(format!(
                "{parameter:?}: enumerated type {other:?}, handle uses {expected:?}"
            )),
        }
    }

    let current = camera.get(handle);
    if let Err(Error::ParameterHasInvalidValueType) = current {
        findings
            .problems
            .push(format!("{parameter:?}: get rejected the handle's type"));
    }
    if let Err(Error::ParameterHasInvalidValueType) = camera.default_value(handle) {
        findings.problems.push(format!(
            "{parameter:?}: default value rejected the handle's type"
        ));
    }
    if let Ok(value) = current
        && let Err(Error::ParameterHasInvalidValueType) = camera.can_set(handle, value)
    {
        findings
            .problems
            .push(format!("{parameter:?}: can set rejected the handle's type"));
    }
}

/// Reads the generated parameter list into `check_all` and `all_parameters`.
macro_rules! define_parameters {
    ($(
        $(#[$meta:meta])*
        $name:ident = $raw:ident, $handle:ident: $ty:ty, $value_type:ident $(($enumerated:ident))?;
    )*) => {
        /// Checks every handle on `camera`.
        fn check_all(camera: &Camera, findings: &mut Findings) {
            $(check(
                camera,
                params::$handle,
                ValueType::$value_type,
                None $(.or(Some(EnumeratedType::$enumerated)))?,
                findings,
            );)*
        }

        /// Every parameter with a handle, in header order.
        fn all_parameters() -> Vec<Parameter> {
            vec![$(params::$handle.parameter()),*]
        }
    };
}

include!("../src/generated/parameters.rs");

/// How many demo models to connect per library. One library connects at most
/// 64 different demo cameras (model and serial number pairs), because PICam
/// 5.15 corrupts its heap on the 65th, so `connect_demo` refuses it. The
/// models are checked in batches, each with a fresh library.
const MODELS_PER_LIBRARY: usize = 32;

/// Connects each demo model in turn and checks every handle on it.
#[test]
fn every_handle_matches_picam() {
    // this test makes its own libraries, one per batch, so it holds the lock
    let _guard = common::lock();

    let models = {
        let library = Library::new().expect("PICam library should initialize");
        Camera::available_demo_models(&library).expect("demo models should be listed")
    };
    assert!(
        !models.is_empty(),
        "PICam should offer at least one demo model"
    );

    let mut findings = Findings {
        all: all_parameters(),
        ..Findings::default()
    };
    for (batch_index, batch) in models.chunks(MODELS_PER_LIBRARY).enumerate() {
        let library = Arc::new(Library::new().expect("PICam library should initialize"));
        for (offset, &model) in batch.iter().enumerate() {
            // each demo camera gets its own serial number so they never collide
            let serial_number = format!("HANDLE{:04}", batch_index * MODELS_PER_LIBRARY + offset);
            let id = CameraId::connect_demo(&library, model, &serial_number)
                .unwrap_or_else(|e| panic!("demo {model:?} should connect: {e}"));
            let camera = Camera::open(&id, Arc::clone(&library))
                .unwrap_or_else(|e| panic!("demo {model:?} should open: {e}"));
            check_all(&camera, &mut findings);
            drop(camera);
            id.disconnect_demo(&library)
                .unwrap_or_else(|e| panic!("demo {model:?} should disconnect: {e}"));
        }
        drop(library);
    }

    // parameters no demo model has are only checked against the header, so
    // list them (shown with `--nocapture`)
    let unseen: Vec<&Parameter> = findings
        .all
        .iter()
        .filter(|parameter| !findings.seen.contains(parameter))
        .collect();
    println!(
        "checked {} of {} handles on {} demo models, never seen: {unseen:?}",
        findings.seen.len(),
        findings.all.len(),
        models.len()
    );

    assert!(
        findings.problems.is_empty(),
        "handles disagree with PICam:\n{}",
        findings.problems.join("\n")
    );
}

/// Shows that PICam reports a wrong value type as
/// `Error::ParameterHasInvalidValueType`, which the checks above look for, by
/// reading an integer parameter through the floating-point call.
#[test]
fn wrong_value_type_is_reported() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "HANDLECTRL";

    let test_library = common::library();
    let library = &test_library.library;
    let models = Camera::available_demo_models(library).expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");
    let id =
        CameraId::connect_demo(library, model, SERIAL_NUMBER).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");

    let result = camera.get_untyped::<f64>(Parameter::SensorActiveWidth);
    assert!(
        matches!(result, Err(Error::ParameterHasInvalidValueType)),
        "reading an integer parameter as f64 should be rejected, got {result:?}"
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}

/// Shows that an untyped call naming the wrong enum is refused with
/// `Error::EnumeratedTypeMismatch`, which PICam can't catch itself since it
/// reads every enum through the same integer call, and that the right enum
/// works.
#[test]
fn untyped_enum_is_checked() {
    // each test uses its own serial number so demo cameras never collide
    const SERIAL_NUMBER: &str = "HANDLEENUM";

    let test_library = common::library();
    let library = &test_library.library;
    let models = Camera::available_demo_models(library).expect("demo models should be listed");
    let model = *models
        .first()
        .expect("PICam should offer at least one demo model");
    let id =
        CameraId::connect_demo(library, model, SERIAL_NUMBER).expect("demo camera should connect");
    let camera = Camera::open(&id, Arc::clone(library)).expect("demo camera should open");
    assert!(
        camera
            .does_parameter_exist(Parameter::ShutterTimingMode)
            .expect("existence should be checked"),
        "the first demo model should have a shutter timing mode"
    );

    let wrong = camera.get_untyped::<AdcQuality>(Parameter::ShutterTimingMode);
    assert!(
        matches!(
            wrong,
            Err(Error::EnumeratedTypeMismatch {
                expected: EnumeratedType::AdcQuality,
                found: EnumeratedType::ShutterTimingMode,
            })
        ),
        "reading a shutter timing mode as an adc quality should be refused, got {wrong:?}"
    );
    // integer and floating-point parameters have no enum, and picam's
    // enumerated type query refuses them (seen with sdk 5.15, the manual only
    // says enum parameters are valid there)
    for parameter in [Parameter::SensorActiveWidth, Parameter::ExposureTime] {
        let not_enum = camera.get_untyped::<AdcQuality>(parameter);
        assert!(
            matches!(not_enum, Err(Error::ParameterHasInvalidValueType)),
            "reading {parameter:?} as an enum should be refused, got {not_enum:?}"
        );
    }
    let right = camera.get_untyped::<ShutterTimingMode>(Parameter::ShutterTimingMode);
    assert!(
        right.is_ok(),
        "reading a shutter timing mode as one should work, got {right:?}"
    );

    drop(camera);
    id.disconnect_demo(library)
        .expect("demo camera should disconnect");
}
