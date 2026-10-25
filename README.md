# picam-rs
library for interfacing with teledyne princeton instruments cameras, written in rust

## to test
- [ ] not sure what happens to `CameraHandle` after closing it, need to test by closing a handle, then querying a lingering copy of it to see what comes back
- [ ] `Camera::read` and `Camera::set_online` haven't been exercised yet, need to find which parameters a demo camera or real camera can read directly from hardware or set online, then read and set one
