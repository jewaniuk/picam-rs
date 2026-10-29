//! Camera lifecycle.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use crate::acquisition::AcquisitionBuffer;
use crate::constraint::{
    ConstraintCategory, ModulationsConstraint, PulseConstraint, RoisConstraint,
};
use crate::convert::{
    FirmwareDetail, c_char_array_to_string, collect_raw_array, define_picam_enum,
    string_to_c_char_array,
};
use crate::device::impl_parameter_methods;
use crate::error::{Result, free_on_error, picam_call};
use crate::library::Library;
use crate::model::Model;
use crate::parameter::sealed::RawHandle;
use crate::parameter::{ComputerInterface, OnlineSettablePicamValue, Parameter, TypedParameter};
use crate::sys;
use crate::user_state;
use crate::watch::{self, Event, Kind, Watch};

/// An open connection to a PICam camera.
#[derive(Debug)]
pub struct Camera {
    handle: sys::PicamHandle,
    // the device handle, found once at open and closed with `handle`
    // by `Picam_CloseCamera` (manual 4.3.2.6)
    device: sys::PicamHandle,
    // the memory picam acquires into instead of its own, if any. kept until
    // picam has been given another or none, or the camera has closed
    buffer: Option<AcquisitionBuffer>,
    // keeps the library alive for as long as any `Camera` exists, and lets
    // multiple `Camera`s share one `Library`
    _library: Arc<Library>,
}

