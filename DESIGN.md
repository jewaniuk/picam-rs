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
| `library` | `Library` (initialize/uninitialize lifecycle, the initialized check), `Version` | done |
| `camera` | `Camera`, `CameraId`, `CameraHandle`, `HandleType`, discovery, demo cameras, opening through the device call (`open_device`), listing open model and device handles, the camera-only parameter methods (`set_online`, the ROI, pulse and modulation constraints) and residual erasure | done |
| `camera_device` | `CameraDevice`, the borrowed view of a camera's device handle, and committing to and refreshing from the device | done |
| `parameter` | `Parameter`, `TypedParameter` and the generated `params` handles, value traits, `Roi`/`Pulse`/`Modulation`, parameter info queries | done |
| `constraint` | constraint structs and their enums | done |
| `model` | `Model`, with a constant per camera or accessory model from `src/generated/models.rs` | done |
| `enums` | the 39 generated value enums of PICam's enum parameters, from `src/generated/enums.rs` | done |
| `device` | `impl_parameter_methods!`, the parameter methods `Camera`, `CameraDevice` and `Accessory` share | done |
| `convert` | shared FFI helpers (string/array conversion, `define_picam_enum!`, `FirmwareDetail`, enum names through `Picam_GetEnumerationString`) | done |
| `acquisition` | `Camera::acquire`, `Camera::start_acquisition` and the `Acquisition` guard, the borrowed data views (`Readouts`, `Readout`, `Frame`, `RoiData`, `Pixels`), `AcquisitionErrors`, user acquisition buffers, overrun, clearing the readout count online, and acquisition state watches (`AcquisitionState`, `AcquisitionStateErrors`, `AcquisitionStateUpdate`) | done |
| `validation` | `ValidationResult`, `ValidationResults`, `DependentValidationResult`, `FailedDependentParameter`, and `Camera`'s validation methods | done |
| `defect` | `PixelDefectMap`, `StarDefectMap` and their parts, and `Camera`'s defect map methods | done |
| `watch` | `Watch` and the callback machinery: the registry, the trampolines (parameter, dependent constraint, status and discovery callbacks), and the `watch_*` methods' halves | done |
| `discovery` | `DiscoveryAction`, `CameraDiscovery`, `AccessoryDiscovery`, and the discovery methods of `Camera` and `Accessory` | done |
| `user_state` | PICam's user state as a `usize` tag, behind the `user_state` methods of `Camera`, `Accessory` and their handle views | done |
| advanced | the rest of `picam_advanced.h`: the acquisition-updated callback | not started |
| `accessory` | `Accessory`, `AccessoryId`, `AccessoryHandle`, `CalibrationPoint`: identifying, opening and querying accessories, the light source reference, and the parameter methods shared with `Camera` | done |
| `em_calibration` | `EmCalibration`, `EmCalibrationHandle`, `EmCalibrationDate`: opening a camera for EM gain calibration, its sensor temperature, and `calibrate` | done |
| special | `picam_special.h`: residual erasure, as `Camera` methods in `camera` | done |

### Single crate, private `sys` module (settled)

There is no separate `picam-sys` crate. The raw bindings live in the private `sys` module (`src/sys.rs`, which `include!`s bindgen's output). Users of the crate must never need to touch, import or even see a `sys` type. See [Public API rules](#public-api-rules).

### Enums and undefined behaviour (settled)

A Rust `enum` holding a discriminant it doesn't declare is undefined behaviour. PICam can return enum values the bindings don't know about, for example when the installed runtime is newer than the SDK used to generate them. So:

- In `build.rs`, bindgen generates real Rust enums by default, but any enum whose values **come back from C** is generated as a newtype (`.newtype_enum(...)`): a struct wrapping a plain integer, where any value is valid. Add new entries there whenever a new such enum is wrapped. Bitmask enums use `.bitfield_enum(...)`.
- Public-facing enums are defined with `define_picam_enum!` (in `src/convert.rs`). Each generated enum has an `Unknown(i32)` variant, `pub(crate)` `from_raw`/`to_raw` conversions and a `name(&Library)` method. `Parameter` uses its own macro, `define_parameters!`, for the same reason.
- Readable names come from `Picam_GetEnumerationString`, which needs an initialized library, so they are a `name(self, library: &Library) -> Result<String>` method rather than `Display`. Only single-value enums, `Parameter` and `Model` get `name()`:
  - Bitmask types (`RoisConstraintRules` and any later `*Mask` wrapper) rely on `bitflags`, whose `Debug` and `iter_names` already give flag names without a PICam call. The manual doesn't say what `Picam_GetEnumerationString` returns for a combination of flags.
  - `Error` relies on `Display`. Its crate-side variants have no PICam code, and error handling often has no `Library` to pass, for example when `Library::new` itself fails.
- Enums and constants are not `#[non_exhaustive]`. When an SDK update adds values, the crate is updated to match.
- Lists taken straight from the headers are generated, not written by hand. `cargo run -p xtask -- generate` (`xtask/src/generate.rs`) reads the installed headers and writes them into `src/generated/`, which the modules pull in with `include!`. Those files are never edited by hand: change the generator and rerun it. Today that covers `Parameter`, `EnumeratedType`, `Model` and the hardware value enums. Enums whose docs need prose (`Error`, the small `define_picam_enum!` enums, `RoisConstraintRules`, `AcquisitionErrors`, `Dynamics`) stay hand-written, but `generate --check` verifies that each one still names every value its C enum has. The check only looks for each value's name somewhere in the wrapper's source file, so a value used only in a test or comment (such as `PicamRoisConstraintRulesMask_None`, which has no flag) still counts. The gate runs `generate --check`, so a stale generated file or a missing value fails before anything is pushed. After an SDK update, run `generate`, review the diff and fill in any values `--check` reports.
- The value enums of PICam's 44 enum parameters (the 39 from `ActiveShutter` to `VacuumStatus`) are generated into `src/generated/enums.rs`:
  - **Which enum each parameter uses:** the generator matches each enum parameter to its enum by name, with `Mask` added, or with a trailing number dropped (`OutputSignal2` uses `OutputSignal`). `PARAMETER_ENUMS` in `xtask/src/generate.rs` covers the seven that don't match, and `tests/handles.rs` checks the mapping against PICam's `Picam_GetParameterEnumeratedType` on every demo model. An unmatched enum parameter fails generation.
  - **Single-value enums** use `define_picam_enum!`, and their docs name the parameters that use them and each value's PICam constant.
  - **Bitmask enums** (the six named `*Mask`) use `bitflags!` without the `Mask` suffix, like `RoisConstraintRules`. Bits the crate doesn't recognize are kept, and a zero value (`None`, or `Orientation`'s `Normal`) isn't a flag: no flags set means it.
  - The advanced API's enums, including its acquisition ones (`AcquisitionState`, `AcquisitionStateErrorsMask`), wait for stage 4.
