//! Project tooling for `picam-rs`, following the `xtask` convention: plain
//! Rust instead of shell scripts, so it runs the same on macOS and Windows.
//!
//! Run with `cargo run -p xtask -- <task>`. Tasks:
//!
//! - `progress`: reports how much of the PICam C API is wired into the safe
//!   wrapper, using `dead_code` warnings on the raw bindings in `sys` as the
//!   signal. `sys` allows them, so it forces them back on. Pass `--list` to
//!   also print every function that is still unwrapped, grouped by header.
//! - `generate`: writes the files in `src/generated/`, the raw bindings and
//!   the lists taken straight from the PICam headers. Pass `--check` to
//!   write nothing and fail if they are out of date, or if a hand-written
//!   enum wrapper misses a header value. See `generate.rs`.
//! - `gate`: runs the checks a branch must pass before it is pushed or turned
//!   into a pull request (the generated-code check, formatting, clippy, the
//!   `xtask` tests and, on Windows, the `picam-rs` tests).
//! - `hook pre-bash` and `hook post-edit`: Claude Code hooks, run from
//!   `.claude/settings.json` rather than by hand.

mod gate;
mod generate;
mod hook;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;

/// The headers that declare the PICam API functions, matching the bindings
/// (minus `pil_platform.h`, which declares no functions).
const HEADER_NAMES: [&str; 5] = [
    "picam.h",
    "picam_advanced.h",
    "picam_accessory.h",
    "picam_em_calibration.h",
    "picam_special.h",
];

/// Where the PICam SDK installs its headers on Windows, matching `build.rs`,
/// which links `Picam.lib` from beside them.
const DEFAULT_INCLUDE_DIR: &str = r"C:\Program Files\Princeton Instruments\PICam\Includes";

