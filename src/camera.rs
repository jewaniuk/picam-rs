//! Camera lifecycle.

use std::sync::Arc;

use crate::convert::{FirmwareDetail, c_char_array_to_string, string_to_c_char_array};
use crate::error::{Result, picam_call};
use crate::library::Library;
use crate::sys;

/// An open connection to a PICam camera.
pub struct Camera {
    handle: sys::PicamHandle,
    // keeps the library alive for as long as any `Camera` exists, and lets
    // multiple `Camera`s share one `Library`
    _library: Arc<Library>,
}

impl Camera {
    /// Opens the first available camera.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::NoCamerasAvailable`] if no camera is connected.
    pub fn open_first(library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening first available camera");
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::Picam_OpenFirstCamera(&mut handle))?;
        Ok(Self {
            handle,
            _library: library,
        })
    }

    /// Opens a specific camera, as identified by [`Camera::available_ids`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidCameraID`] if `id` doesn't
    /// match a currently available camera, or
    /// [`crate::error::Error::CameraAlreadyOpened`] if it's already open
    /// elsewhere in this process.
    pub fn open(id: &CameraId, library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening camera: {}", id.serial_number);
        let raw_id = id.to_raw()?;
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::Picam_OpenCamera(&raw_id, &mut handle))?;
        Ok(Self {
            handle,
            _library: library,
        })
    }

    /// Lists every camera handle currently open in this process, whether or
    /// not it was opened through this crate
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate open cameras.
    pub fn list_open_cameras(library: &Library) -> Result<Vec<CameraHandle<'_>>> {
        tracing::debug!("enumerating open cameras");
        let _ = library; // ties the returned lifetime to `library`, see `CameraHandle`
        let mut array: *const sys::PicamHandle = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetOpenCameras(&mut array, &mut count))?;

        // SAFETY: PICam has just given us a valid pointer to `count`
        // contiguous `PicamHandle`s
        let handles = unsafe { std::slice::from_raw_parts(array, count as usize) }
            .iter()
            .map(|&handle| CameraHandle {
                handle,
                _marker: std::marker::PhantomData,
            })
            .collect();

        picam_call!(sys::Picam_DestroyHandles(array))?;
        Ok(handles)
    }

    /// Lists the currently available (connectable, not yet open) cameras.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate cameras.
    pub fn available_ids() -> Result<Vec<CameraId>> {
        tracing::debug!("enumerating available cameras");
        let mut array: *const sys::PicamCameraID = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetAvailableCameraIDs(&mut array, &mut count))?;
        collect_and_destroy_ids(array, count)
    }

    /// Lists cameras that were previously available but have since been
    /// disconnected or opened elsewhere.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate cameras.
    pub fn unavailable_ids() -> Result<Vec<CameraId>> {
        tracing::debug!("enumerating unavailable cameras");
        let mut array: *const sys::PicamCameraID = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetUnavailableCameraIDs(&mut array, &mut count))?;
        collect_and_destroy_ids(array, count)
    }

    /// Reports whether this camera is still connected.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's connection
    /// status (for example, if the handle is no longer valid).
    pub fn is_connected(&self) -> Result<bool> {
        handle_is_connected(self.handle)
    }

    /// Reports whether this camera is currently in a faulted state.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's fault status.
    pub fn is_faulted(&self) -> Result<bool> {
        handle_is_faulted(self.handle)
    }

    /// Returns the [`CameraId`] identifying this open camera.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's identity.
    pub fn id(&self) -> Result<CameraId> {
        handle_id(self.handle)
    }

    /// Lists the camera models available for use with [`Camera::connect_demo`].
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate demo camera models.
    pub fn available_demo_models() -> Result<Vec<sys::PicamModel>> {
        tracing::debug!("enumerating available demo camera models");
        let mut array: *const sys::PicamModel = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetAvailableDemoCameraModels(
            &mut array, &mut count
        ))?;

        // SAFETY: PICam has just given us a valid pointer to `count`
        // contiguous `PicamModel`s.
        let models = unsafe { std::slice::from_raw_parts(array, count as usize) }.to_vec();

        picam_call!(sys::Picam_DestroyModels(array))?;
        Ok(models)
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        if let Err(e) = picam_call!(sys::Picam_CloseCamera(self.handle)) {
            tracing::error!("failed to close PICam camera: {e}");
        }
    }
}

