//! A read-only hardware check of the accessories PICam can see, such as an
//! IntelliCal lamp, for the Accessories items in `HARDWARE_TESTS.md`.
//!
//! The test is `#[ignore]`d and also needs its name in `PICAM_RS_HARDWARE`,
//! so it only runs when approved:
//!
//! ```text
//! PICAM_RS_HARDWARE=detect_accessories cargo test -p picam-rs --test hardware_accessory -- --ignored --exact detect_accessories --nocapture
//! ```
//!
//! Among IntelliCal units, PICam's accessory API only lists the IntelliCal
//! SWIR QTH lamp (manual Table A-4), so this first settles whether PICam can
//! see the lamp at all. Keep the output, but don't commit it: it holds serial
//! numbers.

mod common;

use std::sync::Arc;

use picam_rs::{Accessory, ConstraintCategory, ConstraintType, params};

/// Lists the accessories PICam sees and, if one is available, opens it and
/// reads its firmware, every parameter's type, access, dynamics and
/// constraints, and the lamp's source, status, age and life expectancy.
///
/// **Changes:** nothing on purpose. It never sets or commits a value, and
/// never reads the lamp's calibration (`light_source_reference`). The manual
/// doesn't say whether opening an accessory changes it, so switch the lamp
/// off before running this.
///
/// **Time:** seconds. **Preparation:** the IntelliCal connected by USB and
/// powered on, its lamp off, and LightField closed.
#[test]
#[ignore = "hardware: needs an accessory, see HARDWARE_TESTS.md"]
fn detect_accessories() {
    if common::hardware_approved("detect_accessories").is_none() {
        return;
    }
    let test_library = common::library();
    let library = &test_library.library;

    let available = Accessory::available_ids(library).expect("accessories should be listed");
    let unavailable = Accessory::unavailable_ids(library).expect("accessories should be listed");
    for id in available.iter().chain(&unavailable) {
        println!(
            "accessory {:?} ({:?}), interface {:?}",
            id.model,
            id.model.name(library),
            id.computer_interface
        );
    }
    println!(
        "{} available, {} unavailable",
        available.len(),
        unavailable.len()
    );
    let Some(id) = available.first() else {
        println!("PICam sees no available accessory");
        return;
    };

    println!(
        "connected {:?}, open elsewhere {:?}, firmware {:?}",
        id.is_connected(library),
        id.is_open_elsewhere(library),
        id.firmware_details(library)
    );
    let accessory = Accessory::open(id, Arc::clone(library)).expect("the accessory should open");
    println!(
        "opened: connected {:?}, committed {:?}",
        accessory.is_connected(),
        accessory.are_parameters_committed()
    );
    for parameter in accessory
        .parameters()
        .expect("the accessory's parameters should be listed")
    {
        let constraint = match accessory.constraint_type(parameter) {
            Ok(ConstraintType::Collection) => format!(
                "{:?}",
                accessory.collection_constraint(parameter, ConstraintCategory::Required)
            ),
            Ok(ConstraintType::Range) => format!(
                "{:?}",
                accessory.range_constraint(parameter, ConstraintCategory::Required)
            ),
            other => format!("{other:?}"),
        };
        println!(
            "{parameter:?}: type {:?}, access {:?}, dynamics {:?}, extrinsic {:?}, constraint {constraint}",
            accessory.value_type(parameter),
            accessory.value_access(parameter),
            accessory.dynamics(parameter),
            accessory.extrinsic_dynamics(parameter)
        );
    }
    println!(
        "light source {:?}, status {:?}, age {:?} min, life expectancy {:?} min",
        accessory.get(params::LIGHT_SOURCE),
        accessory.get(params::LIGHT_SOURCE_STATUS),
        accessory.get(params::AGE),
        accessory.get(params::LIFE_EXPECTANCY)
    );
    println!(
        "open accessories: {}",
        Accessory::list_open_accessories(library).map_or(0, |open| open.len())
    );
    drop(accessory);
}
