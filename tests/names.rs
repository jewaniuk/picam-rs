//! Tests for the readable names PICam gives enum values, models and
//! parameters, which only need an initialized library and no camera.

mod common;

use picam_rs::{Model, Parameter, ValueType};

/// Names an enum value, a parameter and a model.
#[test]
fn known_values_have_names() {
    let test_library = common::library();
    let library = &test_library.library;

    for name in [
        ValueType::FloatingPoint.name(library),
        Parameter::ExposureTime.name(library),
        Model::PIXIS_100F.name(library),
    ] {
        let name = name.expect("picam should name a value it defines");
        assert!(!name.is_empty(), "names should not be empty");
    }
}

/// Asks PICam to name a value it doesn't define.
#[test]
fn unknown_value_has_no_name() {
    let test_library = common::library();

    // the manual doesn't say what happens here, sdk 5.15 is expected to
    // refuse rather than make up a name
    let result = ValueType::Unknown(9999).name(&test_library.library);
    assert!(
        result.is_err(),
        "an undefined value should not be named, got {result:?}"
    );
}
