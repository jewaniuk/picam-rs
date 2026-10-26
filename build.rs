//! Generates the raw PICam bindings with bindgen.
//!
//! Reads the SDK headers from `PICAM_DIR`, or from the SDK's default install
//! path when it isn't set, and on Windows links `Picam.lib` from the
//! `Libraries` directory beside them.

use std::env;
use std::path::PathBuf;

fn main() {
    // check the environment variable for the path to the PICam include directory
    // otherwise resort to default expected path
    let include_dir = env::var("PICAM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(r"C:\Program Files\Princeton Instruments\PICam\Includes")
        });

    // find all headers needed to cover the full PICam API
    let header_names = [
        "pil_platform.h",
        "picam.h",
        "picam_advanced.h",
        "picam_accessory.h",
        "picam_em_calibration.h",
        "picam_special.h",
    ];
    let header_paths: Vec<PathBuf> = header_names
        .iter()
        .map(|name| include_dir.join(name))
        .collect();
    for header_path in &header_paths {
        if !header_path.exists() {
            panic!(
                "could not find '{}', please install the PICam SDK or set the 'PICAM_DIR' environment variable",
                header_path.display()
            );
        }
    }

    // tell cargo to re-run the build script if a header or the environment variable changes
    for header_path in &header_paths {
        println!("cargo:rerun-if-changed={}", header_path.display());
    }
    println!("cargo:rerun-if-env-changed=PICAM_DIR");

    // link against the pre-compiled library, only on windows
    if cfg!(target_os = "windows") {
        if let Some(parent) = include_dir.parent() {
            let lib_dir = parent.join("Libraries");
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        }
        println!("cargo:rustc-link-lib=Picam");
    } else {
        println!(
            "cargo:warning=not targeting windows, skipping Picam library linkage (only `cargo check` will work, not `cargo build`)"
        );
    }

    // generate the bindings
    let mut builder = bindgen::Builder::default()
        .clang_arg(format!("-I{}", include_dir.display()))
        .clang_arg("--target=x86_64-pc-windows-msvc")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: false,
        }) // most enums are real Rust enums
        .newtype_enum("^PicamError$") // exception: raw integer, never undefined behaviour on an unknown value
        .newtype_enum("^PicamParameter$") // same exception, same reason
        .newtype_enum("^PicamModel$") // same exception, same reason
        .newtype_enum("^PicamComputerInterface$") // same exception, same reason
        .newtype_enum("^PicamValueType") // same exception, same reason
        .newtype_enum("^PicamEnumeratedType") // same exception, same reason
        .newtype_enum("^PicamValueAccess") // same exception, same reason
        .newtype_enum("^PicamConstraintType") // same exception, same reason
        .newtype_enum("^PicamConstraintScope") // same exception, same reason
        .newtype_enum("^PicamConstraintSeverity") // same exception, same reason
        .newtype_enum("^PicamConstraintCategory") // same exception, same reason
        .newtype_enum("^PicamActiveShutter$") // a hardware value enum, same reason
        .newtype_enum("^PicamAdcAnalogGain$") // a hardware value enum, same reason
        .newtype_enum("^PicamAdcQuality$") // a hardware value enum, same reason
        .newtype_enum("^PicamCenterWavelengthStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamCoolingFanStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamEMIccdGainControlMode$") // a hardware value enum, same reason
        .newtype_enum("^PicamGatingMode$") // a hardware value enum, same reason
        .newtype_enum("^PicamGatingSpeed$") // a hardware value enum, same reason
        .newtype_enum("^PicamGratingCoating$") // a hardware value enum, same reason
        .newtype_enum("^PicamGratingType$") // a hardware value enum, same reason
        .newtype_enum("^PicamIntensifierStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamLaserOutputMode$") // a hardware value enum, same reason
        .newtype_enum("^PicamLaserStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamLightSource$") // a hardware value enum, same reason
        .newtype_enum("^PicamLightSourceStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamOutputSignal$") // a hardware value enum, same reason
        .newtype_enum("^PicamPhosphorType$") // a hardware value enum, same reason
        .newtype_enum("^PicamPhotocathodeSensitivity$") // a hardware value enum, same reason
        .newtype_enum("^PicamPhotonDetectionMode$") // a hardware value enum, same reason
        .newtype_enum("^PicamPixelFormat$") // a hardware value enum, same reason
        .newtype_enum("^PicamReadoutControlMode$") // a hardware value enum, same reason
        .newtype_enum("^PicamSensorTemperatureStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamSensorType$") // a hardware value enum, same reason
        .newtype_enum("^PicamShutterStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamShutterTimingMode$") // a hardware value enum, same reason
        .newtype_enum("^PicamShutterType$") // a hardware value enum, same reason
        .newtype_enum("^PicamTriggerCoupling$") // a hardware value enum, same reason
        .newtype_enum("^PicamTriggerDetermination$") // a hardware value enum, same reason
        .newtype_enum("^PicamTriggerResponse$") // a hardware value enum, same reason
        .newtype_enum("^PicamTriggerSource$") // a hardware value enum, same reason
        .newtype_enum("^PicamTriggerStatus$") // a hardware value enum, same reason
        .newtype_enum("^PicamTriggerTermination$") // a hardware value enum, same reason
        .newtype_enum("^PicamVacuumStatus$") // a hardware value enum, same reason
        .bitfield_enum("^PicamRoisConstraintRulesMask$")
        .bitfield_enum("^PicamAcquisitionErrorsMask$")
        .bitfield_enum("^PicamDynamicsMask$")
        .bitfield_enum("^PicamCcdCharacteristicsMask$")
        .bitfield_enum("^PicamGateTrackingMask$")
        .bitfield_enum("^PicamIntensifierOptionsMask$")
        .bitfield_enum("^PicamModulationTrackingMask$")
        .bitfield_enum("^PicamOrientationMask$")
        .bitfield_enum("^PicamTimeStampsMask$")
        .allowlist_function("Picam.*")
        .allowlist_type("Picam.*")
        .allowlist_type("^pi[a-z0-9]+$");
    for header_path in &header_paths {
        builder = builder.header(header_path.to_str().unwrap());
    }
    let bindings = builder
        .generate()
        .expect("unable to generate PICam bindings");

    // write the bindings to the cargo OUT_DIR
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("couldn't write bindings to output directory");
}