/// A reference to a camera handle that is open somewhere in this process,
/// without implying ownership of it.
///
/// Unlike [`Camera`], a `CameraHandle` never closes the underlying handle;
/// it exists purely to let you inspect a camera you don't own (for example,
/// one returned by [`Camera::list_open_cameras`] that some other `Camera` is
/// responsible for). Its lifetime ties it to the [`Library`] it came from,
/// which is the only guarantee PICam actually gives us here: the handle
/// cannot outlive the library, though its true owner may still close it
/// earlier. A call on an already-closed handle fails with
/// [`crate::error::Error::InvalidHandle`] rather than being unsound, since
/// PICam validates handles against its own internal table before using them.
#[derive(Debug, Clone, Copy)]
pub struct CameraHandle<'a> {
    handle: sys::PicamHandle,
    _marker: std::marker::PhantomData<&'a Library>,
}

impl CameraHandle<'_> {
    /// Reports whether this camera is currently connected.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's connection
    /// status.
    pub fn is_connected(&self) -> Result<bool> {
        handle_is_connected(self.handle)
    }

    /// Reports whether this camera is currently in a faulted state.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's fault status.
    pub fn is_faulted(&self) -> Result<bool> {
        handle_is_faulted(self.handle)
    }

    /// Returns the [`CameraId`] identifying this camera.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's identity.
    pub fn id(&self) -> Result<CameraId> {
        handle_id(self.handle)
    }
}

fn handle_is_connected(handle: sys::PicamHandle) -> Result<bool> {
    let mut connected: sys::pibln = 0;
    picam_call!(sys::Picam_IsCameraConnected(handle, &mut connected))?;
    Ok(connected != 0)
}

fn handle_is_faulted(handle: sys::PicamHandle) -> Result<bool> {
    let mut faulted: sys::pibln = 0;
    picam_call!(sys::Picam_IsCameraFaulted(handle, &mut faulted))?;
    Ok(faulted != 0)
}

fn handle_id(handle: sys::PicamHandle) -> Result<CameraId> {
    // SAFETY: `PicamCameraID` is a plain-data struct with no invariant
    // that forbids an all-zero bit pattern, `model`/`computer_interface`
    // are newtypes (any i32 is a valid value), and the name fields are
    // just byte arrays, `Picam_GetCameraID` fully overwrites this before
    // we read any of it back
    let mut raw: sys::PicamCameraID = unsafe { std::mem::zeroed() };
    picam_call!(sys::Picam_GetCameraID(handle, &mut raw))?;
    Ok(CameraId::from_raw(&raw))
}

/// Identifies a specific camera, whether or not it is currently open.
#[derive(Debug, Clone)]
pub struct CameraId {
    pub model: sys::PicamModel,
    pub computer_interface: sys::PicamComputerInterface,
    pub sensor_name: String,
    pub serial_number: String,
}

impl CameraId {
    /// Converts a raw `PicamCameraID` into an owned `CameraId`.
    fn from_raw(raw: &sys::PicamCameraID) -> Self {
        Self {
            model: raw.model,
            computer_interface: raw.computer_interface,
            sensor_name: c_char_array_to_string(&raw.sensor_name),
            serial_number: c_char_array_to_string(&raw.serial_number),
        }
    }

