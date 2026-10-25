# picam-rs design record

This file records the design decisions, conventions and roadmap for `picam-rs`. It is written for anyone picking up the project, human or agent, and should be read in full before writing code. Where this file and the code disagree, stop and ask rather than picking one.

Decisions marked **settled** were made deliberately and should not be changed without the maintainer's approval. Items under [Open questions](#open-questions) are not decided yet.

## Purpose and scope

`picam-rs` is a safe, idiomatic Rust wrapper around the Teledyne Princeton Instruments PICam C API, the SDK used to control their scientific cameras.

**Settled:**

- The crate wraps the **entire** PICam C API: every function in `picam.h`, `picam_advanced.h`, `picam_accessory.h`, `picam_em_calibration.h` and `picam_special.h` (199 functions in SDK 5.15). Functions are not skipped because they seem unlikely to be needed.
- The crate exposes what PICam exposes and nothing more. Anything specific to a particular instrument built around a camera (spectrometer gratings, slits, wavelength or intensity calibration, and so on) is out of scope and belongs in downstream code, not here.
- Like `tlpm-rs`, the crate will be published with a clear notice that it is experimental and that much of it has not been tested against real hardware.

## Licensing and distribution

**Settled:**

- Licensed `MIT OR Apache-2.0`.
- The PICam SDK's headers, `.lib` files and DLLs are **never** committed or published. Users install the SDK themselves. `vendor/` (headers for local development) is gitignored for this reason.
- Publishing to crates.io was checked and is fine, on the condition that no SDK files and no instrument-calibration material ever enter this repository.

## Development environment

The crate only runs on Windows, the only platform the lab uses. The SDK also ships a Linux build (CentOS 7), but it is not a target.

| Machine | What works | Header location |
|---|---|---|
| macOS dev laptop | `cargo check` only (no linking) | `vendor/`, via `PICAM_DIR` set in `.cargo/config.toml` |
| Windows lab computers | `cargo build`, `cargo test`, demo cameras, real hardware | default SDK install path |

- [`LAB_SETUP.md`](LAB_SETUP.md) is the checklist for preparing a lab computer for development.
- `build.rs` reads the headers from `PICAM_DIR`, falling back to `C:\Program Files\Princeton Instruments\PICam\Includes`, and links `Picam.lib` from the sibling `Libraries` directory on Windows only.
- `.cargo/` and `vendor/` are gitignored, so a fresh clone on a lab computer uses the default path with no setup.
- bindgen needs LLVM/libclang installed (`choco install llvm`).
- Bindings are generated for `--target=x86_64-pc-windows-msvc` even on macOS, so type sizes match the real target.
- The bindings were generated against **PICam SDK 5.15.11.2503 (March 2025)**. Check that the lab computer's installed SDK matches before relying on behaviour.

## Architecture

### Module map

| Module | Role | Status |
|---|---|---|
| `sys` | raw bindgen output, private, never exposed | done |
| `error` | `Error`, `Result`, `picam_call!` | done |
| `library` | `Library` (initialize/uninitialize lifecycle) | done |
| `camera` | `Camera`, `CameraId`, `CameraHandle`, discovery, demo cameras, parameter/constraint methods | done apart from acquisition |
| `parameter` | `Parameter`, value traits, `Roi`/`Pulse`/`Modulation`, parameter info queries | done, enum refactor in progress |
| `constraint` | constraint structs and their enums | done, enum refactor in progress |
| `convert` | shared FFI helpers (string/array conversion, `define_picam_enum!`, `FirmwareDetail`) | done |
| acquisition | `Picam_Acquire`, start/stop, waiting for updates | not started |
| advanced | `picam_advanced.h` | not started |
| accessory | `picam_accessory.h` | not started |
| EM calibration | `picam_em_calibration.h` | not started |
| special | `picam_special.h` | not started |

### Single crate, private `sys` module (settled)