impl Camera {
    /// Opens the first available camera.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::NoCamerasAvailable`] if no camera is connected
    /// (seen with SDK 5.15), or an error if PICam is unable to find its device.
    pub fn open_first(library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening first available camera");
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::Picam_OpenFirstCamera(&mut handle))?;
        Self::from_model(handle, library)
    }

    /// Opens a specific camera, as identified by [`Camera::available_ids`].
    ///
    /// # Errors
    ///
    /// - [`crate::Error::InvalidCameraId`] if `id` matches no camera PICam
    ///   knows of (seen with SDK 5.15)
    /// - [`crate::Error::DeviceDisconnected`] if `id`'s camera has been
    ///   disconnected, such as a demo camera after
    ///   [`CameraId::disconnect_demo`] (seen with SDK 5.15)
    /// - [`crate::Error::CameraAlreadyOpened`] if it's already open in this
    ///   process (seen with SDK 5.15)
    /// - [`crate::Error::ValueTooLong`] if `id`'s fields don't fit in PICam's
    ///   fixed-size buffers
    /// - otherwise an error if PICam is unable to open it
    pub fn open(id: &CameraId, library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening camera: {}", id.serial_number);
        let raw_id = id.to_raw()?;
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::Picam_OpenCamera(&raw_id, &mut handle))?;
        Self::from_model(handle, library)
    }

    /// Wraps a newly opened camera's model handle, looking up its device
    /// handle once (PICam manual 7.4.3.5) and keeping it until the camera
    /// closes. If the lookup fails, the camera is closed again.
    fn from_model(model: sys::PicamHandle, library: Arc<Library>) -> Result<Self> {
        let mut device: sys::PicamHandle = std::ptr::null_mut();
        if let Err(e) = picam_call!(sys::PicamAdvanced_GetCameraDevice(model, &mut device)) {
            if let Err(close) = picam_call!(sys::Picam_CloseCamera(model)) {
                tracing::error!("failed to close PICam camera: {close}");
            }
            return Err(e);
        }
        Ok(Self {
            handle: model,
            device,
            buffer: None,
            _library: library,
        })
    }

    /// Opens a specific camera through PICam's advanced device call (PICam
    /// manual 7.4.3.1). The result is the same as [`Camera::open`]: the
    /// camera holds its model handle, and [`Camera::device`] gives the device.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::InvalidCameraId`] if `id` matches no camera PICam
    ///   knows of (seen with SDK 5.15)
    /// - [`crate::Error::CameraAlreadyOpened`] if it's already open in this
    ///   process (seen with SDK 5.15)
    /// - [`crate::Error::ValueTooLong`] if `id`'s fields don't fit in PICam's
    ///   fixed-size buffers
    /// - otherwise an error if PICam is unable to open it
    pub fn open_device(id: &CameraId, library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening camera device: {}", id.serial_number);
        let raw_id = id.to_raw()?;
        let mut device: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::PicamAdvanced_OpenCameraDevice(&raw_id, &mut device))?;
        let mut model: sys::PicamHandle = std::ptr::null_mut();
        if let Err(e) = picam_call!(sys::PicamAdvanced_GetCameraModel(device, &mut model)) {
            // closing the device closes the whole camera (manual 7.4.3.1)
            if let Err(close) = picam_call!(sys::PicamAdvanced_CloseCameraDevice(device)) {
                tracing::error!("failed to close PICam camera device: {close}");
            }
            return Err(e);
        }
        Ok(Self {
            handle: model,
            device,
            buffer: None,
            _library: library,
        })
    }

    /// Lists the model handle of every camera open in this process, whether
    /// or not it was opened through this crate.
    /// [`Camera::list_open_camera_devices`] lists their device handles.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate open cameras.
    pub fn list_open_cameras(library: &Library) -> Result<Vec<CameraHandle<'_>>> {
        tracing::debug!("enumerating open cameras");
        let _ = library; // ties the returned lifetime to `library`, see `CameraHandle`
        let mut array: *const sys::PicamHandle = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::Picam_GetOpenCameras(&mut array, &mut count));
        free_on_error!(result, array, sys::Picam_DestroyHandles)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let handles = unsafe {
            collect_raw_array(array, count, |&handle| CameraHandle {
                handle,
                _marker: std::marker::PhantomData,
            })
        };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyHandles(array))?;
        }
        Ok(handles)
    }

    /// Lists the device handle of every camera open in this process (PICam
    /// manual 7.4.3.3), whether or not it was opened through this crate.
    /// [`Camera::list_open_cameras`] lists their model handles.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate open camera devices.
    pub fn list_open_camera_devices(library: &Library) -> Result<Vec<CameraHandle<'_>>> {
        tracing::debug!("enumerating open camera devices");
        let _ = library; // ties the returned lifetime to `library`, see `CameraHandle`
        let mut array: *const sys::PicamHandle = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::PicamAdvanced_GetOpenCameraDevices(
            &mut array, &mut count
        ));
        free_on_error!(result, array, sys::Picam_DestroyHandles)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let handles = unsafe {
            collect_raw_array(array, count, |&handle| CameraHandle {
                handle,
                _marker: std::marker::PhantomData,
            })
        };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyHandles(array))?;
        }
        Ok(handles)
    }

    /// Lists the currently available (connectable, not yet open) cameras.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate cameras.
    pub fn available_ids(library: &Library) -> Result<Vec<CameraId>> {
        tracing::debug!("enumerating available cameras");
        let _ = library; // only taken to prove picam is initialized
        let mut array: *const sys::PicamCameraID = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::Picam_GetAvailableCameraIDs(&mut array, &mut count));
        free_on_error!(result, array, sys::Picam_DestroyCameraIDs)?;
        collect_and_destroy_ids(array, count)
    }

    /// Lists cameras that were previously available but have since been
    /// disconnected or opened elsewhere.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate cameras.
    pub fn unavailable_ids(library: &Library) -> Result<Vec<CameraId>> {
        tracing::debug!("enumerating unavailable cameras");
        let _ = library; // only taken to prove picam is initialized
        let mut array: *const sys::PicamCameraID = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::Picam_GetUnavailableCameraIDs(&mut array, &mut count));
        free_on_error!(result, array, sys::Picam_DestroyCameraIDs)?;
        collect_and_destroy_ids(array, count)
    }

    /// Reports whether this camera is still connected.
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

    /// Returns the [`CameraId`] identifying this open camera.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's identity.
    pub fn id(&self) -> Result<CameraId> {
        handle_id(self.handle)
    }

    /// The number [`Camera::set_user_state`] last stored with this camera, or
    /// 0 if none has been (PICam manual 7.4.7.1).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read it.
    pub fn user_state(&self) -> Result<usize> {
        user_state::get(self.handle)
    }

    /// Stores `state`, a number of your choosing, with this camera, for
    /// example to recognize it later among [`Camera::list_open_cameras`]
    /// (PICam manual 7.4.7.2). The camera's model and device handles share
    /// it, so [`CameraHandle::user_state`] reads it back from either.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to store it.
    pub fn set_user_state(&self, state: usize) -> Result<()> {
        user_state::set(self.handle, state)
    }

    /// Reports whether the camera can erase residual charge from its sensor
    /// (`picam_special.h`, which the PICam manual doesn't document).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_erase_residual(&self) -> Result<bool> {
        let device = self.device_handle();
        let mut erasable: sys::pibln = 0;
        picam_call!(sys::PicamSpecial_CanEraseResidual(device, &mut erasable))?;
        Ok(erasable != 0)
    }

    /// Erases residual charge from the camera's sensor (`picam_special.h`,
    /// which the PICam manual doesn't document). Check
    /// [`Camera::can_erase_residual`] first.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::DemoNotSupported`] on a demo camera, which can't
    /// erase residual charge (seen with SDK 5.15), or an error if PICam is
    /// unable to erase it.
    pub fn erase_residual(&self) -> Result<()> {
        let device = self.device_handle();
        picam_call!(sys::PicamSpecial_EraseResidual(device))
    }

    /// The handle of the camera's device (PICam manual 7.4.3.5), which some
    /// calls need instead of the camera model handle `Picam_OpenCamera`
    /// gives (manual 4.3.2.4): the `picam_special.h` functions refuse the
    /// model handle with `InvalidHandle` (seen on a demo camera with SDK
    /// 5.15). It's found once when the camera opens.
    pub(crate) fn device_handle(&self) -> sys::PicamHandle {
        self.device
    }

    /// The memory picam acquires into instead of its own, if any.
    pub(crate) fn acquisition_buffer(&self) -> Option<&AcquisitionBuffer> {
        self.buffer.as_ref()
    }

    /// Where the camera keeps its acquisition buffer, for setting, clearing
    /// and copying out of it.
    pub(crate) fn acquisition_buffer_slot(&mut self) -> &mut Option<AcquisitionBuffer> {
        &mut self.buffer
    }

    /// Lists the camera models available for use with [`CameraId::connect_demo`].
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate demo camera models.
    pub fn available_demo_models(library: &Library) -> Result<Vec<Model>> {
        tracing::debug!("enumerating available demo camera models");
        let _ = library; // only taken to prove picam is initialized
        let mut array: *const sys::PicamModel = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::Picam_GetAvailableDemoCameraModels(
            &mut array, &mut count
        ));
        free_on_error!(result, array, sys::Picam_DestroyModels)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let models = unsafe { collect_raw_array(array, count, |&m| Model::from_raw(m)) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyModels(array))?;
        }
        Ok(models)
    }

    /// The raw PICam handle, for other modules' calls.
    pub(crate) fn raw_handle(&self) -> sys::PicamHandle {
        self.handle
    }

    /// The handle PICam registers extrinsic value callbacks on, the camera's
    /// device (PICam manual 7.4.9.3), for `impl_parameter_methods!`.
    fn extrinsic_handle(&self) -> Result<sys::PicamHandle> {
        Ok(self.device_handle())
    }

    /// Sets `parameter`, a handle from [`params`](crate::params), to `value`
    /// and applies it immediately, without needing
    /// [`Camera::commit_parameters`].
    ///
    /// # Errors
    ///
    /// - [`crate::Error::AcquisitionNotInProgress`] outside an acquisition,
    ///   since values are only set online during one (PICam manual 5.4.1.20 to
    ///   5.4.1.22, seen with SDK 5.15)
    /// - [`crate::Error::ParameterIsNotOnlineable`] if `parameter` can't be set
    ///   online, as [`Self::can_set_online`] says (seen with SDK 5.15)
    /// - [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply
    ///   to this camera (seen with SDK 5.15)
    /// - [`crate::Error::ParameterValueIsReadOnly`] if `parameter` is read-only
    ///   (seen with SDK 5.15)
    /// - [`crate::Error::InvalidParameterValue`] if `value` is out of range or
    ///   not one of the values allowed (seen with SDK 5.15)
    /// - otherwise an error if PICam is unable to set it
    pub fn set_online<T: OnlineSettablePicamValue>(
        &self,
        parameter: TypedParameter<T>,
        value: T,
    ) -> Result<()> {
        T::set_online(RawHandle(self.handle), parameter.parameter(), value)
    }

    /// Sets `parameter` to a `value` of a type the caller chooses and applies
    /// it immediately, without needing [`Camera::commit_parameters`].
    ///
    /// Prefer [`Camera::set_online`]. This is for parameters with no handle in
    /// [`params`](crate::params).
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ParameterHasInvalidValueType`] if PICam's online call
    ///   for `T` doesn't accept `parameter`'s value type (seen with SDK 5.15)
    /// - [`crate::Error::AcquisitionNotInProgress`] outside an acquisition,
    ///   since values are only set online during one (PICam manual 5.4.1.20 to
    ///   5.4.1.22, seen with SDK 5.15)
    /// - [`crate::Error::ParameterIsNotOnlineable`] if `parameter` can't be set
    ///   online, as [`Self::can_set_online`] says (seen with SDK 5.15)
    /// - [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply
    ///   to this camera (seen with SDK 5.15)
    /// - [`crate::Error::ParameterValueIsReadOnly`] if `parameter` is read-only
    ///   (seen with SDK 5.15)
    /// - [`crate::Error::InvalidParameterValue`] if `value` is out of range or
    ///   not one of the values allowed (seen with SDK 5.15)
    /// - otherwise an error if PICam is unable to set it
    pub fn set_online_untyped<T: OnlineSettablePicamValue>(
        &self,
        parameter: Parameter,
        value: T,
    ) -> Result<()> {
        T::check_type(RawHandle(self.handle), parameter)?;
        T::set_online(RawHandle(self.handle), parameter, value)
    }

    /// Gets the ROI constraint on `parameter`'s value, if any.
    ///
    /// `category` selects which kind of constraint to query: the values
    /// PICam is capable of accepting, the values currently required, or
    /// the values currently recommended.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't ROI-constrained (seen with SDK 5.15),
    /// [`crate::Error::InvalidConstraintCategory`] if `category` is one PICam
    /// doesn't know (seen with SDK 5.15), or an error if PICam is unable to
    /// query this.
    pub fn rois_constraint(
        &self,
        parameter: impl Into<Parameter>,
        category: ConstraintCategory,
    ) -> Result<RoisConstraint> {
        let mut ptr: *const sys::PicamRoisConstraint = std::ptr::null();
        let result = picam_call!(sys::Picam_GetParameterRoisConstraint(
            self.handle,
            parameter.into().to_raw(),
            category.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyRoisConstraints)?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamRoisConstraint`.
        let constraint = RoisConstraint::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyRoisConstraints(ptr))?;
        Ok(constraint)
    }

    /// Gets the pulse constraint on `parameter`'s value, if any.
    ///
    /// `category` selects which kind of constraint to query: the values
    /// PICam is capable of accepting, the values currently required, or
    /// the values currently recommended.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ParameterHasInvalidConstraintType`] if `parameter`
    ///   isn't pulse-constrained (seen with SDK 5.15)
    /// - [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply
    ///   to this camera (seen with SDK 5.15)
    /// - [`crate::Error::InvalidConstraintCategory`] if `category` is one PICam
    ///   doesn't know (seen with SDK 5.15)
    /// - otherwise an error if PICam is unable to query this
    pub fn pulse_constraint(
        &self,
        parameter: impl Into<Parameter>,
        category: ConstraintCategory,
    ) -> Result<PulseConstraint> {
        let mut ptr: *const sys::PicamPulseConstraint = std::ptr::null();
        let result = picam_call!(sys::Picam_GetParameterPulseConstraint(
            self.handle,
            parameter.into().to_raw(),
            category.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyPulseConstraints)?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamPulseConstraint`.
        let constraint = PulseConstraint::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyPulseConstraints(ptr))?;
        Ok(constraint)
    }

    /// Gets the modulations constraint on `parameter`'s value, if any.
    ///
    /// `category` selects which kind of constraint to query: the values
    /// PICam is capable of accepting, the values currently required, or
    /// the values currently recommended.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ParameterHasInvalidConstraintType`] if `parameter`
    ///   isn't modulations-constrained (seen with SDK 5.15)
    /// - [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply
    ///   to this camera (seen with SDK 5.15)
    /// - [`crate::Error::InvalidConstraintCategory`] if `category` is one PICam
    ///   doesn't know (seen with SDK 5.15)
    /// - otherwise an error if PICam is unable to query this
    pub fn modulations_constraint(
        &self,
        parameter: impl Into<Parameter>,
        category: ConstraintCategory,
    ) -> Result<ModulationsConstraint> {
        let mut ptr: *const sys::PicamModulationsConstraint = std::ptr::null();
        let result = picam_call!(sys::Picam_GetParameterModulationsConstraint(
            self.handle,
            parameter.into().to_raw(),
            category.to_raw(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::Picam_DestroyModulationsConstraints)?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamModulationsConstraint`.
        let constraint = ModulationsConstraint::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyModulationsConstraints(ptr))?;
        Ok(constraint)
    }

    /// Gets every ROI constraint on `parameter`'s value (PICam manual
    /// 7.4.11.1).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't ROI-constrained (seen with SDK 5.15), or an error if
    /// PICam is unable to query this.
    pub fn rois_constraints(&self, parameter: impl Into<Parameter>) -> Result<Vec<RoisConstraint>> {
        let mut array: *const sys::PicamRoisConstraint = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::PicamAdvanced_GetParameterRoisConstraints(
            self.handle,
            parameter.into().to_raw(),
            &mut array,
            &mut count
        ));
        free_on_error!(result, array, sys::Picam_DestroyRoisConstraints)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let constraints = unsafe { collect_raw_array(array, count, RoisConstraint::from_raw) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyRoisConstraints(array))?;
        }
        Ok(constraints)
    }

    /// Gets every pulse constraint on `parameter`'s value (PICam manual
    /// 7.4.11.4).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't pulse-constrained (seen with SDK 5.15),
    /// [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply to
    /// this camera (seen with SDK 5.15), or an error if PICam is unable to
    /// query this.
    pub fn pulse_constraints(
        &self,
        parameter: impl Into<Parameter>,
    ) -> Result<Vec<PulseConstraint>> {
        let mut array: *const sys::PicamPulseConstraint = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::PicamAdvanced_GetParameterPulseConstraints(
            self.handle,
            parameter.into().to_raw(),
            &mut array,
            &mut count
        ));
        free_on_error!(result, array, sys::Picam_DestroyPulseConstraints)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let constraints = unsafe { collect_raw_array(array, count, PulseConstraint::from_raw) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyPulseConstraints(array))?;
        }
        Ok(constraints)
    }

    /// Gets every modulation constraint on `parameter`'s value (PICam manual
    /// 7.4.11.7).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't modulations-constrained (seen with SDK 5.15),
    /// [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply to
    /// this camera (seen with SDK 5.15), or an error if PICam is unable to
    /// query this.
    pub fn modulations_constraints(
        &self,
        parameter: impl Into<Parameter>,
    ) -> Result<Vec<ModulationsConstraint>> {
        let mut array: *const sys::PicamModulationsConstraint = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::PicamAdvanced_GetParameterModulationsConstraints(
            self.handle,
            parameter.into().to_raw(),
            &mut array,
            &mut count
        ));
        free_on_error!(result, array, sys::Picam_DestroyModulationsConstraints)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let constraints =
            unsafe { collect_raw_array(array, count, ModulationsConstraint::from_raw) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyModulationsConstraints(array))?;
        }
        Ok(constraints)
    }

    /// Watches `parameter`'s ROIs constraint, sending the new constraint
    /// each time setting another parameter changes it (PICam manual 7.4.11.2). Each
    /// change is sent before the `set` that caused it returns.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't ROI-constrained (seen with SDK 5.15), or an error if
    /// PICam is unable to register its callback.
    pub fn watch_rois_constraint(
        &self,
        parameter: impl Into<Parameter>,
    ) -> Result<(Watch, Receiver<RoisConstraint>)> {
        watch::subscribe(
            self.handle,
            parameter.into(),
            Kind::RoisConstraint,
            |event| match event {
                Event::RoisConstraint(constraint) => Some(constraint.clone()),
                _ => None,
            },
        )
    }

    /// Watches `parameter`'s pulse constraint, sending the new constraint
    /// each time setting another parameter changes it (PICam manual 7.4.11.5). Each
    /// change is sent before the `set` that caused it returns.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't pulse-constrained (seen with SDK 5.15),
    /// [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply to
    /// this camera (seen with SDK 5.15), or an error if PICam is unable to
    /// register its callback.
    pub fn watch_pulse_constraint(
        &self,
        parameter: impl Into<Parameter>,
    ) -> Result<(Watch, Receiver<PulseConstraint>)> {
        watch::subscribe(
            self.handle,
            parameter.into(),
            Kind::PulseConstraint,
            |event| match event {
                Event::PulseConstraint(constraint) => Some(constraint.clone()),
                _ => None,
            },
        )
    }

    /// Watches `parameter`'s modulations constraint, sending the new constraint
    /// each time setting another parameter changes it (PICam manual 7.4.11.8). Each
    /// change is sent before the `set` that caused it returns.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ParameterHasInvalidConstraintType`] if
    /// `parameter` isn't modulations-constrained (seen with SDK 5.15),
    /// [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply to
    /// this camera (seen with SDK 5.15), or an error if PICam is unable to
    /// register its callback.
    pub fn watch_modulations_constraint(
        &self,
        parameter: impl Into<Parameter>,
    ) -> Result<(Watch, Receiver<ModulationsConstraint>)> {
        watch::subscribe(
            self.handle,
            parameter.into(),
            Kind::ModulationsConstraint,
            |event| match event {
                Event::ModulationsConstraint(constraint) => Some(constraint.clone()),
                _ => None,
            },
        )
    }
}

impl_parameter_methods!(Camera, "camera", acquires);

impl Drop for Camera {
    fn drop(&mut self) {
        // watches on the device (extrinsic ones, and those made through
        // `CameraDevice`) are registered on its handle, so both are cleared
        watch::release(self.device);
        watch::release(self.handle);
        if let Err(e) = picam_call!(sys::Picam_CloseCamera(self.handle)) {
            tracing::error!("failed to close PICam camera: {e}");
            // picam may still be acquiring into the buffer, so leak it rather
            // than free memory picam could write to
            std::mem::forget(self.buffer.take());
        }
    }
}

/// A reference to a camera handle that is open somewhere in this process,
/// without implying ownership of it. It's either a model handle, from
/// [`Camera::list_open_cameras`], or a device handle, from
/// [`Camera::list_open_camera_devices`]: [`CameraHandle::handle_type`] says
/// which.
///
/// Unlike [`Camera`], a `CameraHandle` never closes the underlying handle;
/// it exists purely to let you inspect a camera you don't own (for example,
/// one returned by [`Camera::list_open_cameras`] that some other `Camera` is
/// responsible for). Its lifetime ties it to the [`Library`] it came from,
/// which is the only guarantee PICam actually gives us here: the handle
/// cannot outlive the library, though its true owner may still close it
/// earlier. A call on an already-closed handle fails with
/// [`crate::Error::InvalidHandle`] rather than being unsound, since
/// PICam validates handles against its own internal table before using them.
#[derive(Debug, Clone, Copy)]
pub struct CameraHandle<'a> {
    handle: sys::PicamHandle,
    _marker: std::marker::PhantomData<&'a Library>,
}

impl<'a> CameraHandle<'a> {
    /// Reports whether this is a camera's model or device handle (PICam
    /// manual 7.4.4.1).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to check
    /// this.
    pub fn handle_type(&self) -> Result<HandleType> {
        let mut raw = sys::PicamHandleType(0);
        picam_call!(sys::PicamAdvanced_GetHandleType(self.handle, &mut raw))?;
        Ok(HandleType::from_raw(raw))
    }

    /// The camera's model handle, whichever handle this is (PICam manual
    /// 7.4.3.4).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to look it
    /// up.
    pub fn model(&self) -> Result<CameraHandle<'a>> {
        let mut model: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::PicamAdvanced_GetCameraModel(self.handle, &mut model))?;
        Ok(CameraHandle {
            handle: model,
            _marker: std::marker::PhantomData,
        })
    }

    /// The camera's device handle, whichever handle this is (PICam manual
    /// 7.4.3.5).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to look it
    /// up.
    pub fn device(&self) -> Result<CameraHandle<'a>> {
        let mut device: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::PicamAdvanced_GetCameraDevice(self.handle, &mut device))?;
        Ok(CameraHandle {
            handle: device,
            _marker: std::marker::PhantomData,
        })
    }

    /// Reports whether this camera is currently connected.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to query the
    /// camera's connection status.
    pub fn is_connected(&self) -> Result<bool> {
        handle_is_connected(self.handle)
    }

    /// Reports whether this camera is currently in a faulted state.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to query the
    /// camera's fault status.
    pub fn is_faulted(&self) -> Result<bool> {
        handle_is_faulted(self.handle)
    }

    /// Returns the [`CameraId`] identifying this camera.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to query the
    /// camera's identity.
    pub fn id(&self) -> Result<CameraId> {
        handle_id(self.handle)
    }

    /// The number stored with this handle's camera by
    /// [`Camera::set_user_state`], or 0 if none has been (PICam manual
    /// 7.4.7.1). The camera's model and device handles share it.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidHandle`] if the handle's camera has since
    /// closed (seen with SDK 5.15), or an error if PICam is unable to read it.
    pub fn user_state(&self) -> Result<usize> {
        user_state::get(self.handle)
    }
}

