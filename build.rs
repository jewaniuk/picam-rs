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