There is no separate `picam-sys` crate. The raw bindings live in the private `sys` module (`src/sys.rs`, which `include!`s bindgen's output). Users of the crate must never need to touch, import or even see a `sys` type. See [Public API rules](#public-api-rules).

### Enums and undefined behaviour (settled)

A Rust `enum` holding a discriminant it doesn't declare is undefined behaviour. PICam can return enum values the bindings don't know about, for example when the installed runtime is newer than the SDK used to generate them. So:

- In `build.rs`, bindgen generates real Rust enums by default, but any enum whose values **come back from C** is generated as a newtype (`.newtype_enum(...)`): a struct wrapping a plain integer, where any value is valid. Add new entries there whenever a new such enum is wrapped. Bitmask enums use `.bitfield_enum(...)`.
- Public-facing enums are defined with `define_picam_enum!` (in `src/convert.rs`). Each generated enum has an `Unknown(i32)` variant and `pub(crate)` `from_raw`/`to_raw` conversions. `Parameter` uses its own macro, `define_parameters!`, for the same reason.
- `Error::from_raw` follows the same rule and maps unrecognized codes to `Error::Unknown(i32)`.

### Errors (settled)

- `Error` is a `thiserror` enum with one variant per `PicamError` code, plus `Unknown(i32)` and a few crate-side variants (`ValueTooLong`, `InvalidString`).
- Every raw call goes through `picam_call!` (in `src/error.rs`), which wraps the call in `unsafe` and turns the return code into a `Result<()>`. The macro makes the return value safe to handle, not the call itself; the caller is still responsible for passing valid pointers.

### Library lifecycle (settled)

- `Library::new()` calls `Picam_InitializeLibrary`; `Drop` calls `Picam_UninitializeLibrary` and logs any failure, since `Drop` can't return one.
- Cameras hold an `Arc<Library>`, so many cameras share one library and the library outlives them all. This was chosen to fit the downstream server's design.

### Cameras and thread safety (settled)

- `Camera` owns a `PicamHandle` and closes it on `Drop`.
- `Camera` is deliberately **not** `Send` or `Sync`. This falls out naturally from the raw-pointer handle and must stay that way: downstream code decides how to manage thread safety (the same choice as `tlpm-rs`'s `PowerMeter`). Never add `unsafe impl Send/Sync` without the maintainer's approval.
- `CameraId` identifies a camera whether or not it is open. It is spelled `CameraId`, not `CameraID`, following Rust naming conventions.
- `CameraHandle<'a>` is a non-owning view of a handle that is open somewhere in the process, returned by `Camera::list_open_cameras`. It never closes the handle, and its lifetime ties it to the `Library`. A call on a handle that has since been closed should fail with `Error::InvalidHandle`, which is listed in the README's "to test" list to confirm on hardware.
- Method names say what they do. For example, `list_open_cameras` (not `open_cameras`) so it isn't mistaken for a way to open cameras.
- Demo cameras: `CameraId::connect_demo` creates one and `CameraId::disconnect_demo` removes it; `Camera::available_demo_models` lists the models.

### Parameters (settled)

- `Camera::get::<T>()`, `set`, `default_value` and `can_set` are generic over the `PicamValue` trait. It is implemented for `i32` (`piint`), `i64` (`pi64s`), `f64` (`piflt`), `Vec<Roi>`, `Vec<Modulation>` and `Pulse`.
- `ReadablePicamValue` (`Camera::read`) and `OnlineSettablePicamValue` (`Camera::set_online`) are sub-traits implemented only for the types PICam supports those operations for.
- The value traits are sealed. Their methods that take a handle live on supertraits in the private `parameter::sealed` module (`RawPicamValue`, `RawReadablePicamValue`, `RawOnlineSettablePicamValue`) and take a `RawHandle`, a wrapper around the raw handle that only the crate can construct. So users can neither implement the traits nor call those methods. A plain sealed supertrait (a `pub trait` in a private module) is not enough on its own, because its methods can still be called through a generic bound (`T::get(ptr, �)` with `T: PicamValue`), and the raw handle is just `*mut c_void`. A truly `pub(crate)` supertrait would need `#[allow(private_bounds)]`. Any future public trait whose methods need a PICam handle (accessory parameters, for example) follows the same pattern.
- Changes made with `set` are pending until `Camera::commit_parameters`, which returns the parameters that failed validation (an empty `Vec` means all were accepted).
- Repetitive per-type wrappers are generated by macros (`impl_picam_value!`, `impl_bool_query!`, `impl_parameter_info!`, `impl_readable_value!`, `impl_online_settable_scalar!`). Macros live in the module that uses them; `convert.rs` holds the ones shared across modules.

### Memory that PICam allocates (settled)

Many PICam functions return arrays or structs that PICam allocated and must free. The rule:

1. Copy the data into owned Rust types immediately.
2. Then call the matching `Picam_Destroy*` function on PICam's pointer.
3. Never hand a PICam-owned pointer to the user.

Null pointers:

- A null **array** pointer means "nothing to report" and becomes an empty `Vec`. `collect_raw_array` (in `src/convert.rs`) does this; use it for every PICam-allocated array.
- A null pointer where a **single struct** was expected becomes `Error::UnexpectedNullPointer`.
- Only call `Picam_Destroy*` when the pointer is non-null.

When the user passes arrays **into** PICam (for example in `Vec<Roi>::set`), build the raw `Vec` first and keep it alive and unmodified until the call returns. The existing comments in `parameter.rs` explain why.

## Public API rules

**Settled:**

- No `sys` type may appear in a public signature or a public field. `sys` is private, so users can't even name those types. Use the `define_picam_enum!` wrappers and Rust primitives (`i32`, `i64`, `f64`, `bool`) instead of `sys::piint`, `sys::pi64s`, `sys::piflt` or `sys::pibln`. This is not fully true yet; see the [roadmap](#roadmap).
- Every public type is re-exported from `lib.rs`. Modules themselves stay private.
- Follow Rust API conventions (the Rust API Guidelines) wherever possible: idiomatic names, `Result` for anything fallible, owned return types, and RAII for resources.

## Documentation standards

**Settled:**

- Use standard rustdoc conventions. Every public item and every public struct field gets a `///` doc comment, starting with a one-line summary sentence.
- Every fallible public function has a `# Errors` section.
  - When the manual or the code makes clear which errors can occur, name them with intra-doc links.
  - When it is not clear, write "Returns an error if PICam is unable to …" rather than guessing. A final pass through the PICam manual to fill in these gaps is planned.
- Every `unsafe fn` has a `# Safety` section saying what the caller must guarantee.
- Every `unsafe` block is preceded by a `// SAFETY:` comment explaining why it is sound.
- Inline (`//`) comments are lowercase with minimal punctuation, matching the existing code.
- `pub(crate)` helpers get at least a short doc comment when their purpose isn't obvious from the name.
- When PICam behaviour is unclear, the PICam Programmer's Manual is the source of truth. Note in the code when an assumption has been checked against it.

## Testing

**Settled:**

- **Three tiers:**
  1. Pure-Rust unit tests (enum round trips, error mapping, conversions) live next to the code in `#[cfg(test)]` modules and never touch PICam.
  2. Integration tests in `tests/` run against the real PICam runtime using **demo cameras** (`CameraId::connect_demo`), so they need a Windows machine with the SDK installed but no hardware.
  3. Real-hardware tests are only ever run by the maintainer, and are gated behind `#[ignore]`.
- **One test at a time, a fresh library per test.** PICam can only be initialized once per process at a time, and the tests in one `tests/*.rs` file run in parallel threads of one process. So every integration test that touches PICam starts by calling `common::library()` (a fresh `Library` held under a global lock) or `common::lock()` (the lock alone, for tests that create `Library` themselves). Both are in `tests/common/mod.rs`. Never call `Library::new()` in a test without holding the lock.
  - This relies on PICam allowing re-initialization within one process, which `tests/library.rs::reinitialize_after_drop` checks. It passed on a lab computer on 2026-10-24. **If it ever fails** (for example after an SDK update), switch to the fallback: one shared `Arc<Library>` per test binary, kept in a `static OnceLock`, with tests written so they don't depend on each other's PICam state.
- Each test uses its own demo-camera serial number, so demo cameras never collide. PICam appends `:Demo` to the serial number of a demo camera (`SMOKE0001` comes back as `SMOKE0001:Demo`, seen with SDK 5.15), so compare against the `CameraId` that `connect_demo` returns rather than the string passed in.
- Each file in `tests/` starts with a `//!` doc comment saying what it tests (`missing_docs` requires it).
- When a test drops a camera or library part-way through, it does so explicitly with `drop(...)`. Variables that must live until the end of a test are named `_name`, never `_`, because `let _ = ...` drops the value immediately.

## Progress tracking

- `cargo run -p xtask -- progress` counts `dead_code` warnings from `cargo check` and reports how many of the 199 header functions are wired into the safe layer, overall and per header. Add `--list` to print every function still unwrapped. It is plain Rust (the `xtask` workspace member, never published), so it runs the same on macOS and Windows. Because the tool relies on these warnings, the crate deliberately has **no** `#![allow(dead_code)]`; don't add one.
- The README's "to test" list collects behaviour that can only be confirmed on real hardware or with the SDK running. Add to it whenever an assumption can't be checked from the manual.

## Roadmap

As of 2026-10-24: 82 / 199 functions wrapped.

### 1. Finish the `sys`-type refactor (in progress)

- [x] `define_picam_enum!` and wrappers for `ComputerInterface`, `ValueType`, `ValueAccess`, `ConstraintType`, `ConstraintScope`, `ConstraintCategory`
- [ ] wrappers for `PicamEnumeratedType`, `PicamConstraintSeverity`, `PicamRoisConstraintRulesMask` (bitmask)
- [ ] decide on and implement a public `Model` type (see open questions)
- [ ] replace `sys` enums in public signatures and fields with the wrappers (`CameraId`, `Camera::value_type` and friends, the constraint `category` arguments, the constraint struct fields, `available_demo_models`)
- [ ] replace `sys::piint`/`pi64s`/`piflt` with `i32`/`i64`/`f64` in public items
- [x] re-export `CameraId` and `CameraHandle` from `lib.rs`
- [ ] re-export `FirmwareDetail`, the existing `define_picam_enum!` enums (`ComputerInterface`, `ValueType`, `ValueAccess`, `ConstraintType`, `ConstraintScope`, `ConstraintCategory`) and the new ones from `lib.rs`
- [ ] doc comments on the enums generated by `define_picam_enum!` and their variants, and on all public struct fields
- [x] seal the value traits (`PicamValue`, `ReadablePicamValue`, `OnlineSettablePicamValue`). Their methods are currently public, safe and take a raw `PicamHandle`, so callers can pass PICam an arbitrary pointer from safe code, and `cargo clippy` fails with `not_unsafe_ptr_arg_deref`. Use the sealed-trait pattern: the public traits stay usable as bounds (`camera.get::<f64>(…)` is unchanged), and the methods that take a handle move to a crate-private supertrait that users can neither call nor implement.
- [x] clear every lint in `Cargo.toml`'s `[lints]` table: all `missing_docs` (including the `Error` variants, the crate-level `//!` doc in `lib.rs` and a `//!` doc for `build.rs`), the `# Errors` sections on the trait methods, `Debug` for `Camera` and `Library`, and the stray blank line after the doc comment on `OnlineSettablePicamValue::set_online`
- [x] once clean, `cargo clippy -p picam-rs -- -D warnings -A dead_code` passes. This is the same check the push/PR gate runs.

### 2. Remaining `picam.h` (10 functions)

- quick wins: `Picam_GetVersion`, `Picam_IsLibraryInitialized`, `Picam_GetEnumerationString` + `Picam_DestroyString`, `Picam_DestroyCalibrations`
- acquisition: `Picam_Acquire`, `Picam_StartAcquisition`, `Picam_StopAcquisition`, `Picam_IsAcquisitionRunning`, `Picam_WaitForAcquisitionUpdate`. This is the most important remaining piece and needs careful design: who owns the frame buffers, and how data is copied out.

### 3. `picam_advanced.h` (76 functions)

Callback registration (`Register*`/`Unregister*` pairs), parameter validation, camera devices, defect maps, user-supplied acquisition buffers and camera discovery. It is very repetitive and well suited to macros, but callbacks across the FFI boundary are the hardest part of the crate to get right and should be designed with the maintainer.

### 4. `picam_accessory.h` (18), `picam_em_calibration.h` (11), `picam_special.h` (2)

These largely mirror patterns already in `camera.rs`.

### 5. Finishing

- a pass through the manual to fill in `# Errors` sections
- a crate-level README with an experimental notice
- hardware testing from the README's "to test" list, then publishing

## Open questions

- **`Model`:** PICam defines 300+ camera models. Hand-writing a full `define_picam_enum!` list was explicitly deferred. Options include a public newtype with `Display` through `Picam_GetEnumerationString`, or generating the variant list.
- **Intra-doc link paths:** the code currently mixes `crate::Error::X` and `crate::error::Error::X`. Since `error` is a private module, the public path `crate::Error::X` is probably the right standard.
- **Functions that need an initialized library but don't take one:** `Library`'s docs say you can't do anything without a live `Library`, but `Camera::available_ids`, `Camera::unavailable_ids`, `Camera::available_demo_models` and `CameraId::connect_demo` take no `&Library`. They fail at run time with `Error::LibraryNotInitialized` instead of being prevented at compile time. `Camera::list_open_cameras` does take `&Library`. Decide whether all of these should take `&Library` for consistency.
- **Acronyms in public names:** some public names copy PICam's capitalization rather than Rust's convention of treating acronyms as words (`CameraId`, not `CameraID`). Examples: `Error::InvalidCameraID`, `Error::InvalidAccessoryID`, `Parameter::ApplicableStarDefectMapID`, `Parameter::EMIccdGain`, `Parameter::AdcEMGain`. Decide whether to rename them (`InvalidCameraId`, `EmIccdGain`, and so on) before publishing, since renaming afterwards is a breaking change. `SeNsRWindowHeight` is a product name (SeNsR) and is a separate case.
- **Acquisition API design:** blocking versus non-blocking, buffer ownership, and how frames are handed to the user.
- **Callback design** for `picam_advanced.h`.
