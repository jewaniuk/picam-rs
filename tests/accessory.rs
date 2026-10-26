//! Tests for identifying and opening accessories. PICam has no demo
//! accessories, so these check the paths where no accessory is connected,
//! and the rest is on the README's "to test" list.

mod common;

use std::sync::Arc;

use picam_rs::{Accessory, AccessoryId, ComputerInterface, Error, Model};

/// Checks that listing accessories works with none connected, and that
/// opening the first one then fails.
#[test]
fn no_accessories_connected() {
    let test_library = common::library();
    let library = &test_library.library;

    let available =
        Accessory::available_ids(library).expect("available accessories should be listed");
    let unavailable =
        Accessory::unavailable_ids(library).expect("unavailable accessories should be listed");
    let open =
        Accessory::list_open_accessories(library).expect("open accessories should be listed");
    // shown with `--nocapture`, in case the lab computer has accessories
    println!(
        "available: {available:?}, unavailable: {unavailable:?}, open: {}",
        open.len()
    );
    assert!(open.is_empty(), "nothing in this test opened an accessory");

    if available.is_empty() {
        assert_eq!(
            Accessory::open_first(Arc::clone(library)).err(),
            Some(Error::NoAccessoriesAvailable)
        );
    }
}

/// Checks that PICam refuses an accessory ID that matches nothing, with
/// `InvalidAccessoryId` from every call that takes one (seen with SDK 5.15,
/// the manual doesn't say).
#[test]
fn made_up_accessory_id_is_refused() {
    let test_library = common::library();
    let library = &test_library.library;
    let id = AccessoryId {
        model: Model::FERGIE_QTH,
        computer_interface: ComputerInterface::Usb2,
        serial_number: "NOSUCHACC".to_string(),
    };

    assert_eq!(
        id.is_connected(library).err(),
        Some(Error::InvalidAccessoryId)
    );
    assert_eq!(
        id.is_open_elsewhere(library).err(),
        Some(Error::InvalidAccessoryId)
    );
    assert_eq!(
        id.firmware_details(library).err(),
        Some(Error::InvalidAccessoryId)
    );
    assert_eq!(
        Accessory::open(&id, Arc::clone(library)).err(),
        Some(Error::InvalidAccessoryId)
    );
}