- `Error::from_raw` follows the same rule and maps unrecognized codes to `Error::Unknown(i32)`.

### Models (settled)

- `Model` is a newtype over PICam's model code, which identifies cameras and accessories alike (manual 4.1). It has a private field and is not an enum, so any code PICam reports is a valid `Model`.
- Each model in the SDK the crate was generated from has an associated constant (`Model::PIXIS_100F`), generated from the header. Constant names are converted automatically from PICam's names, with a few overrides in `xtask/src/generate.rs` where a product name and model number run together, or where a product name's capitals hide a word boundary (`ProEMHS` → `PRO_EM_HS`).
- PICam's "series" values stand for a model family rather than a single model (manual 4.1). `Model::is_series` reports them, based on the crate's own list (a PICam name ending in `Series`), since PICam has no call that answers it. Its docs say so.
- `Debug` shows the constant name when there is one, and `Model::name(&Library)` gives PICam's readable name.

### Errors (settled)

- `Error` is a `thiserror` enum with one variant per `PicamError` code, plus `Unknown(i32)` and a few crate-side variants (`ValueTooLong`, `InvalidString`, `TooManyDemoCameras`, `EnumeratedTypeMismatch`, `AcquisitionFailed`, `InconsistentDataLayout`).
- Every raw call goes through `picam_call!` (in `src/error.rs`), which wraps the call in `unsafe` and turns the return code into a `Result<()>`. The macro makes the return value safe to handle, not the call itself; the caller is still responsible for passing valid pointers.

### Library lifecycle (settled)

- `Library::new()` calls `Picam_InitializeLibrary`; `Drop` calls `Picam_UninitializeLibrary` and logs any failure, since `Drop` can't return one. Just before, `Drop` clears the discovery watches, which belong to no camera (see [Callbacks](#callbacks-settled), decided with the maintainer on 2026-10-26).
- Cameras hold an `Arc<Library>`, so many cameras share one library and the library outlives them all. This was chosen to fit the downstream server's design.
- With SDK 5.15, connecting a 65th different demo camera (model and serial number pair) in one library session corrupts PICam's heap inside `Picam_ConnectDemoCamera` and kills the process, even if the others were disconnected. Reconnecting a pair already used doesn't count again, and a fresh library resets the count. This was found with demo cameras and reproduced with raw `sys` calls: a new model each time, or one model with a new serial number each time, crashes on the 65th connect, while repeating pairs never does. Safe code must not reach it, so `Library` records the pairs `CameraId::connect_demo` has connected (counting attempts, to be safe), and `connect_demo` returns `Error::TooManyDemoCameras` instead of making a 65th.
- PICam needs an initialized library for every call except `Picam_GetVersion` and `Picam_IsLibraryInitialized` (manual 3.2). So every public function that calls PICam takes `library: &Library` (first, after any `self`), unless its receiver already holds one (`Camera`) or borrows one (`CameraHandle`). Constructors that keep the library (`Camera::open`, `Camera::open_first`) take an `Arc<Library>` instead. Calling PICam too early then fails to compile rather than returning `Error::LibraryNotInitialized`. A function that only needs the proof takes the argument and ignores it (`let _ = library;`). The two exceptions are the associated functions `Library::version()` and `Library::is_initialized()`, which take no `&Library`.

### Cameras and thread safety (settled)

