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
    Camera, CameraId, Error, Library, Parameter, PicamValue, TypedParameter, ValueType, params,
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
    if let Ok(value) = current {
        if let Err(Error::ParameterHasInvalidValueType) = camera.can_set(handle, value) {
            findings
                .problems
                .push(format!("{parameter:?}: can set rejected the handle's type"));
        }
    }
}

/// Reads the generated parameter list into `check_all` and `all_parameters`.
macro_rules! define_parameters {
    ($(
        $(#[$meta:meta])*
        $name:ident = $raw:ident, $handle:ident: $ty:ty, $value_type:ident;
    )*) => {
        /// Checks every handle on `camera`.
        fn check_all(camera: &Camera, findings: &mut Findings) {
            $(check(camera, params::$handle, ValueType::$value_type, findings);)*
        }

        /// Every parameter with a handle, in header order.
        fn all_parameters() -> Vec<Parameter> {
            vec![$(params::$handle.parameter()),*]
        }
    };
}

include!("../src/generated/parameters.rs");

/// How many demo models to connect per library. With SDK 5.15, connecting a
/// 65th distinct demo model in one library session corrupts PICam's heap and
/// kills the process, inside `Picam_ConnectDemoCamera` itself (reproduced with
/// raw calls, see `DESIGN.md`'s open questions), so the models are checked in
/// batches, each with a fresh library.
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
