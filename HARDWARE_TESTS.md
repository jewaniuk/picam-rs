# Hardware tests

Behaviour that has only been checked on PICam's demo cameras, or not at all where no demo exists, and needs checking on real hardware. If you check one, please [open an issue](https://github.com/jewaniuk/picam-rs/issues/new?template=hardware-report.md) saying whether it works.

Checked so far: a PIXIS 100BR with PICam 5.16.3.2512, on 2026-10-30, using `tests/hardware_pixis.rs` and `tests/hardware_accessory.rs`. Each result is a sub-bullet under its item. An item is ticked only when every part of it has been answered; otherwise its sub-bullet says what remains.

## Running the hardware tests

`tests/hardware_pixis.rs` checks many of these items on a PIXIS camera and its shutter, and `tests/hardware_accessory.rs` reads what PICam sees of an accessory without changing it. Each test's doc comment says which items it covers, what it changes on the device, how long it takes, and any preparation. Every test is `#[ignore]`d and runs only when `PICAM_RS_HARDWARE` names it, one at a time:

```sh
PICAM_RS_HARDWARE=cooling cargo test -p picam-rs --test hardware_pixis -- --ignored --exact cooling --nocapture
```

Close LightField first, since PICam can't open a device that's open elsewhere. Every PIXIS test opens the camera, keeps the shutter closed (except `pixel_order`), and commits every parameter PICam opened with, including the temperature set point. `PICAM_RS_SET_POINT` sets the set point in °C instead: give the same one to every run, so no test warms or re-cools the camera. Three need care:

- `cooling`: run it first, right after powering the camera on from room temperature, so it has to cool. `PICAM_RS_COOLING_MINUTES` sets how long to wait (default 30). `cooling_locked` checks the rest on a camera that's already locked, so run it right after `cooling`, or any time the camera is locked at the same set point. Both read the temperature status with `read`, since on the PIXIS 100BR `get` kept the status from when the camera opened.
- `pixel_order`: needs light into the spectrometer, at a level that doesn't saturate. `PICAM_RS_EXPOSURE_MS` sets the exposure (default 100).
- `closing_while_acquiring`: run it last, since it closes the camera mid-acquisition, and the camera might need a power cycle afterwards.

`PICAM_RS_HARDWARE_DEMO=1` rehearses a PIXIS test on a demo camera.

## Handles

- [x] a `CameraHandle` whose camera had closed gave `InvalidHandle` from every method on a demo camera (`tests/errors.rs`), need to check on hardware, in case PICam reuses a closed handle's value for a camera opened later
  - PIXIS 100BR, PICam 5.16.3: `InvalidHandle` after closing. Reopening the camera gave a different handle value, and the old one still gave `InvalidHandle`. Demo cameras do reuse a closed handle's value for the next camera opened (seen with SDK 5.15 and PICam 5.16.3), so there a stale handle refers to the new camera.

## Accessories

PICam's accessory API covers FERGIE accessories and, among IntelliCal units, only the IntelliCal SWIR QTH lamp (manual Table A-4). The lab's IntelliCal is a He and Ne/Ar unit, which PICam doesn't support: with it connected, PICam 5.16.3 listed no accessories, available or unavailable. So these items stay open until someone with a supported accessory checks them.

