# Contributing

`picam-rs` is developed one reviewable chunk at a time, mostly by an LLM agent (Claude Code) working to rules in this repository, with the maintainer approving each plan and reviewing each PR. This file covers setting up a computer for that work. [`CLAUDE.md`](CLAUDE.md) says how to work, and [`DESIGN.md`](DESIGN.md) what to build. Read both before changing anything.

This repository is public. Never commit PICam SDK files (headers, libraries, DLLs), text from the PICam manual, hardware test output, or anything about calibrating a specific instrument.

## Setup

**Windows** (x86-64) builds, tests and runs the crate:

- **Rust**, from <https://rustup.rs>. The minimum version is the `rust-version` in `Cargo.toml`.
- **The PICam SDK.** The build links `Picam.lib` from `C:\Program Files\Princeton Instruments\PICam\Libraries`. For an SDK elsewhere, set `PICAM_DIR` to its `Includes` directory, and the build uses the `Libraries` directory beside it.
- **Pleora eBUS on `PATH`** (`C:\Program Files\Common Files\Pleora\eBUS SDK`), which PICam loads when it initializes. The SDK installer doesn't add it. Add it to your user `PATH` through "Edit environment variables for your account" rather than `setx`, which can truncate a long `PATH`, then open a new terminal. `which PvBase64.dll` in Git Bash should then find it.
- **LLVM** (`choco install llvm`), only for `xtask generate` and the gate, which run bindgen. Building the crate doesn't need it, since the bindings are committed.
- **`cargo-public-api`**, for the PR report's public API section: `cargo install cargo-public-api --locked` and `rustup toolchain install nightly --profile minimal`.

**macOS** can only `cargo check`, since there's nothing to link. To run `xtask generate` there, copy the SDK headers into a gitignored `vendor/` and set `PICAM_DIR` in a gitignored `.cargo/config.toml`.

If a test crashes at startup, see [Troubleshooting](README.md#troubleshooting).

## First checks

On each new computer or SDK version, run these in order:

1. `cargo test -p picam-rs --test library`. The important test is `reinitialize_after_drop`: the test helper relies on PICam allowing a second initialization in one process. If it fails, switch to the fallback in `DESIGN.md`'s Testing section before writing any tests.
2. `cargo test -p picam-rs --test camera`, a demo camera from start to finish.
3. `cargo run -p xtask -- gate`, which should pass. Turn real cameras off first: with one on, PICam's limit on demo cameras is lower, and `demo_camera_limit` counts it.

## Commands

| What | Command |
|---|---|
| build | `cargo build -p picam-rs` |
| all tests (demo cameras, no hardware) | `cargo test -p picam-rs` |
| the gate, which must pass before pushing | `cargo run -p xtask -- gate` |
| functions and enums wrapped | `cargo run -p xtask -- progress` |
| regenerate the bindings from the headers | `cargo run -p xtask -- generate` |
| public API listing | `cargo public-api --package picam-rs` |

`CLAUDE.md` has the full list.

## The agent workflow

- **`CLAUDE.md`** gives the agent its workflow (plan, wait for approval, branch, implement, gate, review, PR, stop), the rules on when to stop and ask, and the PR report format.
- **`DESIGN.md`** records the design. Sections marked settled change only with the maintainer's approval.
- **Hooks** in `.claude/settings.json`, implemented in `xtask/src/hook.rs`, format each Rust file the agent edits and run the gate before every `git push` and `gh pr create`. They also block pushes to `main`, force pushes and `gh pr merge`, and ask the maintainer before any command that runs a hardware test.
- **The `reviewer` agent** (`.claude/agents/reviewer.md`) reviews each chunk before its PR is opened.
- **Merging** is the maintainer's: each PR is one chunk, squash-merged.

To set up Claude Code:

1. Run `claude` in the repository and trust the folder when asked. The hooks stay off until you do.
2. Check that `/hooks` lists both hooks (`PreToolUse` for `Bash|PowerShell`, `PostToolUse` for `Edit|Write`) and that `/agents` lists `reviewer`.
3. Test the guards: on `main`, ask Claude to `git push`. The hook should refuse it.

Give the agent a GitHub token that can only reach this repository: a fine-grained token with read and write access to Contents and Pull requests. Sign `gh` in with it (`gh auth login`), and set only this clone to push through `gh` (`git config --local credential.https://github.com.helper '!gh auth git-credential'`, after clearing any inherited helper with an empty one).

## The PICam manual

Agents search the PICam Programmer's Manual rather than reading it whole. It's Teledyne's copyrighted material, so it lives only in `reference/`, which is gitignored.

1. Copy the manual's PDF into `reference/`. It usually installs with the SDK, next to `Includes`.
2. Ask Claude to convert it to text, for example:

   > Convert the PICam manual PDF in `reference/` into plain text agents can search. Use a Python PDF library (such as pymupdf) and write one `.txt` file per chapter into `reference/manual/`, named by chapter number and title, with page numbers marked in the text as `[page N]`. Keep function signatures, tables and code examples intact. Then write `reference/README.md`: what each file contains, with its chapter and page range, and how to search them. Don't commit anything in `reference/`.

3. Check that `git status` shows nothing new, and that searching `reference/manual/` for `Picam_Acquire` finds it.

Code comments and PRs may cite a manual section, but never copy its text.

## Hardware tests

`tests/hardware_*.rs` drive real devices, and [`HARDWARE_TESTS.md`](HARDWARE_TESTS.md) lists what each checks and the results so far. Each test is `#[ignore]`d and runs only when `PICAM_RS_HARDWARE` names it. An agent may run one only after quoting its doc comment and getting the maintainer's yes for that run. Never commit the output: it holds serial numbers.

If you check an item on your own hardware, please [open an issue](https://github.com/jewaniuk/picam-rs/issues/new?template=hardware-report.md) with what you saw.
