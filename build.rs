//! Links `Picam.lib`, from the `Libraries` directory beside the SDK headers
//! in `PICAM_DIR`, or beside the SDK's default install path when it isn't
//! set.
//!
//! The raw bindings are generated ahead of time by `cargo run -p xtask --
//! generate`, so building needs neither the headers nor libclang.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=PICAM_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    // docs.rs builds docs without linking, and has no SDK to link against
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    // the target, not the host this script runs on
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!(
            "cargo:warning=not targeting windows, skipping Picam library linkage (only `cargo check` will work, not `cargo build`)"
        );
        return;
    }

    // check the environment variable for the path to the PICam include directory
    // otherwise resort to default expected path
    let include_dir = env::var("PICAM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(r"C:\Program Files\Princeton Instruments\PICam\Includes")
        });
    if let Some(parent) = include_dir.parent() {
        let lib_dir = parent.join("Libraries");
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
    }
    println!("cargo:rustc-link-lib=Picam");
}