- [ ] no demo accessories exist, so `Accessory` and `AccessoryId` were only checked with nothing connected, need to open a real accessory and check `id`, `is_connected`, `AccessoryId::is_connected`, `is_open_elsewhere`, `firmware_details`, `list_open_accessories` and, on a light source, `light_source_reference`, then configure it through the shared parameter methods (`parameters`, a `get` and `set`, and a `range_constraint` or `collection_constraint`), checking that `set` takes effect without a commit and `are_parameters_committed` stays `true` (manual 5.4.6), and that a value type accessories don't use (`get_untyped::<i64>`, or `params::ROIS`) is refused, then drop the `Accessory` and query a leftover `AccessoryHandle` from `list_open_accessories` to see what comes back, like `CameraHandle` above
- [ ] `dynamics`, `extrinsic_dynamics`, `collection_constraints` and `range_constraints` on an accessory, which only demo cameras have exercised
- [ ] the shared parameter methods' `# Errors` sections show on `Accessory` too, but no demo accessory exists, so none was seen on one, need to check them on a real accessory, and that `set`, `commit_parameters` and `restore_parameters_to_default_values` never give `AcquisitionInProgress` there, since accessories don't acquire
- [ ] watches were only tried on demo cameras, need to check on a real accessory (`watch_value`, `watch_relevance`, `watch_value_access` and `watch_extrinsic_value` through an `Accessory`)

## EM gain calibration

- [ ] PICam refuses to open a demo camera for EM calibration (`DemoNotSupported`), so `EmCalibration` was never opened, need to open a real EM camera for calibration and check `camera_id`, `calibration_date`, the sensor temperature reading, status and set point, its constraint, setting the set point, `list_open_calibrations`, and that `Camera::open` fails on the same camera while it's open, then drop the `EmCalibration` and query a leftover `EmCalibrationHandle` from `list_open_calibrations` to see what comes back, like `CameraHandle` above
- [ ] PICam won't open a demo camera for EM calibration, so `EmCalibration::calibrate` was never run, need to calibrate a real EM camera and check the progress values, that returning `false` gives `OperationCanceled`, and which thread PICam calls the progress callback on
- [ ] `calibrate` is sound only if PICam never calls the progress callback after `PicamEMCalibration_Calibrate` returns, which the manual implies (the call blocks until calibration ends) but doesn't say, need to check on hardware, for example by logging each callback and when `calibrate` returns

## Acquisition

- [x] acquired pixels are assumed to be stored row by row (`RoiData::width()` pixels per row), which the manual doesn't say, need to check with an image that has a known orientation
  - PIXIS 100BR, PICam 5.16.3: row by row. In a full-sensor frame of a spectrum, which varies along x, the column means varied by 37.1 and the row means by 0.8.
- [ ] `Error::AcquisitionFailed` hasn't been seen, since demo cameras never report acquisition errors, need to check each `AcquisitionErrors` bit on hardware, for example by unplugging a camera mid-acquisition (`CONNECTION_LOST`)
  - PIXIS 100BR, PICam 5.16.3: `DATA_LOST` seen, from an overrun. The other bits not seen.
- [ ] dropping or finishing an `Acquisition` on a camera that stops responding should give up after about 10 s, need to check on hardware, and what the next `start_acquisition` returns
- [x] `Acquisition::stop` from the waiting thread, between bounded waits, was only tried on demo cameras, need to check it ends a real camera's acquisition promptly
  - PIXIS 100BR, PICam 5.16.3: the acquisition ended 4 ms after `stop`, with no further readouts.
- [x] PICam refusing to set or commit parameters while an acquisition runs (`AcquisitionInProgress`) was seen on a demo camera only, need to check on hardware, since the acquisition views rely on the layout not changing mid-acquisition
  - PIXIS 100BR, PICam 5.16.3: setting the region and `commit_parameters` were refused with `AcquisitionInProgress`. The exposure time, which can be set online, was accepted through a plain `set`.
- [ ] the same for a camera's device handle: during an acquisition, `CameraDevice`'s `set`, `commit_parameters` and `restore_parameters_to_default_values`, and `Camera::commit_parameters_to_device` and the refreshes, were refused on demo cameras except for online-settable parameters (exposure time and the analog, intensifier and EM gains there), need to check on hardware for the same reason
  - PIXIS 100BR, PICam 5.16.3: the device's `set` of the region and `commit_parameters`, and `commit_parameters_to_device`, were refused with `AcquisitionInProgress`, and the device accepted the exposure time. `restore_parameters_to_default_values` and the refreshes weren't tried.