const USAGE: &str = "usage: cargo run -p xtask -- \
    <progress [--list] | generate [--check] | gate | hook <pre-bash | post-edit>>";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let root = workspace_root();
    let result = match args.as_slice() {
        ["progress", rest @ ..] => progress(&root, rest.contains(&"--list")),
        ["generate"] => generate::write(&root),
        ["generate", "--check"] => generate::check(&root),
        ["gate"] => gate::run(&root, gate::Output::Inherit),
        // hooks pick their own exit codes, see `hook`
        ["hook", "pre-bash"] => return hook::pre_bash(&root),
        ["hook", "post-edit"] => return hook::post_edit(),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Checks `picam-rs` with `dead_code` warnings forced on, then prints them by
/// item kind, how many header functions are wrapped, overall and per header,
/// and how many header enums are.
///
/// # Errors
///
/// Returns an error if `cargo` can't be run, if `picam-rs` doesn't
/// compile, or if a header can't be read.
fn progress(root: &Path, list_unwrapped: bool) -> Result<(), String> {
    let dead_code_messages = dead_code_messages(root)?;

    // count warnings by the kind of item they're about, i.e. the text before
    // the first backtick ("function", "enum", "type alias" and so on)
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for message in &dead_code_messages {
        let kind = message.split('`').next().unwrap_or(message).trim();
        *kinds.entry(kind).or_default() += 1;
    }
    let mut kinds: Vec<(&str, usize)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    println!("=== dead_code warnings by item kind ===");
    for (kind, count) in &kinds {
        println!("{count:>4} {kind}");
    }
    println!();
    println!("total dead_code warnings: {}", dead_code_messages.len());
    println!();

    // names of the raw functions nothing calls yet
    let dead_functions = dead_function_names(&dead_code_messages);
    // the safe layer's own source, to see which raw enums it names. unused
    // code can't show this for enums, since a raw struct that an unwrapped
    // function takes keeps the enums in its fields from being reported
    let source = rust_source(&root.join("src"))?;

    // cross-reference against the real headers, independent of whatever
    // bindgen actually generated
    let include_dir = include_dir(root);
    let mut total = 0;
    let mut total_unwrapped = 0;
    let mut enums = 0;
    let mut enums_unwrapped = 0;
    let mut per_header = Vec::new();
    for name in HEADER_NAMES {
        let path = include_dir.join(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("could not read '{}': {e}", path.display()))?;
        let functions = api_functions(&text);
        let unwrapped: Vec<String> = functions
            .iter()
            .filter(|function| dead_functions.contains(function.as_str()))
            .cloned()
            .collect();
        total += functions.len();
        total_unwrapped += unwrapped.len();
        let header_enums = api_enums(&text);
        enums += header_enums.len();
        enums_unwrapped += header_enums
            .iter()
            .filter(|name| !names_raw_type(&source, name))
            .count();
        per_header.push((name, functions.len(), unwrapped));
    }

    let wrapped = total - total_unwrapped;
    println!(
        "functions wrapped: {wrapped} / {total} ({}%)",
        100 * wrapped / total.max(1)
    );
    for (name, count, unwrapped) in &per_header {
        println!("  {name:<24} {:>3} / {count}", count - unwrapped.len());
    }
    // an enum counts as wrapped once the safe layer names its raw type
    println!();
    println!("enums wrapped: {} / {enums}", enums - enums_unwrapped);

    if list_unwrapped {
        for (name, _, unwrapped) in &per_header {
            if unwrapped.is_empty() {
                continue;
            }
            println!();
            println!("=== unwrapped in {name} ({}) ===", unwrapped.len());
            for function in unwrapped {
                println!("  {function}");
            }
        }
    }
    Ok(())
}

/// Checks `picam-rs` with `dead_code` warnings forced on, overriding the
/// crate's own allows, and returns the text of every one.
///
/// # Errors
///
/// Returns an error if `cargo` can't be run or `picam-rs` doesn't compile.
fn dead_code_messages(root: &Path) -> Result<Vec<String>, String> {
    let output = cargo(root)
        // `cargo rustc` passes the flag to picam-rs alone, and it turns back on
        // the warnings `sys` allows
        .args([
            "rustc",
            "--package",
            "picam-rs",
            "--lib",
            "--profile",
            "check",
            "--message-format=json",
            "--",
            "--force-warn",
            "dead_code",
        ])
        .output()
        .map_err(|e| format!("could not run cargo check: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut dead_code = Vec::new();
    let mut errors = Vec::new();
    for line in stdout.lines() {
        let Ok(json) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if json["reason"] != "compiler-message" {
            continue;
        }
        let message = &json["message"];
        if message["level"] == "error" {
            errors.push(message["rendered"].as_str().unwrap_or("").to_string());
        } else if message["code"]["code"] == "dead_code" {
            dead_code.push(message["message"].as_str().unwrap_or("").to_string());
        }
    }

    if !errors.is_empty() {
        return Err(format!(
            "picam-rs does not compile, fix these first:\n\n{}",
            errors.join("\n")
        ));
    }
    if !output.status.success() {
        // failed without a compiler error, e.g. the build script panicked
        return Err(format!(
            "cargo check failed:\n\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(dead_code)
}

/// Returns a `cargo` command that runs in the workspace root.
fn cargo(root: &Path) -> Command {
    // `CARGO` is set by `cargo run` to the cargo binary that launched us
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(cargo);
    command.current_dir(root);
    command
}

/// Returns the workspace root, which is the `picam-rs` crate root.
fn workspace_root() -> PathBuf {
    // this crate lives in `<root>/xtask`
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask should live inside the workspace root")
        .to_path_buf()
}

/// Returns the PICam include directory, resolved the same way as `build.rs`:
/// `PICAM_DIR` if set (relative paths are taken from the workspace root),
/// otherwise the default SDK install path.
fn include_dir(root: &Path) -> PathBuf {
    match env::var("PICAM_DIR") {
        Ok(dir) => root.join(dir),
        Err(_) => PathBuf::from(DEFAULT_INCLUDE_DIR),
    }
}

/// Reads every `.rs` file under `dir` into one string.
///
/// # Errors
///
/// Returns an error if a directory or file can't be read.
fn rust_source(dir: &Path) -> Result<String, String> {
    let mut source = String::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("could not read '{}': {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("could not read '{}': {e}", dir.display()))?
            .path();
        if path.is_dir() {
            source.push_str(&rust_source(&path)?);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            source.push_str(
                &std::fs::read_to_string(&path)
                    .map_err(|e| format!("could not read '{}': {e}", path.display()))?,
            );
        }
    }
    Ok(source)
}

/// Whether `source` names the raw type `sys::{name}`, as every wrapper does.
fn names_raw_type(source: &str, name: &str) -> bool {
    let path = format!("sys::{name}");
    source.match_indices(&path).any(|(index, _)| {
        !source[index + path.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Returns the names of the functions that `dead_code` warnings are about.
fn dead_function_names(messages: &[String]) -> BTreeSet<&str> {
    messages
        .iter()
        .filter_map(|message| message.strip_prefix("function `"))
        .filter_map(|rest| rest.split('`').next())
        .collect()
}

/// Extracts the name of every enum declared as `typedef enum Name` in a
/// header's text.
fn api_enums(header: &str) -> Vec<String> {
    const MARKER: &str = "typedef enum ";
    header
        .match_indices(MARKER)
        .map(|(index, _)| {
            header[index + MARKER.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Extracts the name of every function declared as `PICAM_API Name(` in a
/// header's text.
fn api_functions(header: &str) -> Vec<String> {
    const MARKER: &str = "PICAM_API";
    let mut names = Vec::new();
    for (index, _) in header.match_indices(MARKER) {
        let rest = &header[index + MARKER.len()..];
        // skip longer identifiers that merely start with the marker
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        // the `#define PICAM_API ...` line itself is followed by more
        // tokens, not an opening parenthesis, so it's skipped here
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with('(') {
            names.push(name);
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_header_enums() {
        let header = "typedef enum PicamOne { PicamOne_A } PicamOne;
                      typedef struct PicamTwo { int a; } PicamTwo;
                      typedef enum PicamThree { PicamThree_B } PicamThree;";
        assert_eq!(api_enums(header), ["PicamOne", "PicamThree"]);
    }

    #[test]
    fn finds_raw_type_names() {
        let source = "let raw: sys::PicamWidget = sys::PicamWidget(1);
                      let other = sys::PicamWidgetMask::PicamWidgetMask_Shiny;";
        assert!(names_raw_type(source, "PicamWidget"));
        assert!(names_raw_type(source, "PicamWidgetMask"));
        assert!(!names_raw_type(source, "PicamWid"));
        assert!(!names_raw_type(
            "// PicamGadget, not named as a type",
            "PicamGadget"
        ));
    }

    #[test]
    fn picks_dead_function_names() {
        let messages = [
            "function `Picam_A` is never used".to_string(),
            "enum `PicamB` is never used".to_string(),
        ];
        let functions = dead_function_names(&messages);
        assert_eq!(functions.into_iter().collect::<Vec<_>>(), ["Picam_A"]);
    }
}
