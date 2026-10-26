# picam-rs
library for interfacing with teledyne princeton instruments cameras, written in rust

## to test
- [ ] not sure what happens to `CameraHandle` after closing it, need to test by closing a handle, then querying a lingering copy of it to see what comes back
- [ ] the typed handles for `InactiveShutterTimingModeResult`, `BracketGating`, `InputTriggerStatus`, `LightSource`, `LightSourceStatus`, `Age` and `LifeExpectancy` are typed from the header only, since no demo model has these parameters, need to check `Camera::value_type` for them on hardware that has them, and `Camera::enumerated_type` for the four enum parameters among them: `InactiveShutterTimingModeResult` (`ShutterTimingMode`) and `InputTriggerStatus` (`TriggerStatus`), whose enums come from the manual, and `LightSource` and `LightSourceStatus`, matched to the enums of the same name
- [ ] `Camera::read` and `Camera::set_online` haven't been exercised yet, need to find which parameters a demo camera or real camera can read directly from hardware or set online, then read and set one
- [ ] acquired pixels are assumed to be stored row by row (`RoiData::width()` pixels per row), which the manual doesn't say, need to check with an image that has a known orientation
- [ ] `Error::AcquisitionFailed` hasn't been seen, since demo cameras never report acquisition errors, need to check each `AcquisitionErrors` bit on hardware, for example by unplugging a camera mid-acquisition (`CONNECTION_LOST`)
- [ ] dropping or finishing an `Acquisition` on a camera that stops responding should give up after about 10 s, need to check on hardware, and what the next `start_acquisition` returns
- [ ] `Acquisition::stop` from the waiting thread, between bounded waits, was only tried on demo cameras, need to check it ends a real camera's acquisition promptly
- [ ] frame metadata sits between `FrameSize` and `FrameStride` (seen on a PIXIS 100F demo camera with time stamps on), need to check on hardware with time stamps and frame tracking on