- [ ] a copy out of the camera's buffer is only kept if the overrun flag is still clear after copying, which catches a copy PICam wrote over midway (wrong bytes rather than undefined behaviour, since the copy reads the buffer volatilely, outside Rust's memory) only if PICam sets the flag no later than it starts writing over unread data, which the manual doesn't say, need to check on hardware at a high frame rate with a small buffer
  - PIXIS 100BR, PICam 5.16.3: inconclusive. A slow reader at about 92 frames per second kept 1 readout, then got `DATA_LOST`, which doesn't show when the flag is set.
- [x] `Camera` frees its acquisition buffer right after a successful `Picam_CloseCamera`, assuming PICam has stopped writing to it by then, need to check on hardware by closing a camera that is still acquiring until stopped
  - PIXIS 100BR, PICam 5.16.3: closed after the first readout of an acquisition running until stopped (20 ms exposures). No crash in the 3 s after, and the camera reopened and acquired normally. That shows no sign of writes after closing, though a stray write needn't crash.
- [ ] overrun was only seen on a demo camera (the flag, then `DataLost`), need to check a real camera's overrun at a high frame rate, and that the flag clears when the next acquisition starts
  - PIXIS 100BR, PICam 5.16.3: at about 92 frames per second into a small buffer, the flag was set and the next wait failed with `DATA_LOST`. The flag was clear after the next acquisition, but the buffer had been cleared before it, so whether starting an acquisition clears the flag is still open.
- [ ] the 10 s drain limit was never reached, need to check on hardware that ending an acquisition that runs until stopped gives up after a stop fails
- [x] whether a finite acquisition into the camera's buffer with more readouts than it holds is refused or wraps, and acquiring more readouts than `ExactReadoutCountMaximum` with a buffer, need to check both on hardware. Both copy out of the buffer with volatile reads checked against its bounds, so either outcome is sound, but which one happens isn't known
  - PIXIS 100BR, PICam 5.16.3: it wraps: 5 readouts into a buffer with room for 2 all arrived, with no errors. `ExactReadoutCountMaximum` is 65534, and 65535 readouts with a buffer started and gave a first update.
- [x] `acquire` and `wait` view PICam's own memory in place when the camera has no buffer (clearing the readout count online needs one), assuming that memory never wraps during a counted acquisition, seen on a demo camera only (an update held for 3 s through 400 readouts was unchanged, and counts above `ExactReadoutCountMaximum` were refused without a buffer), need to check on hardware with a long, fast acquisition
  - PIXIS 100BR, PICam 5.16.3: at about 92 frames per second, an update held for 3 s kept its data, and 400 of 400 readouts arrived.
- [ ] clearing the readout count online was only seen on a demo camera, where later acquisitions kept running until stopped, refusing to start without a buffer, though `READOUT_COUNT` still read the old count and `are_parameters_committed` was true, until `commit_parameters` ran again. Need to check all of this on hardware, and that `acquire(n)` after a clear still takes exactly `n` readouts, as on the demo. Without a buffer the crate refuses to clear, since the demo then reused PICam's own memory circularly
  - PIXIS 100BR, PICam 5.16.3: clearing worked, and the acquisition ran past its old count of 10. Afterwards `READOUT_COUNT` still read 10, `are_parameters_committed` was `true`, and `acquire(3)` took 3 readouts. A later `start_acquisition` with the cleared count wasn't tried.
- [x] `acquire(n)` leaves `READOUT_COUNT` at `n` afterwards on a demo camera, so a later `start_acquisition` takes `n` readouts unless the count is set again, need to check on hardware
  - PIXIS 100BR, PICam 5.16.3: `READOUT_COUNT` read 3 after `acquire(3)`.
