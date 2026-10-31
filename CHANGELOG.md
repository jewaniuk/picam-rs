# Changelog

## 0.1.0 (unreleased)

The first release. It wraps all 199 functions and 58 enums of the PICam C API (SDK 5.15):

- **Library and cameras:** `Library` initializes PICam for as long as it or any camera opened with it is alive. Cameras, demo cameras, camera IDs and firmware details.
- **Parameters:** typed handles in `params` for every parameter, with untyped fallbacks. Constraints, dynamics, validation, and committing to and refreshing from the camera device.
- **Acquisition:** `acquire`, `start_acquisition` with waits and stops, and `start_acquisition_stream`. Data is borrowed from PICam's memory without copying, unless you give PICam a buffer, and frame metadata is decoded.
- **Callbacks:** watches, status notifications, acquisition state watches, and camera and accessory discovery, all delivered through channels.
- **Accessories, EM gain calibration, defect maps and residual charge erasure.**

Tested against PICam's demo cameras with SDK 5.15.11.2503 and the PICam 5.16.3.2512 runtime, and on a PIXIS 100BR with 5.16.3. Much of the crate is untested on real hardware: see `HARDWARE_TESTS.md`. Windows x86-64 only, with the PICam SDK installed separately.