    /// Converts this [`CameraId`] into a raw `PicamCameraID`, for passing
    /// back into PICam (e.g. via [`Camera::open`]).
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ValueTooLong`] if `sensor_name` or
    /// `serial_number` (plus its nul terminator) doesn't fit in PICam's
    /// fixed-size buffers.
    fn to_raw(&self) -> Result<sys::PicamCameraID> {
        Ok(sys::PicamCameraID {
            model: self.model,
            computer_interface: self.computer_interface,
            sensor_name: string_to_c_char_array(&self.sensor_name)?,
            serial_number: string_to_c_char_array(&self.serial_number)?,
        })
    }

    /// Reports whether a camera matching this ID is currently connected,
    /// without opening it.
    ///
    /// # Errors
    /// Returns [`crate::error::Error::ValueTooLong`] if this ID's fields
    /// don't fit in PICam's fixed-size buffers, or an error if PICam is unable
    /// to query connection status.
    pub fn is_connected(&self) -> Result<bool> {
        let raw = self.to_raw()?;
        let mut connected: sys::pibln = 0;
        picam_call!(sys::Picam_IsCameraIDConnected(&raw, &mut connected))?;
        Ok(connected != 0)
    }

    /// Reports whether a camera matching this ID is currently open in
    /// another process.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ValueTooLong`] if this ID's fields
    /// don't fit in PICam's fixed-size buffers, or an error if PICam is
    /// unable to query this.
    pub fn is_open_elsewhere(&self) -> Result<bool> {
        let raw = self.to_raw()?;
        let mut open_elsewhere: sys::pibln = 0;
        picam_call!(sys::Picam_IsCameraIDOpenElsewhere(
            &raw,
            &mut open_elsewhere
        ))?;
        Ok(open_elsewhere != 0)
    }

    /// Connects to a simulated ("demo") camera of the given model, identified
    /// by an arbitrary serial number, without requiring real hardware.
    ///
    /// Returns the new camera's [`CameraId`], pass it to [`Camera::open`] to
    /// actually open it.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidString`] if `serial_number`
    /// contains an embedded null byte, [`crate::error::Error::InvalidDemoModel`]
    /// if `model` isn't a valid demo model, or
    /// [`crate::error::Error::DemoAlreadyConnected`] if a demo camera with this
    /// serial number is already connected.
    pub fn connect_demo(model: sys::PicamModel, serial_number: &str) -> Result<CameraId> {
        tracing::debug!("connecting demo camera: {serial_number}");
        let c_serial_number = std::ffi::CString::new(serial_number)
            .map_err(|_| crate::error::Error::InvalidString)?;
        let mut raw: sys::PicamCameraID = unsafe { std::mem::zeroed() };
        picam_call!(sys::Picam_ConnectDemoCamera(
            model,
            c_serial_number.as_ptr(),
            &mut raw
        ))?;
        Ok(CameraId::from_raw(&raw))
    }

    /// Disconnects this demo camera.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ValueTooLong`] if this ID's fields
    /// don't fit in PICam's fixed-size buffers, or an error if this isn't
    /// a currently connected demo camera.
    pub fn disconnect_demo(&self) -> Result<()> {
        let raw = self.to_raw()?;
        picam_call!(sys::Picam_DisconnectDemoCamera(&raw))
    }

    /// Reports whether this camera is a simulated ("demo") camera rather
    /// than real hardware.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ValueTooLong`] if this ID's fields
    /// don't fit in PICam's fixed-size buffers, or an error if PICam is
    /// unable to query this.
    pub fn is_demo(&self) -> Result<bool> {
        let raw = self.to_raw()?;
        let mut demo: sys::pibln = 0;
        picam_call!(sys::Picam_IsDemoCamera(&raw, &mut demo))?;
        Ok(demo != 0)
    }

    /// Lists this camera's firmware components.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ValueTooLong`] if this ID's fields
    /// don't fit in PICam's fixed-size buffers, or an error if PICam is
    /// unable to query firmware details.
    pub fn firmware_details(&self) -> Result<Vec<FirmwareDetail>> {
        let raw_id = self.to_raw()?;
        let mut array: *const sys::PicamFirmwareDetail = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetFirmwareDetails(
            &raw_id, &mut array, &mut count
        ))?;

        // SAFETY: PICam has just given us a valid pointer to `count`
        // contiguous `PicamFirmwareDetail`s
        let details = unsafe { std::slice::from_raw_parts(array, count as usize) }
            .iter()
            .map(FirmwareDetail::from_raw)
            .collect();

        picam_call!(sys::Picam_DestroyFirmwareDetails(array))?;
        Ok(details)
    }
}

/// Copies `count` `PicamCameraID`s out of a PICam-owned array into owned
/// [`CameraId`]s, then frees PICam's array. Shared by `available_ids` and
/// `unavailable_ids`, which are otherwise identical apart from which raw
/// function produced the array.
fn collect_and_destroy_ids(
    array: *const sys::PicamCameraID,
    count: sys::piint,
) -> Result<Vec<CameraId>> {
    // SAFETY: the caller just received `array`/`count` from a PICam
    // "ALLOCATES" function, which guarantees `count` valid, contiguous
    // `PicamCameraID`s at `array` until `Picam_DestroyCameraIDs` is called
    let ids = unsafe { std::slice::from_raw_parts(array, count as usize) }
        .iter()
        .map(CameraId::from_raw)
        .collect();
    picam_call!(sys::Picam_DestroyCameraIDs(array))?;
    Ok(ids)
}