- [ ] acquiring with non-destructive readout on was never tried, since the NIRVANA LN 640 demo camera, the only demo model with it, refuses to commit it (`commit_parameters` returns `Ok([TrackFrames])`), need to check on hardware what `acquire` and `start_acquisition` return then, since manual 6.4.1 says `Picam_Acquire` can't be used with it
  - PIXIS 100BR, PICam 5.16.3: the camera doesn't have non-destructive readout.

## Streamed acquisitions

- [ ] a streaming acquisition never overran on a demo camera, since the sink copies faster than the demo fills the buffer, need to check on hardware at a high frame rate with a small buffer that a stream reports `DATA_LOST` rather than a torn copy
  - PIXIS 100BR, PICam 5.16.3: it never overran at about 92 frames per second either: 275 updates, no lost data.
- [ ] PICam called the acquisition-updated callback once per readout on a demo camera, never batching, even when the callback was slow, need to check on hardware, though the crate handles several readouts per update either way
  - PIXIS 100BR, PICam 5.16.3: once per readout at about 92 frames per second (275 updates of 1 readout), then once with no data. Not tried with a slow callback.
- [ ] the acquisition-updated trampoline reads the overrun flag inside PICam's callback, which worked on a demo camera, need to check on hardware, including while the callback is being unregistered after a stream gave up ending its acquisition
  - PIXIS 100BR, PICam 5.16.3: worked during streams. Unregistering after a stream gave up wasn't tried.
- [ ] a stream copies data in PICam's own memory with an ordinary copy, assuming PICam isn't writing to it during the callback, seen on a demo camera only (the data stayed unchanged until the next acquisition), need to check on hardware with a long, fast acquisition
  - PIXIS 100BR, PICam 5.16.3: a stream of 400 frames at about 92 frames per second brought consecutive frame numbers and no errors, which doesn't show whether PICam writes during the callback.

## Frame metadata

- [x] frame metadata sits between `FrameSize` and `FrameStride` (seen on a PIXIS 100F demo camera with time stamps on), need to check on hardware with time stamps and frame tracking on
  - PIXIS 100BR, PICam 5.16.3: `FrameSize` 268,000 and `FrameStride` 268,024 bytes, with the 24 bytes between them holding both time stamps and the frame number.
- [x] frame metadata time stamps counted from the acquisition's start at 10 MHz on demo cameras, need to check on hardware where the camera stamps them, and which resolutions it offers
  - PIXIS 100BR, PICam 5.16.3: counted from the acquisition's start, the first exposure starting at about 15.5 to 15.9 ms, and only 10 MHz offered.
- [x] frame numbers counted frames, not readouts, from 1 in each acquisition on demo cameras, need to check on hardware
  - PIXIS 100BR, PICam 5.16.3: 1 to 5 in each of two acquisitions of 5 readouts of one frame.
- [ ] every demo model allows only 64-bit metadata values, need to check on hardware whether any camera offers other bit depths, which the crate decodes as sign-extended integers of up to 64 bits and floats of 32 or 64 bits
  - PIXIS 100BR, PICam 5.16.3: 64-bit only, for both time stamps and frame tracking.
- [ ] gate tracking added no metadata on a PI-MAX4 demo camera with repetitive gating or with modulation on, where PICam says it isn't relevant, even when turned on, need to check on a PI-MAX
- [ ] gate and modulation tracking were in the units of `Pulse` and `Modulation` on demo cameras, and the output signal frequency read 0 with sequential modulation phases, need to check on a PI-MAX

## Parameters

- [ ] the typed handles for `InactiveShutterTimingModeResult`, `BracketGating`, `InputTriggerStatus`, `LightSource`, `LightSourceStatus`, `Age` and `LifeExpectancy` are typed from the header only, since no demo model has these parameters, need to check `Camera::value_type` for them on hardware that has them, and `Camera::enumerated_type` for the four enum parameters among them: `InactiveShutterTimingModeResult` (`ShutterTimingMode`) and `InputTriggerStatus` (`TriggerStatus`), whose enums come from the manual, and `LightSource` and `LightSourceStatus`, matched to the enums of the same name
  - PIXIS 100BR, PICam 5.16.3: the camera has none of them.
