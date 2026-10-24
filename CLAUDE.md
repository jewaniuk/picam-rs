# CLAUDE.md

Instructions for agents working on `picam-rs`. This file says **how** to work: the workflow, when to stop and ask, and how to report. [`DESIGN.md`](DESIGN.md) says **what** to build: the design, the patterns and the roadmap.

**Read `DESIGN.md` in full before doing anything else, every session.** Where this file, `DESIGN.md` and the code disagree, stop and ask.

## The project

`picam-rs` is a safe, idiomatic Rust wrapper around the Teledyne Princeton Instruments PICam C API (SDK 5.15), which controls their scientific cameras. The goal is to wrap all 199 API functions. The raw bindgen output lives in the private `sys` module, and no `sys` type may ever appear in the public API.

This repository is **public**. Never commit PICam SDK files (headers, libraries, DLLs), text or excerpts from the PICam manual, or anything about calibrating a specific instrument.

## Commands

| What | Command |
|---|---|
| build | `cargo build -p picam-rs` |
| all tests (needs the SDK installed) | `cargo test -p picam-rs` |
| format | `cargo fmt --all` |
| the gate, which must pass before pushing | `cargo run -p xtask -- gate` (format check, `cargo clippy -p picam-rs --all-targets -- -D warnings -A dead_code`, then the tests) |
| progress | `cargo run -p xtask -- progress` (add `--list` for every unwrapped function) |
| public API listing | `cargo public-api --package picam-rs` |

Hooks in `.claude/settings.json` format every Rust file you edit, and run the gate before every `git push` and `gh pr create`, blocking either one if it fails. They also block pushes to `main`, force pushes and `gh pr merge`. When a hook blocks a command, read its message and fix the cause. Never try to get around a hook.

## Workflow for each chunk

Work is done one **chunk** at a time: one coherent group of roughly 10 to 20 functions, or one roadmap bullet. The aim is a PR the maintainer can review in about 15 minutes. If a roadmap bullet is much bigger than that, propose splitting it.

1. **Plan, in plan mode.** Research without editing anything, then present a plan covering:
   - which functions or roadmap items the chunk covers
   - the public items it will add or change, with their signatures
   - which reference implementations (below) each piece follows, and any piece that follows none
   - any new `unsafe` pattern
   - the tests it will add
   - any stop-and-ask rule it is likely to hit, and open questions
2. **Wait for approval.** The maintainer approves, redirects or rejects the plan. Don't start until it's approved.
3. **Branch.** Create a branch from an up-to-date `main`, named `stage-<n>/<short-topic>`, e.g. `stage-1/seal-value-traits`.
4. **Implement** exactly the approved plan, committing as you go.
5. **Test and check.** Run the gate and the progress tool. Fix what fails, within the rules below.
6. **Open a PR** against `main` with `gh pr create`, using the [report format](#report-format) as its description.
7. **Stop.** Report the PR link and end. Don't start the next chunk.

Stopping to ask is always acceptable and never counts as failure. Guessing past one of the rules below does.

## Stop-and-ask rules

Stop and ask the maintainer **before**:

1. changing anything `DESIGN.md` marks as settled, or changing an existing public signature or type
2. adding a public type, trait or module that the approved plan doesn't mention
3. adding or changing a dependency
4. editing `build.rs`, beyond adding `newtype_enum`/`bitfield_enum` lines
5. writing an `unsafe` pattern not already used in the crate, or any `unsafe impl Send` or `unsafe impl Sync`
6. touching acquisition or callbacks at all
7. using a new pattern because the existing ones don't fit cleanly
8. assuming PICam behaviour that neither the manual nor a demo-camera test settles
9. changing a test's expectations to make it pass
10. adding any `#[allow(...)]`
11. editing `CLAUDE.md`, the hook or settings files in `.claude/`, or `DESIGN.md`'s settled sections
12. going beyond the approved plan
13. deleting files, rewriting git history, or force-pushing

When you stop, say which rule applies, describe the problem, list the options with your recommendation, and wait for an answer.

The maintainer writes acquisition, callbacks and any `Send`/`Sync` decision personally. For those, your job is to research and explain, not to write the code.

## Coding standards

`DESIGN.md` is the full standard. The points most often missed:

- every raw call goes through `picam_call!`
- PICam-allocated memory: copy into owned types, then call the matching `Picam_Destroy*`, and only on a non-null pointer
- enums whose values come from C: a `newtype_enum` line in `build.rs` plus a `define_picam_enum!` wrapper, never a raw Rust enum
- public types are re-exported from `lib.rs`; modules stay private
- every public item and field has a `///` doc, every fallible public function has `# Errors`, every `unsafe fn` has `# Safety`, and every `unsafe` block has a `// SAFETY:` comment
- inline `//` comments are lowercase with minimal punctuation
- never add `#![allow(dead_code)]`, because the progress tool depends on those warnings

### Reference implementations

Copy these instead of inventing something new:

| Situation | Follow |
|---|---|
| PICam allocates an array | `Camera::parameters` in `src/camera.rs` |
| PICam allocates a single struct | `Camera::collection_constraint` in `src/camera.rs` |
| the same wrapper repeated for several types | `impl_picam_value!` in `src/parameter.rs` |
| passing an array into PICam | `<Vec<Roi> as PicamValue>::set` in `src/parameter.rs` |
| a public enum | `define_picam_enum!` in `src/convert.rs` |
| an integration test | `tests/camera.rs`, using `tests/common/mod.rs` |

## Testing

Follow `DESIGN.md`'s Testing section. In short:

- every integration test that touches PICam starts with `common::library()` or `common::lock()`
- each test uses its own demo-camera serial number
- every file in `tests/` starts with a `//!` doc comment
- drop things part-way through a test explicitly with `drop(...)`, and name values that must live to the end `_name`, never `_`

Every chunk adds tests for what it wraps where a demo camera allows it. When something can't be tested on a demo camera, say so in the report and add it to the README's "to test" list.

## Reference material

The PICam Programmer's Manual and other reference material live in `reference/`, which is gitignored and exists only on the lab computer. `reference/README.md` explains what is there and how to search it. Search for the section you need rather than reading whole documents. Cite the manual section in code comments and reports when it settles a behaviour, but never copy its text into the repository.

## Git and GitHub

- Never commit or push to `main`. All work goes through a branch and a PR.
- Never merge a PR. The maintainer merges with "Squash and merge" and writes the final commit message.
- Commit messages are lowercase and past tense, describing what changed and why, matching `git log`.
- PR titles are a short lowercase summary of the chunk.

## Updating docs without asking

You may make these changes without asking, as long as the report mentions each one:

- tick roadmap boxes in `DESIGN.md` and update its "functions wrapped" count
- add to `DESIGN.md`'s open questions
- add items to the README's "to test" list
- update the status column of `DESIGN.md`'s module map

Anything else in `DESIGN.md` falls under stop-and-ask rule 11.

## Report format

The PR description, with these sections in order:

1. **Summary:** two or three sentences.
2. **Progress:** functions wrapped before and after, from `cargo run -p xtask -- progress`.
3. **Public API changes:** the difference in `cargo public-api --package picam-rs` output between `main` and the branch. If the tool isn't available, say so and list the changes by hand.
4. **New `unsafe` blocks:** file and line for each, with one line on why it is sound.
5. **Deviations:** anything that doesn't follow `DESIGN.md` or the reference implementations exactly, and why.
6. **Tests:** what was added, the results, and what couldn't be tested and why.
7. **Assumptions to check on hardware:** each one is also added to the README's "to test" list.
8. **Open questions.**

Write "none" under a section rather than leaving it out.