define_picam_enum!(
    /// The kind of thing a PICam handle refers to (PICam manual 7.1.2.1).
    HandleType,
    sys::PicamHandleType,
    PicamEnumeratedType_HandleType,
    /// A physical camera, as [`crate::CameraHandle::device`] and
    /// [`crate::Camera::list_open_camera_devices`] give.
    CameraDevice = PicamHandleType_CameraDevice,
    /// A camera's model, the copy of its parameters in memory, as
    /// [`crate::Camera`] holds and [`crate::Camera::list_open_cameras`] gives.
    CameraModel = PicamHandleType_CameraModel,
    /// An accessory.
    Accessory = PicamHandleType_Accessory,
    /// A camera opened for EM gain calibration.
    #[doc(alias = "EMCalibration")]
    EmCalibration = PicamHandleType_EMCalibration,
);

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
    /// The camera model.
    pub model: Model,
    /// How the camera connects to the computer.
    pub computer_interface: ComputerInterface,
    /// The name of the camera's sensor.
    pub sensor_name: String,
    /// The camera's serial number. PICam appends `:Demo` to a demo camera's
    /// serial number (seen with SDK 5.15).
    pub serial_number: String,
}

impl CameraId {
    /// Converts a raw `PicamCameraID` into an owned `CameraId`.
    pub(crate) fn from_raw(raw: &sys::PicamCameraID) -> Self {
        Self {
            model: Model::from_raw(raw.model),
            computer_interface: ComputerInterface::from_raw(raw.computer_interface),
            sensor_name: c_char_array_to_string(&raw.sensor_name),
            serial_number: c_char_array_to_string(&raw.serial_number),
        }
    }