- [x] `Camera::read` and `Camera::set_online` haven't been exercised yet, need to find which parameters a demo camera or real camera can read directly from hardware or set online, then read and set one
  - PIXIS 100BR, PICam 5.16.3: `SensorTemperatureReading` and `SensorTemperatureStatus` can be read, and `ExposureTime` and `AdcAnalogGain` set online. `read` gave the temperature, and `set_online` of the exposure time was accepted during an acquisition. `get` on `SensorTemperatureStatus` kept the status from when the camera opened while the sensor cooled and locked. Reading the status with `read` while it changed hasn't been tried.
- [x] the demo's sensor temperature status estimated 0 ms to `Locked` and couldn't estimate `Unlocked` (`None`), need to check `estimate_time_to_status_parameter_value` on a cooling camera, and which other parameters `can_wait_for_status` reports on hardware, since only `SensorTemperatureStatus` was waitable on the PIXIS 100F demo
  - PIXIS 100BR, PICam 5.16.3: the estimate to `Locked`, from room temperature, was `None`. Only `SensorTemperatureStatus` is waitable. Waiting for `Locked` once locked returned at once.
- [x] the errors the parameter methods' `# Errors` sections name were seen on demo cameras only (`tests/errors.rs`), need to check the common ones on hardware: a parameter the camera doesn't have (`ParameterDoesNotExist`), a read-only one (`ParameterValueIsReadOnly`), a value out of range (`InvalidParameterValue`), `set_online` outside an acquisition (`AcquisitionNotInProgress`) and changes during one (`AcquisitionInProgress`)
  - PIXIS 100BR, PICam 5.16.3: all five, as on demo cameras.
- [x] a commit that failed validation on a demo camera came back from PICam as `InvalidParameterValues` with the failed parameters, which `commit_parameters` returns as `Ok`, and committed none of the changes, need to check on hardware that PICam lists them the same way, and that a failed commit leaves the hardware's values alone
  - PIXIS 100BR, PICam 5.16.3: kinetics readout with the full-sensor region gave `Ok([Rois])`, left the parameters uncommitted, and left the device's exposure time and readout mode unchanged.

## Camera devices

- [ ] `CameraDevice` values were only compared with the model on demo cameras, need to check they match what a real camera is doing, for example a committed exposure time, and that setting one through the device takes effect on the hardware
  - PIXIS 100BR, PICam 5.16.3: the device's exposure time matched the committed one, and the camera ran at exposure times set through the device: 3 readouts took 0.40 s at 50 ms and 1.74 s at 500 ms. But each device `set` was followed by a commit on the model, which already held the same value, so the device `set` alone taking effect isn't isolated.
- [x] a failed `commit_parameters_to_device` gave `InvalidParameterValue` and left the demo device unchanged, need to check both on hardware, and whether `are_parameters_committed` stays `false` after `refresh_parameter_from_device` there too, as on demo cameras
  - PIXIS 100BR, PICam 5.16.3: the same: `InvalidParameterValue`, the device's readout mode unchanged, and `are_parameters_committed` still `false` after the refresh.
- [ ] `Camera` keeps the device handle it looks up when it opens, assuming it stays the same for as long as the camera is open, which the manual doesn't say. Need to check on hardware, after a disconnect and reconnect while open and after an acquisition, that `CameraDevice`'s calls, `erase_residual` and extrinsic watches still work, and that `list_open_camera_devices` lists the same handle as before (compared by its `Debug` output). If it changes, those calls would use a stale handle, most likely failing with `InvalidHandle`
- [x] a camera device reported the camera model's pending values as uncommitted and committed them with `commit_parameters`, while a value set through the device took effect or was refused at once, on a demo camera only, need to check on hardware
  - PIXIS 100BR, PICam 5.16.3: with a value pending on the model, the device reported uncommitted, and its `commit_parameters` returned `Ok([])` and committed the model's 77 ms exposure. A value set through the device showed on the model at once, still committed.

