//! The gate: the checks a branch must pass before it is pushed or turned into
//! a pull request.

use std::path::Path;

use crate::{cargo, generate};

/// What to do with the output of each check.
#[derive(Clone, Copy)]
pub enum Output {
    /// Let it through to the terminal as it happens, for a person running
    /// `cargo run -p xtask -- gate`.
    Inherit,
    /// Hold onto it and return it only if the check fails, for a hook, where
    /// only the failure is worth showing to the agent.
    Capture,
}

/// Runs the generated-code check, `cargo fmt --check`, the clippy gate and
/// the tests, stopping at the first failure.
///
/// The `picam-rs` tests are skipped on platforms other than Windows because
/// they need the PICam runtime to link. The `xtask` tests always run.
///
/// # Errors
///
/// Returns an error naming the check that failed, with its output when
/// `output` is [`Output::Capture`].
pub fn run(root: &Path, output: Output) -> Result<(), String> {
    // in-process, so it runs before any cargo command
    eprintln!("gate: generated");
    generate::check(root)?;

    let mut checks: Vec<(&str, &[&str])> = vec![
        ("format", &["fmt", "--all", "--check"]),
        (
            "clippy",
            &[
                "clippy",
                "-p",
                "picam-rs",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
    ];
    if cfg!(windows) {
        checks.push(("tests", &["test", "-p", "picam-rs", "-p", "xtask"]));
    } else {
        eprintln!("gate: skipping the picam-rs tests, they only link on Windows");
        checks.push(("tests", &["test", "-p", "xtask"]));
    }

    for (name, args) in checks {
        eprintln!("gate: {name}");
        let mut command = cargo(root);
        command.args(args);
        match output {
            Output::Inherit => {
                let status = command
                    .status()
                    .map_err(|e| format!("could not run cargo {name}: {e}"))?;
                if !status.success() {
                    return Err(format!("{name} failed"));
                }
            }
            Output::Capture => {
                let result = command
                    .output()
                    .map_err(|e| format!("could not run cargo {name}: {e}"))?;
                if !result.status.success() {
                    return Err(format!(
                        "{name} failed:\n\n{}{}",
                        String::from_utf8_lossy(&result.stdout),
                        String::from_utf8_lossy(&result.stderr)
                    ));
                }
            }
        }
    }
    eprintln!("gate: passed");
    Ok(())
}