    /// Converts this [`CameraId`] into a raw `PicamCameraID`, for passing
    /// back into PICam (e.g. via [`Camera::open`]).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ValueTooLong`] if `sensor_name` or
    /// `serial_number` (plus its nul terminator) doesn't fit in PICam's
    /// fixed-size buffers.
    pub(crate) fn to_raw(&self) -> Result<sys::PicamCameraID> {
        Ok(sys::PicamCameraID {
            model: self.model.to_raw(),
            computer_interface: self.computer_interface.to_raw(),
            sensor_name: string_to_c_char_array(&self.sensor_name)?,
            serial_number: string_to_c_char_array(&self.serial_number)?,
        })
    }

    /// Reports whether a camera matching this ID is currently connected,
    /// without opening it.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ValueTooLong`] if this ID's fields don't fit in
    /// PICam's fixed-size buffers, [`crate::Error::InvalidCameraId`] if this ID
    /// matches no camera PICam knows of (seen with SDK 5.15), or an error if
    /// PICam is unable to query connection status.
    pub fn is_connected(&self, library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
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
    /// Returns [`crate::Error::ValueTooLong`] if this ID's fields don't fit in
    /// PICam's fixed-size buffers, [`crate::Error::InvalidCameraId`] if this ID
    /// matches no camera PICam knows of (seen with SDK 5.15), or an error if
    /// PICam is unable to query this.
    pub fn is_open_elsewhere(&self, library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
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
    /// One `library` tries at most 64 different demo cameras, counting each
    /// model and serial number pair once. PICam 5.15 corrupts its own memory,
    /// killing the process, when a 65th different pair is connected in one
    /// library session, even if the others were disconnected (found with demo
    /// cameras, not documented), so this refuses instead. Every attempt
    /// counts, even one that fails, in case a failed attempt uses up one of
    /// PICam's slots too. Trying a pair already tried doesn't count again, and
    /// dropping the `Library` and making a new one starts the count again.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::InvalidString`] if `serial_number` contains an
    ///   embedded null byte
    /// - [`crate::Error::TooManyDemoCameras`] if this would be the 65th
    ///   different demo camera for `library`
    /// - [`crate::Error::InvalidDemoModel`] if `model` isn't a demo model, such
    ///   as a series or an accessory (seen with SDK 5.15)
    /// - [`crate::Error::InvalidDemoSerialNumber`] if `serial_number` is too
    ///   long for PICam (seen with SDK 5.15)
    /// - [`crate::Error::DemoAlreadyConnected`] if this model and serial number
    ///   are already connected (seen with SDK 5.15)
    /// - otherwise an error if PICam is unable to connect it
    pub fn connect_demo(library: &Library, model: Model, serial_number: &str) -> Result<CameraId> {
        tracing::debug!("connecting demo camera: {serial_number}");
        let c_serial_number = std::ffi::CString::new(serial_number)
            .map_err(|_| crate::error::Error::InvalidString)?;
        // counted before the call, whether or not it succeeds, since an
        // attempt might use up one of picam's slots too
        library.note_demo_camera(model, serial_number)?;

        // SAFETY: `PicamCameraID` is a plain-data struct with no invariant
        // that forbids an all-zero bit pattern, `model`/`computer_interface`
        // are newtypes (any i32 is a valid value), and the name fields are
        // just byte arrays, so the zeroed value is valid even before
        // `Picam_ConnectDemoCamera` writes the new camera's id into it on
        // success (PICam manual 4.3.4.3)
        let mut raw: sys::PicamCameraID = unsafe { std::mem::zeroed() };

        picam_call!(sys::Picam_ConnectDemoCamera(
            model.to_raw(),
            c_serial_number.as_ptr(),
            &mut raw
        ))?;
        Ok(CameraId::from_raw(&raw))
    }

    /// Disconnects this demo camera.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ValueTooLong`] if this ID's fields don't fit in
    ///   PICam's fixed-size buffers
    /// - [`crate::Error::InvalidCameraId`] if this ID matches no camera PICam
    ///   knows of (seen with SDK 5.15)
    /// - [`crate::Error::DeviceDisconnected`] if it's already been disconnected
    ///   (seen with SDK 5.15)
    /// - otherwise an error if PICam is unable to disconnect it
    pub fn disconnect_demo(&self, library: &Library) -> Result<()> {
        let _ = library; // only taken to prove picam is initialized
        let raw = self.to_raw()?;
        picam_call!(sys::Picam_DisconnectDemoCamera(&raw))
    }

    /// Reports whether this camera is a simulated ("demo") camera rather
    /// than real hardware.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ValueTooLong`] if this ID's fields don't fit in
    /// PICam's fixed-size buffers, [`crate::Error::InvalidCameraId`] if this ID
    /// matches no camera PICam knows of (seen with SDK 5.15), or an error if
    /// PICam is unable to query this.
    pub fn is_demo(&self, library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
        let raw = self.to_raw()?;
        let mut demo: sys::pibln = 0;
        picam_call!(sys::Picam_IsDemoCamera(&raw, &mut demo))?;
        Ok(demo != 0)
    }

    /// Lists this camera's firmware components.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ValueTooLong`] if this ID's fields don't fit in
    /// PICam's fixed-size buffers, [`crate::Error::InvalidCameraId`] if this ID
    /// matches no camera PICam knows of (seen with SDK 5.15), or an error if
    /// PICam is unable to query firmware details.
    pub fn firmware_details(&self, library: &Library) -> Result<Vec<FirmwareDetail>> {
        let _ = library; // only taken to prove picam is initialized
        let raw_id = self.to_raw()?;
        let mut array: *const sys::PicamFirmwareDetail = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::Picam_GetFirmwareDetails(
            &raw_id, &mut array, &mut count
        ));
        free_on_error!(result, array, sys::Picam_DestroyFirmwareDetails)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let details = unsafe { collect_raw_array(array, count, FirmwareDetail::from_raw) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyFirmwareDetails(array))?;
        }
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
    // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
    let ids = unsafe { collect_raw_array(array, count, CameraId::from_raw) };
    if !array.is_null() {
        picam_call!(sys::Picam_DestroyCameraIDs(array))?;
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::assert_round_trips;

    #[test]
    fn handle_type_round_trips() {
        assert_round_trips!(HandleType, sys::PicamHandleType, 4);
    }
}
