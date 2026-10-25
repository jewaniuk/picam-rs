# picam-rs
library for interfacing with teledyne princeton instruments cameras, written in rust

## to test
- [ ] not sure what happens to `CameraHandle` after closing it, need to test by closing a handle, then querying a lingering copy of it to see what comes back
- [ ] the typed handles for `InactiveShutterTimingModeResult`, `BracketGating`, `InputTriggerStatus`, `LightSource`, `LightSourceStatus`, `Age` and `LifeExpectancy` are typed from the header only, since no demo model has these parameters, need to check `Camera::value_type` for them on hardware that has them, and `Camera::enumerated_type` for the four enum parameters among them: `InactiveShutterTimingModeResult` (`ShutterTimingMode`) and `InputTriggerStatus` (`TriggerStatus`), whose enums come from the manual, and `LightSource` and `LightSourceStatus`, matched to the enums of the same name
- [ ] `Camera::read` and `Camera::set_online` haven't been exercised yet, need to find which parameters a demo camera or real camera can read directly from hardware or set online, then read and set one
