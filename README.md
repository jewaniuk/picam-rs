# picam-rs

A safe Rust wrapper around the PICam C API (SDK 5.15), which controls Teledyne Princeton Instruments scientific cameras. It wraps all 199 PICam functions.

> **Experimental.** Most of this crate has only been tested against PICam's demo cameras, not real hardware, and its API may change. See [Hardware testing](#hardware-testing).
>
> **LLM-generated.** Much of the code was written by an LLM (Anthropic's Claude), working to a design set by the maintainer, who reviewed every change before it was merged.

## Requirements

- Windows, x86-64.
- The PICam SDK, installed separately. Tested with 5.15.11.2503. The crate links `Picam.lib` from `C:\Program Files\Princeton Instruments\PICam\Libraries`, or from the `Libraries` directory next to the `Includes` directory `PICAM_DIR` points to. No SDK files ship with this crate.
- Pleora eBUS on `PATH` (`C:\Program Files\Common Files\Pleora\eBUS SDK`), which PICam loads when it initializes. The SDK installer doesn't add it.

## Usage

```toml
[dependencies]
picam-rs = "0.1"
```

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use picam_rs::{Camera, Library, Pixels, params};

fn main() -> picam_rs::Result<()> {
    let library = Arc::new(Library::new()?);
    let mut camera = Camera::open_first(library)?;

    camera.set(params::EXPOSURE_TIME, 50.0)?; // milliseconds
    let failed = camera.commit_parameters()?;
    assert!(failed.is_empty(), "rejected: {failed:?}");

    let readouts = camera.acquire(1, Some(Duration::from_secs(5)))?;
    for readout in readouts.iter() {
        for frame in readout.frames() {
            for roi in frame.rois() {
                if let Some(Pixels::Mono16(pixels)) = roi.pixels() {
                    println!("{} x {}: {} pixels", roi.width(), roi.height(), pixels.len());
                }
            }
        }
    }
    Ok(())
}
```

Without a camera, connect one of PICam's demo cameras with `CameraId::connect_demo` and open it with `Camera::open`.

- **Parameters:** typed handles in `params` fix each value's type at compile time. The `_untyped` methods cover parameters with no handle.
- **Acquired data:** borrowed from PICam's memory without copying, unless you give PICam a buffer or stream the acquisition, and the borrow checker stops the next acquisition while it's in use.
- **Callbacks:** watches, discovery and streamed acquisitions deliver events through channels, so no user code runs inside PICam. The exception is `EmCalibration::calibrate`'s progress closure.
- **Threads:** `Camera` is neither `Send` nor `Sync`, so it stays on the thread that opened it, and other threads send that thread their requests.

## Hardware testing

Checked on a PIXIS 100BR with PICam 5.16.3: [HARDWARE_TESTS.md](HARDWARE_TESTS.md) gives each result. These are still unchecked on hardware, or only partly checked, and that file lists every item and how to check it.

- Accessories: everything, since there are no demo accessories.
- EM gain calibration, which PICam won't run on a demo camera.
- Acquisition: PICam's acquisition errors other than lost data, ending acquisitions on a camera that stops responding, when PICam sets and clears the overrun flag, the device handle's refusals while running, clearing the readout count online, and non-destructive readout.
- Streamed acquisitions: overrun, slow callbacks, reading the overrun flag while the callback is unregistered, and whether PICam writes to its own memory during a callback.
- Frame metadata: bit depths other than 64, and gate and modulation tracking on PI-MAX cameras.
- Parameters: the parameters no demo camera has.
- Camera devices: that a value set through `CameraDevice` takes effect on its own, and that its handle stays the same while the camera is open.
- Watches and notifications: extrinsic readings during an acquisition and the thread they arrive on, value access changes, status notification errors and notifications set as one fires, and constraint callbacks.
- Discovery: real cameras and accessories being plugged in, unplugged and powered off.
- Defect maps and residual charge erasure.

If you try the crate on real hardware and find that one of these works or doesn't, please [open an issue](https://github.com/jewaniuk/picam-rs/issues/new?template=hardware-report.md) with your camera model, PICam version and what you saw.

## Tests

`cargo test` runs against PICam's demo cameras, so it needs the SDK but no hardware.

## License

MIT or Apache-2.0, at your option.
