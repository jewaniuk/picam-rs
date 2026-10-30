//! Claude Code hooks, wired up in `.claude/settings.json`.
//!
//! Claude Code sends each hook a JSON description of the tool call on stdin
//! and acts on the exit code: 0 lets the call go ahead, and 2 blocks it and
//! shows stderr to the agent so it can see why. Any other exit code only
//! warns, so everything here that means "block" must exit with 2. To have
//! Claude Code ask the user instead, a hook prints a JSON decision on stdout
//! and exits with 0.

use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitCode};

use serde_json::Value;

use crate::gate::{self, Output};

/// The exit code that makes Claude Code block a tool call.
const BLOCK: u8 = 2;

/// Runs before every Bash command. Blocks pushes to `main`, force pushes,
/// branch deletion and `gh pr merge`, runs the gate before any other `git
/// push` or `gh pr create`, and asks the user before anything that runs a
/// hardware test.
///
/// Fails closed: if the hook input can't be read, the command is blocked.
pub fn pre_bash(root: &Path) -> ExitCode {
    match check_bash(root) {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(reason)) => {
            let decision = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "ask",
                    "permissionDecisionReason": reason,
                }
            });
            println!("{decision}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("{reason}");
            ExitCode::from(BLOCK)
        }
    }
}

/// Runs after every file edit, and formats the file with `rustfmt` if it is
/// Rust.
///
/// Never blocks. A file that doesn't parse part-way through a change is left
/// alone, since the compiler reports it anyway.
pub fn post_edit() -> ExitCode {
    if let Ok(input) = read_input()
        && let Some(path) = input["tool_input"]["file_path"].as_str()
        && path.ends_with(".rs")
    {
        // output is discarded, a parse error here is just noise
        let _ = Command::new("rustfmt")
            .args(["--edition", "2024", path])
            .output();
    }
    ExitCode::SUCCESS
}

/// Decides whether a Bash command may run, and whether the user must be
/// asked first, returning the reason to show them if so.
///
/// # Errors
///
/// Returns the reason to show the agent when the command is blocked.
fn check_bash(root: &Path) -> Result<Option<String>, String> {
    let input = read_input()?;
    let command = input["tool_input"]["command"]
        .as_str()
        .ok_or("hook input has no `tool_input.command`, so the command was blocked")?;

    // split on the shell's command separators (`&&`, `||`, `;`, `|`, `&`
    // and newlines) so each command in a chain is checked on its own
    let mut needs_gate = false;
    for segment in command.split([';', '|', '&', '\n']) {
        let words: Vec<&str> = segment.split_whitespace().collect();
        match words.as_slice() {
            ["git", "push", rest @ ..] => {
                check_push(rest)?;
                needs_gate = true;
            }
            ["gh", "pr", "create", ..] => needs_gate = true,
            ["gh", "pr", "merge", ..] => {
                return Err("agents never merge PRs, the maintainer does \
                            (CLAUDE.md, \"Git and GitHub\")"
                    .to_string());
            }
            _ => {}
        }
    }

    if needs_gate {
        if current_branch(root)? == "main" {
            return Err("you are on `main`. Work on a branch and open a PR \
                        (CLAUDE.md, \"Workflow for each chunk\")"
                .to_string());
        }
        gate::run(root, Output::Capture).map_err(|e| {
            format!("the gate failed, so the command was blocked. Fix this first:\n\n{e}")
        })?;
    }
    Ok(hardware_run(command))
}

/// Says why `command` might run a hardware test, if it might: it mentions
/// `PICAM_RS_HARDWARE`, a `hardware_` test, or running ignored tests, in any
/// shell's syntax, quoting or case. Hardware tests drive real devices, so the
/// maintainer approves every run (CLAUDE.md, "Hardware tests"). Matching
/// any mention also asks for harmless commands like a `grep`, which is the
/// safe side to err on.
fn hardware_run(command: &str) -> Option<String> {
    // lowercase, since windows doesn't care about the case of variable names
    let command = command.to_lowercase();
    let mention = [
        "picam_rs_hardware",
        "hardware_",
        "--ignored",
        "--include-ignored",
        // cargo nextest's flag for running ignored tests
        "--run-ignored",
    ]
    .into_iter()
    .find(|mention| command.contains(mention))?;
    Some(format!(
        "this command mentions `{mention}`, so it may run a hardware test, which \
         drives real devices. If it does, approve it only if the agent has \
         explained this run and you agreed to it (CLAUDE.md, \"Hardware tests\")"
    ))
}

/// Checks the arguments of a `git push` for anything an agent must never
/// do: target `main`, force push, delete a branch, or push every ref.
///
/// # Errors
///
/// Returns the reason to show the agent when the push is blocked.
fn check_push(args: &[&str]) -> Result<(), String> {
    for arg in args {
        let targets_main = matches!(*arg, "main" | "refs/heads/main")
            || arg.ends_with(":main")
            || arg.ends_with(":refs/heads/main");
        if targets_main {
            return Err("agents never push to `main`. Push your branch and open \
                        a PR (CLAUDE.md, \"Git and GitHub\")"
                .to_string());
        }
        let forbidden = arg.starts_with("--force")
            || arg.starts_with('+')
            || matches!(*arg, "-f" | "-d" | "--delete" | "--all" | "--mirror");
        if forbidden {
            return Err(format!(
                "`git push {arg}` is not allowed. Force pushing, deleting \
                 branches and pushing every ref need the maintainer \
                 (CLAUDE.md, stop-and-ask rule 13)"
            ));
        }
    }
    Ok(())
}

/// Returns the name of the checked-out branch, or `HEAD` when detached.
///
/// # Errors
///
/// Returns an error if `git` can't be run or fails.
fn current_branch(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run git to find the current branch: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not find the current branch:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Reads and parses the JSON that Claude Code sends on stdin.
///
/// # Errors
///
/// Returns an error if stdin can't be read or isn't valid JSON.
fn read_input() -> Result<Value, String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| format!("could not read the hook input: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("hook input is not valid JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_runs_need_the_user() {
        for command in [
            "PICAM_RS_HARDWARE=cooling cargo test -p picam-rs --test hardware_pixis -- --ignored --exact cooling",
            "export PICAM_RS_HARDWARE=cooling",
            "$env:PICAM_RS_HARDWARE=\"cooling\"",
            "cargo test -- --include-ignored",
            "cd x && cargo test -p picam-rs -- --ignored",
            "target/debug/deps/hardware_pixis-0123.exe --ignored",
            r"target\debug\deps\hardware_pixis-0123.exe",
            "$env:PICAM_RS_HARDWARE = 'cooling'; cargo test",
            "setx PICAM_RS_HARDWARE cooling",
            "cargo nextest run -p picam-rs --run-ignored only",
            "cargo test -p picam-rs -- '--ignored'",
            "grep -n hardware_ tests/hardware_pixis.rs",
            "$env:picam_rs_hardware = 'cooling'",
        ] {
            assert!(hardware_run(command).is_some(), "should ask: {command}");
        }
    }

    #[test]
    fn other_commands_run_freely() {
        for command in [
            "cargo test -p picam-rs",
            "cargo test -p picam-rs --test errors",
            "cargo run -p xtask -- gate",
            "git commit -m 'added hardware tests'",
        ] {
            assert!(hardware_run(command).is_none(), "shouldn't ask: {command}");
        }
    }
}