- `Camera` owns a `PicamHandle` and closes it on `Drop`, after clearing the watches on its model and device handles (see [Callbacks](#callbacks-settled)). It is the camera *model* handle `Picam_OpenCamera` gives (manual 4.3.2.4): a copy of the camera's parameters in memory, whose changes reach the physical camera, the *device*, only when committed (manual 2.6.1). `Picam_CloseCamera` closes both handles (manual 4.3.2.6). `Camera::open_device` opens through `PicamAdvanced_OpenCameraDevice` instead and keeps the model handle the same way (decided with the maintainer on 2026-10-26).
- `Camera::device()` gives a `CameraDevice<'_>`, a view of the device handle with the parameter methods `Camera` and `Accessory` share. It borrows the camera, so it can't outlive it, and it has no `Drop`. Setting a value through it also sets it on the model (manual 5.4.1.3) (decided with the maintainer on 2026-10-26). One can be made between an acquisition's updates through `Acquisition::camera`, but PICam then refuses every change through it with `Error::AcquisitionInProgress`, except to the parameters it can set online, the same ones `Camera::set_online` changes (seen on three demo models with SDK 5.15, and checked on the PIXIS 100F by `tests/camera_device.rs`). So it can't change the data layout the acquisition views rely on.
- `Camera` looks its device handle up once when it opens (`Camera::open_device` gets it directly) and keeps it, closing the camera again if the lookup fails. `Picam_CloseCamera` closes both handles, so the device handle needs no cleanup of its own. Keeping it assumes it stays the same for as long as the camera is open, which the manual doesn't say. This is on the README's "to test" list. The `picam_special.h` functions refuse the model handle with `Error::InvalidHandle` (seen on a demo camera with SDK 5.15), so they use it, as do `CameraDevice`, extrinsic watches and clearing watches on close, which therefore can't fail to find it (decided with the maintainer on 2026-10-26, replacing a lookup for each call).
- `Camera` also owns the acquisition buffer it gives PICam, if any (see [Acquisition](#acquisition-settled)). It's a raw allocation, which keeps `Camera` neither `Send` nor `Sync` as before.
- `Camera` is deliberately **not** `Send` or `Sync`. This falls out naturally from the raw-pointer handle and must stay that way: downstream code decides how to manage thread safety (the same choice as `tlpm-rs`'s `PowerMeter`). Never add `unsafe impl Send/Sync` without the maintainer's approval.
- `CameraId` identifies a camera whether or not it is open. It is spelled `CameraId`, not `CameraID`, following Rust naming conventions.
- `CameraHandle<'a>` is a non-owning view of a handle that is open somewhere in the process, returned by `Camera::list_open_cameras` (model handles) and `Camera::list_open_camera_devices` (device handles). `handle_type` says which, and `model` and `device` look up the other. It never closes the handle, and its lifetime ties it to the `Library`. A call on a handle that has since been closed should fail with `Error::InvalidHandle`, which is listed in the README's "to test" list to confirm on hardware.
- Method names say what they do. For example, `list_open_cameras` (not `open_cameras`) so it isn't mistaken for a way to open cameras.
- Demo cameras: `CameraId::connect_demo(&library, model, serial_number)` creates one and `id.disconnect_demo(&library)` removes it; `Camera::available_demo_models(&library)` lists the models.

### Parameters (settled)

- Parameters are typed. Every parameter has a generated handle in the public `params` module, a `TypedParameter<T>` whose `T` comes from the value type in the parameter's `PI_V` definition (`params::EXPOSURE_TIME: TypedParameter<f64>`). Handles can't be made any other way, so none claims the wrong type.
- `Camera` and `Accessory`'s `get`, `set`, `default_value`, `can_set` and `read`, and `Camera::set_online`, take a handle, so a wrong value type, or an operation PICam doesn't support for that type, doesn't compile. Their `_untyped` twins (`get_untyped::<T>(Parameter)` and so on) are the explicit fallback for parameters with no handle, such as `Parameter::Unknown` values from `Camera::parameters`. For an enum type they first check PICam's enumerated type for the parameter and return `Error::EnumeratedTypeMismatch` if it differs, since PICam reads every enum through the same integer call and can't catch that itself. The typed methods skip the check, since a handle's type is already right. The methods that only describe a parameter (`value_type`, the constraints and so on) take `impl Into<Parameter>`, so either works.
- The value methods are generic over the `PicamValue` trait. It is implemented for `i32`, `bool`, `i64`, `f64`, `Vec<Roi>`, `Vec<Modulation>`, `Pulse` and the generated value enums. `bool` and the enums go through PICam's integer calls, `bool` as 0 or 1 (manual 5.1.1.42), and neither is online-settable because PICam only sets integer parameters online (5.4.1.20).
- `ReadablePicamValue` (`Camera::read`) and `OnlineSettablePicamValue` (`Camera::set_online`) are sub-traits implemented only for the types PICam supports those operations for.
- The value traits are sealed. Their methods that take a handle live on supertraits in the private `parameter::sealed` module (`RawPicamValue`, `RawReadablePicamValue`, `RawOnlineSettablePicamValue`) and take a `RawHandle`, a wrapper around the raw handle that only the crate can construct. So users can neither implement the traits nor call those methods. A plain sealed supertrait (a `pub trait` in a private module) is not enough on its own, because its methods can still be called through a generic bound (`T::get(ptr, ...)` with `T: PicamValue`), and the raw handle is just `*mut c_void`. A truly `pub(crate)` supertrait would need `#[allow(private_bounds)]`. Any future public trait whose methods need a PICam handle follows the same pattern.
- Changes made with `set` are pending until `Camera::commit_parameters`, which returns the parameters that failed validation (an empty `Vec` means all were accepted). `Camera::commit_parameters_to_device` commits them through the advanced call, which returns an error instead and leaves the device as it was if it fails (manual 7.4.13.7), and `Camera::refresh_parameter_from_device` and `refresh_parameters_from_device` discard pending changes by copying the device's values back.
- Accessories are configured through the same parameter calls as cameras. `impl_parameter_methods!` (in `src/device.rs`) generates the same inherent methods on `Camera`, `CameraDevice` and `Accessory`: every call `picam_accessory.h` lists for accessories (the value methods and their `_untyped` twins, `read`, the information and commit methods, status waiting, dynamics, the collection and range constraints, one at a time or all at once, and the parameter watches, see [Callbacks](#callbacks-settled)). `set_online` and the ROI, pulse and modulation constraints stay `Camera`-only, since accessories don't support them, and so do validation and the defect maps, which `picam_advanced.h` defines for cameras only. The value methods stay generic over `PicamValue`, so an accessory call with a value type it doesn't use compiles and PICam refuses it.
- Repetitive per-type wrappers are generated by macros (`impl_picam_value!`, `impl_bool_query!`, `impl_parameter_info!`, `impl_readable_value!`, `impl_online_settable_scalar!`, `impl_parameter_methods!`). Macros live in the module that uses them; `convert.rs` holds the small helpers shared across modules, and `device.rs` the parameter methods `Camera` and `Accessory` share.

### Acquisition (settled)

Designed with the maintainer on 2026-10-25, through the design discussion in `CLAUDE.md`. The code lives in `src/acquisition.rs`.

- **Shape:** `Camera::acquire(&mut self, readout_count, timeout)` blocks until every readout has arrived. `Camera::start_acquisition(&mut self)` returns an `Acquisition` guard that borrows the camera, with `wait`, `stop`, `is_running`, `camera()` (for calls between updates) and `finish`.
- **Ending:** PICam requires waiting until an acquisition reports it's no longer running, even after an error or a stop (manual 6.4.5). `finish(self)` stops the acquisition and waits for that, returning any error. Dropping the guard does the same and logs errors. Both give up after `DRAIN_ATTEMPTS` (10) waits of `DRAIN_TIMEOUT` (1 s) in a row bring nothing, or `DRAIN_LIMIT` (10 s) after asking it to stop, rather than hang on a camera that has stopped responding or keeps sending data after a stop failed (decided with the maintainer on 2026-10-27).
- **Data ownership:** zero copy, except from circular memory (see **User buffers** below). `Readouts`, `Readout`, `Frame` and `RoiData` are views of the memory PICam acquired into. Their lifetime ties them to the `&mut` borrow of the camera (for `acquire`) or of the guard (for `wait`), which blocks every call that could invalidate the buffer (manual 6.4.1 and 6.4.5). Copy the data to keep it longer. The views are automatically `Send` and `Sync`, since they hold only shared slices, so scoped threads can process frames in parallel. `Camera` and `Acquisition` stay neither.
- **Layout:** before acquiring, the crate reads the layout parameters (`ReadoutStride`, `FramesPerReadout`, `FrameStride`, `FrameSize`, `PixelFormat`, `PixelBitDepth`, `Rois`) and checks that their parts fit together, returning `Error::InconsistentDataLayout` if not, so the views never slice out of bounds. It refuses uncommitted parameters with `Error::ParametersNotCommitted`, since PICam acquires with the committed values. The layout can't change while an acquisition runs: PICam refuses both setting and committing parameters then, with `Error::AcquisitionInProgress` (seen on a demo camera with SDK 5.15, checked by `tests/acquisition.rs`), so the layout read at the start describes every update. `FrameSize` covers only a frame's pixels, and its metadata runs from there to `FrameStride` (seen on a demo camera with SDK 5.15, consistent with manual figure 6-1).
- **Pixels:** `RoiData::pixels()` returns `Pixels::Mono16` or `Pixels::Mono32` as a `Cow`. It borrows when the buffer is aligned for the pixel type (`<[u8]>::align_to`) and copies otherwise, since PICam doesn't document its alignment.
- **Metadata:** `Frame::metadata()` returns the raw bytes. Decoding them is a later chunk.
- **Timeouts and errors:** timeouts are `Option<Duration>`, rounded up to milliseconds, with `None` waiting forever. A timed-out wait returns `Ok(None)`, and the acquisition carries on. PICam's acquisition errors mask becomes `Error::AcquisitionFailed(AcquisitionErrors)`, and any partial data is dropped.
- **Threads:** an acquisition is stopped from the thread that waits on it. Other threads signal that thread, which calls `stop` between bounded waits. A `Send` stop handle could be added later without breaking anything, but needs the maintainer's decision and a hardware test of PICam's thread safety.
- **User buffers** (designed with the maintainer on 2026-10-27):
  - `Camera::set_acquisition_buffer(len)` allocates `len` zeroed bytes, 8-byte aligned, and gives them to PICam on the device handle (manual 7.4.14.2). A readout count of 0 (until stopped) or one above `ExactReadoutCountMaximum` needs one.
  - The camera keeps the memory until PICam has been given another or none (`clear_acquisition_buffer`, or a length of 0), or the camera has closed. If closing fails, it's leaked rather than freed, since PICam may still write to it.
  - It's a raw allocation rather than a `Box`, since PICam writes to parts of it while the crate holds views of others.
  - Changing it needs `&mut Camera`, so no acquisition or view can be alive. It's also refused with `Error::AcquisitionInProgress` while PICam says an acquisition is running, which only happens after ending one gave up. PICam refuses that too.
  - Views of it are checked to lie inside it, so a misunderstanding of how it wraps gives `Error::InconsistentDataLayout` rather than a read out of bounds.
  - **Circular memory is copied.** PICam fills the buffer circularly, and once it runs out of room it writes over readouts it has already handed out, sets the overrun flag, and fails the next wait with `DataLost` (seen on a demo camera with SDK 5.15). Clearing the readout count online does the same to PICam's own memory, even with no buffer. So whenever the camera has a buffer, or the readout count was cleared online, `Acquisition::wait` copies each update through the raw pointer into a `Vec` the guard reuses, without ever making a reference to PICam's memory. It then checks the overrun flag, and returns `Error::AcquisitionFailed` with `DATA_LOST` rather than a copy PICam may have written over midway. Otherwise views stay zero copy: `acquire` with a buffer needs room for every readout (`InvalidAcquisitionBuffer` otherwise), and PICam's own memory for a counted acquisition holds every readout (both seen with SDK 5.15).
  - **Sizes** (seen with SDK 5.15): PICam takes a buffer of any size, and only refuses one too small when acquiring, with `InvalidAcquisitionBuffer`. `acquire(n)` needs room for `n` readouts, and an acquisition that runs until stopped needs room for 2.
- **Overrun and clearing the readout count:** `Camera::has_acquisition_buffer_overrun` reports PICam's flag, which clears when the next acquisition starts (seen with SDK 5.15). `Acquisition::clear_readout_count_online` makes a running acquisition run until stopped, and `Camera::can_clear_readout_count_online` says whether the camera supports it (manual 7.4.17).

### Callbacks (settled)

Designed with the maintainer on 2026-10-26, through the design discussion in `CLAUDE.md`. The machinery lives in `src/watch.rs`. Parameter, dependent constraint, status, discovery and acquisition state callbacks all go through it, and `Calibrate` passes its closure through PICam's user state instead. Only the acquisition-updated callback is left, for its own design discussion.

- **Delivery through channels:** each `watch_*` method returns a `Watch` guard and an unbounded `mpsc::Receiver` of typed values. PICam's callback, a crate trampoline, only copies the event and sends it, so no user code ever runs inside PICam: no re-entrancy, no panics across FFI, no blocking PICam's thread. The receiver is `Send` and can be read on any thread. Unbounded, so nothing is lost and sending never blocks; unread events only cost memory.
- **Guard:** `Watch` doesn't borrow what it watches, so it can be kept during an acquisition, for example to follow the sensor temperature. It unregisters on drop. A camera or accessory clears every watch on its handles just before it closes (a camera both its model and its device handle): each callback is unregistered with PICam and each sink removed, so the receivers report the channel disconnected. A `Watch` dropped after that does nothing, so it never reaches PICam with a closed handle, or one a later camera has reused. Leaking one with `mem::forget` leaves nothing behind once its camera closes (decided with the maintainer on 2026-10-26, replacing a guard that borrowed the camera). It holds a raw handle, so it is neither `Send` nor `Sync`. `let (_, events) = ...` drops it at once, so the docs say to keep it as `_watch`.
- **Methods:** `watch_value` and `watch_extrinsic_value` take a `params` handle, so the receiver's type comes from the parameter, with `_untyped` twins that check enum types like the other untyped methods. `watch_relevance` and `watch_value_access` take `impl Into<Parameter>`. They're shared methods from `impl_parameter_methods!`, with the per-type halves on the sealed value traits (`RawPicamValue::watch`, `RawReadablePicamValue::watch_extrinsic`), so the public traits gained no methods. PICam refuses watches an accessory doesn't support, as it does for values.
- **Extrinsic watches** go on a device or accessory handle (manual 7.4.9.3), so `Camera` registers them on the device handle it keeps.
- **Registry:** PICam passes callbacks no user pointer, so a global registry maps (handle, parameter, callback kind) to the watches' senders. One PICam registration is shared by every watch on a key, refcounted. Two locks keep it free of deadlocks: the registration lock is held across PICam's register and unregister calls and never taken by a callback, and the sink lock, the only one callbacks take, is never held across a PICam call.
- **Trampolines** always report success to PICam, since the manual doesn't say what PICam does with an error, and copy any data behind a pointer before returning, since the manual doesn't say how long it stays valid. Each callback kind has its own trampoline, the extrinsic ones included, so a trampoline always knows which kind fired.
- **Dependent constraint watches:** `watch_collection_constraint` and `watch_range_constraint` are shared methods, and `watch_rois_constraint`, `watch_pulse_constraint` and `watch_modulations_constraint` are camera-only, like their getters. PICam calls them inside the `set` that changes the constraint (manual 7.4.12.2). The manual calls the constraint passed an array but gives no count, and the header passes one, so one constraint is copied per callback. It equals the `Required` constraint read afterwards (seen with SDK 5.15).
- **Status notifications:** `notify_when_status(parameter, value)` returns a `Watch` and a receiver of one `Result<()>`, then disconnects. It goes on the device or accessory handle, like extrinsic watches (manual 7.4.9.9). PICam calls it only once, so its trampoline takes the key's sinks as it sends. A key with no sinks then has nothing registered with PICam, whatever the count says: the next watch on it registers again, and `release` skips cancelling it.
- **Discovery:** `Camera::watch_discovery`, `start_discovery`, `stop_discovery` and `is_discovering`, and the same on `Accessory`, take `&Library`. Discovery callbacks have no handle or parameter, so they're keyed by a null handle, and `Library`'s `Drop` clears them before uninitializing. Events carry `open_in_this_process` instead of the device handle PICam passes, since a handle view borrows the library and can't travel through a channel. Events only arrive while discovery runs (seen with SDK 5.15).
- **User state:** PICam's per-camera `void*` is offered as a `usize` tag, `user_state` and `set_user_state` on `Camera` and `Accessory`, and `user_state` on their handle views. Neither the crate nor PICam dereferences it, and a camera's model and device share it (manual 7.4.7).
- **Acquisition state watches:** `Camera::watch_acquisition_state(state)` and `can_watch_acquisition_state` go on the device handle (manual 7.4.16). The callback has no parameter, so it's keyed by the state, with parameter 0. Updates come from PICam's acquisition thread. A demo camera says it can detect neither state, yet PICam accepts the watch, and after other acquisitions in the same process it sends `ReadoutEnded` with both counters 0 (seen with SDK 5.15), so the docs point to `can_watch_acquisition_state` (decided with the maintainer on 2026-10-27).
- **`Calibrate`:** `EmCalibration::calibrate` takes a `FnMut(f64) -> bool + Send` closure. It is the one callback that runs user code inside PICam, since `Calibrate` is synchronous and takes a user pointer. The closure goes through that pointer inside a mutex, so it stays sound whichever thread PICam calls it on. A panic is caught, cancels the calibration, and resumes once `Calibrate` returns.

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

- No `sys` type may appear in a public signature or a public field. `sys` is private, so users can't even name those types. Use the `define_picam_enum!` wrappers and Rust primitives (`i32`, `i64`, `f64`, `bool`) instead of `sys::piint`, `sys::pi64s`, `sys::piflt` or `sys::pibln`.
- Public names treat acronyms as words, following Rust convention: `CameraId`, `Error::InvalidCameraId`, `Parameter::EmIccdGain`, not PICam's `CameraID` or `EMIccdGain`. Generated names get this from `type_case` in `xtask/src/generate.rs`, with an override table for names it can't convert (such as the SeNsR product name, `Parameter::SensrWindowHeight`). Enum variants whose name differs from PICam's carry `#[doc(alias = "...")]` with PICam's spelling, so rustdoc search still finds them.
- Every public type is re-exported from `lib.rs`. Modules themselves stay private, except `params`, a generated set of typed parameter handles that would crowd the crate root.
- Follow Rust API conventions (the Rust API Guidelines) wherever possible: idiomatic names, `Result` for anything fallible, owned return types, and RAII for resources.

## Documentation standards

**Settled:**

- Use standard rustdoc conventions. Every public item and every public struct field gets a `///` doc comment, starting with a one-line summary sentence.
- Every fallible public function has a `# Errors` section.
  - When the manual or the code makes clear which errors can occur, name them with intra-doc links.
  - When it is not clear, write "Returns an error if PICam is unable to …" rather than guessing. A final pass through the PICam manual to fill in these gaps is planned.
- Intra-doc links use public paths, the crate-root re-exports (`crate::Error::X`, `crate::Camera::open`), never paths through a private module (`crate::error::Error::X`).
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

- `cargo run -p xtask -- progress` counts `dead_code` warnings from `cargo check` and reports how many of the 199 header functions are wired into the safe layer, overall and per header. Add `--list` to print every function still unwrapped. It also reports how many of the 58 header enums are wrapped, counting an enum once `src/` names its raw type as `sys::<Name>`. Unused-code warnings can't show this for enums, because a raw struct that an unwrapped function takes keeps the enums in its fields from being reported. It is plain Rust (the `xtask` workspace member, never published), so it runs the same on macOS and Windows. Because the tool relies on these warnings, the crate deliberately has **no** `#![allow(dead_code)]`; don't add one.
- `cargo run -p xtask -- generate` writes the files in `src/generated/` from the installed headers, and `generate --check` checks them and the hand-written enums without writing anything. See [Enums and undefined behaviour](#enums-and-undefined-behaviour-settled).
- The README's "to test" list collects behaviour that can only be confirmed on real hardware or with the SDK running. Add to it whenever an assumption can't be checked from the manual.

## Roadmap

As of 2026-10-27: 197 / 199 functions wrapped.

### 1. Finish the `sys`-type refactor (done)

- [x] `define_picam_enum!` and wrappers for `ComputerInterface`, `ValueType`, `ValueAccess`, `ConstraintType`, `ConstraintScope`, `ConstraintCategory`
- [x] wrappers for `PicamEnumeratedType`, `PicamConstraintSeverity`, `PicamRoisConstraintRulesMask` (bitmask)
- [x] decide on and implement a public `Model` type (see [Models](#models-settled))
- [x] replace `sys` enums in public signatures and fields with the wrappers (`CameraId`, `Camera::value_type` and friends, the constraint `category` arguments, the constraint struct fields, `available_demo_models`)
- [x] replace `sys::piint`/`pi64s`/`piflt` with `i32`/`i64`/`f64` in public items
- [x] `Camera::available_ids`, `Camera::unavailable_ids`, `Camera::available_demo_models`, `CameraId::connect_demo`, `CameraId::disconnect_demo`, `CameraId::is_demo`, `CameraId::is_connected`, `CameraId::is_open_elsewhere` and `CameraId::firmware_details` take `&Library`, like `Camera::list_open_cameras` and `name()`, so calling them without an initialized library doesn't compile instead of failing with `Error::LibraryNotInitialized`
- [x] intra-doc links use the public path `crate::Error::X`, not `crate::error::Error::X`
- [x] public names treat acronyms as words, following Rust convention: `Error::InvalidCameraId`, `Error::InvalidAccessoryId`, `Parameter::EmIccdGain`, `Parameter::EmIccdGainControlMode`, `Parameter::AdcEmGain`, `Parameter::ApplicableStarDefectMapId`, `Parameter::SensrWindowHeight` and `EnumeratedType::EmIccdGainControlMode`. Generated names get this from a rule in `xtask/src/generate.rs` rather than one-off overrides, and each renamed item carries `#[doc(alias = "...")]` with PICam's spelling so rustdoc search still finds it. In the same pass, read the full list of `Model` constants once and add overrides where names are uneven (such as `PRO_EM_1600XX_2B` next to `PI_MAX4_1024X256`, or `RETIGA_E7` next to `LANSIS_A_261BR`). Outcome: `ProEMHS` became `PRO_EM_HS`. The rest keep PICam's own grouping, since `1600xx` isn't a `width x height` size like `1024x256`
- [x] re-export `CameraId` and `CameraHandle` from `lib.rs`
- [x] re-export `FirmwareDetail`, the existing `define_picam_enum!` enums (`ComputerInterface`, `ValueType`, `ValueAccess`, `ConstraintType`, `ConstraintScope`, `ConstraintCategory`) and the new ones from `lib.rs`
- [x] doc comments on the enums generated by `define_picam_enum!` and their variants, and on all public struct fields
- [x] seal the value traits (`PicamValue`, `ReadablePicamValue`, `OnlineSettablePicamValue`). Their methods are currently public, safe and take a raw `PicamHandle`, so callers can pass PICam an arbitrary pointer from safe code, and `cargo clippy` fails with `not_unsafe_ptr_arg_deref`. Use the sealed-trait pattern: the public traits stay usable as bounds (`camera.get::<f64>(…)` is unchanged), and the methods that take a handle move to a crate-private supertrait that users can neither call nor implement.
- [x] clear every lint in `Cargo.toml`'s `[lints]` table: all `missing_docs` (including the `Error` variants, the crate-level `//!` doc in `lib.rs` and a `//!` doc for `build.rs`), the `# Errors` sections on the trait methods, `Debug` for `Camera` and `Library`, and the stray blank line after the doc comment on `OnlineSettablePicamValue::set_online`
- [x] once clean, `cargo clippy -p picam-rs -- -D warnings -A dead_code` passes. This is the same check the push/PR gate runs.

### 2. Typed parameters and hardware value enums

Decided with the maintainer on 2026-10-25. Two chunks, after stage 1, because the generated handles copy stage 1's renamed parameter names.

- [x] **typed parameter handles:**
  - Each parameter gets a generated handle that carries its Rust value type, so `camera.get(…)` infers the type and a wrong type doesn't compile.
  - The type comes from the value type in the parameter's `PI_V` code, and on/off parameters become `bool`.
  - `get`, `set`, `default_value`, `can_set`, `read` and `set_online` take a handle.
  - `Parameter` stays, for parameters learned at run time (`Camera::parameters`, `Camera::commit_parameters`, `Unknown`) and for the functions that don't touch values, which accept either.
  - An untyped fallback stays for parameters without a handle: the `_untyped` methods.
  - Enum parameters are typed `i32` until the next chunk.
  - The handles live in the public `params` module (decided with the maintainer on 2026-10-25).
- [x] **hardware value enums:**
  - Before the untyped fallback reads or writes an enum value, it checks the parameter's enumerated type with `Picam_GetParameterEnumeratedType`, since PICam reads every enum through the same integer call and can't catch a mismatch itself. This waits for this chunk because no enum value types exist before it.
  - Generate the 39 hardware enums, `ActiveShutter` through `VacuumStatus`. The six `*Mask` enums use `bitflags`, and the rest use `define_picam_enum!`.
  - Docs are short and generated: each enum names its `Parameter`, and each value names its PICam constant.
  - Enum parameters switch from `i32` to their enum through a mapping table in the generator, matched by name with overrides. It is checked against `Camera::enumerated_type` on the demo models, and any parameter no demo model has goes on the README's "to test" list.
  - The acquisition enums (`AcquisitionState`, `AcquisitionErrorsMask` and `AcquisitionStateErrorsMask`) wait for acquisition. The advanced API's (`DiscoveryAction`, `HandleType` and `DynamicsMask`) wait for stage 4.

### 3. Remaining `picam.h` (7 functions, plus `Picam_DestroyCalibrations` in stage 5)

- [x] quick wins: `Picam_GetVersion`, `Picam_IsLibraryInitialized`. `Picam_DestroyCalibrations` moved to stage 5: it frees only the array `PicamAccessory_GetLightSourceReference` allocates (manual 4.3.3.4 and 7.4.6.1), so it is wrapped with that call
- [x] acquisition: `Picam_Acquire`, `Picam_StartAcquisition`, `Picam_StopAcquisition`, `Picam_IsAcquisitionRunning`, `Picam_WaitForAcquisitionUpdate`. This is the most important remaining piece and needs careful design: who owns the frame buffers, and how data is copied out. An agent may write it, but only after the longer design discussion in `CLAUDE.md`, with the actual code shown and approved first (decided with the maintainer on 2026-10-25). See [Acquisition](#acquisition-settled).
- [ ] decode frame metadata (time stamps, frame tracking, gate and modulation tracking), as its own chunk, after checking on demo cameras what each produces

### 4. `picam_advanced.h` (76 functions)

Callback registration (`Register*`/`Unregister*` pairs), parameter validation, camera devices, defect maps, user-supplied acquisition buffers, and camera and accessory discovery (`picam_accessory.h`'s discovery functions come here, with the discovery callback their results arrive through). It is very repetitive and well suited to macros, but callbacks across the FFI boundary are the hardest part of the crate to get right, so they go through the design discussion in `CLAUDE.md`, with the actual code shown and approved first (decided with the maintainer on 2026-10-25).

- [x] parameter dynamics (`Dynamics`, `dynamics`, `extrinsic_dynamics`) and the calls listing every constraint on a parameter (`collection_constraints` and the rest)
- [x] parameter validation (`validate_parameter`, `validate_parameters`, `validate_dependent_parameter`)
- [x] pixel and star defect maps
- [x] camera devices and handle types (`Camera::open_device`, `Camera::device` and `CameraDevice`, `Camera::list_open_camera_devices`, `HandleType` and `CameraHandle`'s `handle_type`, `model` and `device`)
- [x] committing to and refreshing from the camera device
- [x] user-supplied acquisition buffers, under the acquisition design discussion (see [Acquisition](#acquisition-settled))
- callbacks, designed with the maintainer on 2026-10-26 (see [Callbacks](#callbacks-settled)):
  - [x] parameter value, extrinsic value, relevance and access watches, with the machinery
  - [x] dependent constraint watches and status notifications
  - [x] camera and accessory discovery
  - [x] user state (`Get/SetUserState`) and `Calibrate`'s progress callback
  - [x] acquisition state watches
  - [ ] the acquisition-updated callback, under its own design discussion

### 5. `picam_accessory.h` (18), `picam_em_calibration.h` (11), `picam_special.h` (2)

These largely mirror patterns already in `camera.rs`.

- [x] accessory identification and access (`Accessory`, `AccessoryId`, `AccessoryHandle`), firmware details and the light source reference, with `Picam_DestroyCalibrations` from `picam.h`, which frees the reference
- [x] `picam_special.h`: `Camera::can_erase_residual` and `Camera::erase_residual`, through the camera's device handle (wrapping `PicamAdvanced_GetCameraDevice` privately)
- [x] accessory parameters: `Accessory` shares `Camera`'s parameter methods through `impl_parameter_methods!` (no new functions)
- [x] `picam_em_calibration.h`, apart from `Calibrate` and its progress callback, which wait for the callbacks
- accessory discovery (`DiscoverAccessories`, `StopDiscoveringAccessories`, `IsDiscoveringAccessories`) moved to stage 4's callbacks, with the discovery callback its results arrive through

### 6. Finishing

- a pass through the manual to fill in `# Errors` sections
- a crate-level README with an experimental notice
- revisit the shared `commit_parameters` and `are_parameters_committed` docs, which read oddly on a `CameraDevice`, where `set` isn't pending (left until then, decided with the maintainer on 2026-10-26)
- hardware testing from the README's "to test" list, then publishing

## Open questions

- **The acquisition-updated callback** (`PicamAdvanced_RegisterForAcquisitionUpdated`), the last two functions, under its own design discussion. PICam calls it with the data instead of `wait`, and the two can't be combined (manual 6.4.5).
- **`Camera::device` can't fail:** since `Camera` keeps its device handle, `Camera::device` always succeeds, but it still returns a `Result` so its merged signature stays the same. Dropping the `Result` is a candidate for the stage 6 API review.
- **Typed status values:** `notify_when_status` and `wait_for_status_parameter_value` take the status value as a raw `i32`. A version taking a `TypedParameter<T>` with an enum `T`, such as `params::SENSOR_TEMPERATURE_STATUS` with `SensorTemperatureStatus::Locked`, would catch a value of the wrong enum at compile time. A candidate for the stage 6 API review.