## Watches and notifications

- [ ] demo cameras send no extrinsic readings, so `watch_extrinsic_value` was only seen to register, need to watch `SENSOR_TEMPERATURE_READING` on a cooling camera, including while acquiring, and check the readings arrive on PICam's own thread one at a time (manual 7.4.9.3)
  - PIXIS 100BR, PICam 5.16.3: while cooling, readings arrived about every 2 s, apparently only when the temperature changed by 1 °C. None arrived during an acquisition while locked, since the temperature stayed put. Neither the thread they arrive on nor whether they are serialized was checked.
- [ ] no value access change was seen on a demo camera, so `watch_value_access` was only seen to register, need to find a parameter whose access changes on hardware
  - PIXIS 100BR, PICam 5.16.3: no change across all 69 parameters, through the settings the test changes.
- [ ] the demo's sensor temperature is always locked, so `notify_when_status` only fired at once or never, need to check it on a cooling camera, for example `SensorTemperatureStatus` reaching `Locked`, and what an error result looks like
  - PIXIS 100BR, PICam 5.16.3: `Locked` fired with `Ok(())` after 266 s of cooling from room temperature. No error result seen.
- [ ] the dependent constraint callbacks are assumed to pass one constraint, as the header says, though the manual calls it an array, need to check on hardware that each one sent matches the `Required` constraint, as on demo cameras
  - PIXIS 100BR, PICam 5.16.3: inconclusive. Changing the ADC quality sent no constraint.
- [ ] no demo setting changes the custom modulation sequence's constraint, so `watch_modulations_constraint` was only seen to register, need to find a setting that changes it on a camera with intensifier modulation
- [ ] a status notification set while an earlier one on the same status and value is still being delivered might be dropped, since the manual doesn't say whether PICam removes a fired notification before or after calling it, need to check on hardware by setting one from another thread just as one fires
- [ ] a status notification's sinks are found by the device handle, parameter and value PICam passes back, so on an error PICam must still pass the requested value, which the manual doesn't say (it describes the value as the one met), or the receiver never gets the error and never disconnects, need to check on hardware by making a notification fail
- [x] a demo camera says it can detect neither acquisition state, yet after other acquisitions in the same process it sends `ReadoutEnded` with both counters 0, need to check which states real cameras detect, the counters they send, and `LostCount`
  - PIXIS 100BR, PICam 5.16.3: it detects only `ReadoutEnded`, and sent ended counts 1, 2 and 3 for 3 readouts, with a started count of 0 and no errors, so no `LostCount`.

## Discovery

- [ ] camera discovery was only seen with demo cameras connecting and disconnecting, need to check `Found`, `Lost` and `Faulted` on hardware: plugging in, unplugging, powering off, opening in another process, and `open_in_this_process` for a camera open here
- [ ] no demo accessories exist, so accessory discovery was only started and stopped, need to plug in, unplug and power a real accessory while `Accessory::watch_discovery` runs, and check `user_state` and `set_user_state` on an `Accessory` and an `AccessoryHandle`

## Defect maps and residual charge

- [ ] demo cameras have no sensor defects, so `pixel_defect_map`, `star_defect_map` and `star_defect_maps` only returned empty maps, need to read them on a camera with known defects
  - PIXIS 100BR, PICam 5.16.3: empty as well: no pixel defects, and one empty star defect map.
- [ ] `Camera::erase_residual` was never called, since the PIXIS 100F demo camera the test uses can't erase residual charge, need to check it on a camera whose `can_erase_residual` is true
  - PIXIS 100BR, PICam 5.16.3: `can_erase_residual` is `false`.
