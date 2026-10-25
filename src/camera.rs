//! Camera lifecycle.

use std::sync::Arc;

use crate::constraint::{
    CollectionConstraint, ConstraintCategory, ConstraintType, ModulationsConstraint,
    PulseConstraint, RangeConstraint, RoisConstraint,
};
use crate::convert::{
    FirmwareDetail, c_char_array_to_string, collect_raw_array, string_to_c_char_array,
};
use crate::error::{Result, picam_call};
use crate::library::Library;
use crate::model::Model;
use crate::parameter::sealed::RawHandle;
use crate::parameter::{
    ComputerInterface, EnumeratedType, OnlineSettablePicamValue, Parameter, PicamValue,
    ReadablePicamValue, ValueAccess, ValueType, can_read, can_set_online, can_wait_for_status,
    constraint_type, enumerated_type, is_relevant, value_access, value_type,
};
use crate::sys;

/// An open connection to a PICam camera.
#[derive(Debug)]
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

    /// Lists the camera models available for use with [`CameraId::connect_demo`].
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate demo camera models.
    pub fn available_demo_models() -> Result<Vec<Model>> {
        tracing::debug!("enumerating available demo camera models");
        let mut array: *const sys::PicamModel = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetAvailableDemoCameraModels(
            &mut array, &mut count
        ))?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let models = unsafe { collect_raw_array(array, count, |&m| Model::from_raw(m)) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyModels(array))?;
        }
        Ok(models)
    }

    /// Gets the current value of `parameter`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ParameterDoesNotExist`] if
    /// `parameter` doesn't apply to this camera, or an error if
    /// PICam is unable to read it.
    pub fn get<T: PicamValue>(&self, parameter: Parameter) -> Result<T> {
        T::get(RawHandle(self.handle), parameter)
    }

    /// Sets `parameter` to `value`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ParameterValueIsReadOnly`] if
    /// `parameter` can't be set, [`crate::error::Error::InvalidParameterValue`]
    /// if `value` is out of range, or an error if PICam is unable to set it.
    pub fn set<T: PicamValue>(&self, parameter: Parameter, value: T) -> Result<()> {
        T::set(RawHandle(self.handle), parameter, value)
    }

    /// Gets the factory default value of `parameter`.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read the default value.
    pub fn default_value<T: PicamValue>(&self, parameter: Parameter) -> Result<T> {
        T::default_value(RawHandle(self.handle), parameter)
    }

    /// Reports whether `value` could currently be set for `parameter`,
    /// without actually setting it.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_set<T: PicamValue>(&self, parameter: Parameter, value: T) -> Result<bool> {
        T::can_set(RawHandle(self.handle), parameter, value)
    }

    /// Reports whether `parameter` can be set online (without committing).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_set_online(&self, parameter: Parameter) -> Result<bool> {
        can_set_online(self.handle, parameter)
    }

    /// Reports whether `parameter` can be read directly from hardware
    /// via [`Camera::read`].
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_read(&self, parameter: Parameter) -> Result<bool> {
        can_read(self.handle, parameter)
    }

    /// Reports whether `parameter` is a waitable status parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_wait_for_status(&self, parameter: Parameter) -> Result<bool> {
        can_wait_for_status(self.handle, parameter)
    }

    /// Reports whether `parameter` is currently relevant to this
    /// camera's configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn is_relevant(&self, parameter: Parameter) -> Result<bool> {
        is_relevant(self.handle, parameter)
    }

    /// Gets the underlying value type of `parameter`.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn value_type(&self, parameter: Parameter) -> Result<ValueType> {
        value_type(self.handle, parameter).map(ValueType::from_raw)
    }

    /// Gets the enumerated type `parameter`'s value belongs to.
    ///
    /// Only parameters whose value type is [`ValueType::Enumeration`] are
    /// valid here (PICam manual 5.4.3.6).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn enumerated_type(&self, parameter: Parameter) -> Result<EnumeratedType> {
        enumerated_type(self.handle, parameter).map(EnumeratedType::from_raw)
    }

    /// Gets whether/how `parameter`'s value can currently be accessed.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn value_access(&self, parameter: Parameter) -> Result<ValueAccess> {
        value_access(self.handle, parameter).map(ValueAccess::from_raw)
    }

    /// Gets the kind of constraint that applies to `parameter`'s value.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn constraint_type(&self, parameter: Parameter) -> Result<ConstraintType> {
        constraint_type(self.handle, parameter).map(ConstraintType::from_raw)
    }

    /// Lists the parameters relevant to this camera.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate parameters.
    pub fn parameters(&self) -> Result<Vec<Parameter>> {
        let mut array: *const sys::PicamParameter = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_GetParameters(
            self.handle,
            &mut array,
            &mut count
        ))?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let parameters =
            unsafe { collect_raw_array(array, count, |&raw| Parameter::from_raw(raw)) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyParameters(array))?;
        }
        Ok(parameters)
    }

    /// Reports whether `parameter` applies to this camera.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn does_parameter_exist(&self, parameter: Parameter) -> Result<bool> {
        let mut exists: sys::pibln = 0;
        picam_call!(sys::Picam_DoesParameterExist(
            self.handle,
            parameter.to_raw(),
            &mut exists
        ))?;
        Ok(exists != 0)
    }

    /// Reports whether all pending parameter changes have been committed.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn are_parameters_committed(&self) -> Result<bool> {
        let mut committed: sys::pibln = 0;
        picam_call!(sys::Picam_AreParametersCommitted(
            self.handle,
            &mut committed
        ))?;
        Ok(committed != 0)
    }

    /// Commits all pending parameter changes made via [`Camera::set`],
    /// validating them against PICam's constraints.
    ///
    /// Returns the parameters that failed to commit, if any. An empty
    /// `Vec` means every pending change was accepted.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to attempt the commit at all.
    pub fn commit_parameters(&self) -> Result<Vec<Parameter>> {
        let mut array: *const sys::PicamParameter = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::Picam_CommitParameters(
            self.handle,
            &mut array,
            &mut count
        ))?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let failed = unsafe { collect_raw_array(array, count, |&raw| Parameter::from_raw(raw)) };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyParameters(array))?;
        }
        Ok(failed)
    }

    /// Restores all parameters to their factory default values.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to do this.
    pub fn restore_parameters_to_default_values(&self) -> Result<()> {
        picam_call!(sys::Picam_RestoreParametersToDefaultValues(self.handle))
    }

    /// Lists the discrete values `parameter` (a "status parameter") can take.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query this.
    pub fn status_parameter_purview(&self, parameter: Parameter) -> Result<Vec<i32>> {
        let mut ptr: *const sys::PicamStatusPurview = std::ptr::null();
        picam_call!(sys::Picam_GetStatusParameterPurview(
            self.handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let values = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `values_array` has `values_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(raw.values_array, raw.values_count, |&v| v)
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyStatusPurviews(ptr))?;
        }
        Ok(values)
    }

    /// Estimates how long (in milliseconds) it would take for `parameter`
    /// to reach `value`.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to estimate this.
    pub fn estimate_time_to_status_parameter_value(
        &self,
        parameter: Parameter,
        value: i32,
    ) -> Result<i32> {
        let mut estimated_time: sys::piint = 0;
        picam_call!(sys::Picam_EstimateTimeToStatusParameterValue(
            self.handle,
            parameter.to_raw(),
            value,
            &mut estimated_time
        ))?;
        Ok(estimated_time)
    }

    /// Blocks until `parameter` reaches `value`, or `time_out` milliseconds
    /// have elapsed.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::TimeOutOccurred`] if `timeout`
    /// elapses first, or an error if PICam is unable to wait for this.
    pub fn wait_for_status_parameter_value(
        &self,
        parameter: Parameter,
        value: i32,
        timeout: i32,
    ) -> Result<()> {
        picam_call!(sys::Picam_WaitForStatusParameterValue(
            self.handle,
            parameter.to_raw(),
            value,
            timeout
        ))
    }

    /// Reads the current value of `parameter` directly from hardware,
    /// bypassing PiCam's cached/committed value.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read it.
    pub fn read<T: ReadablePicamValue>(&self, parameter: Parameter) -> Result<T> {
        T::read(RawHandle(self.handle), parameter)
    }

    /// Sets `parameter` to `value` and applies it immediately, without
    /// needing [`Camera::commit_parameters`].
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to set it.
    pub fn set_online<T: OnlineSettablePicamValue>(
        &self,
        parameter: Parameter,
        value: T,
    ) -> Result<()> {
        T::set_online(RawHandle(self.handle), parameter, value)
    }

    /// Gets the collection constraint on `parameter`'s value, if any.
    ///
    /// `category` selects which kind of constraint to query: the values
    /// PICam is capable of accepting, the values currently required, or
    /// the values currently recommended.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ParameterHasInvalidConstraintType`]
    /// if `parameter` isn't collection-constrained, or an error if PICam
    /// is unable to query this.
    pub fn collection_constraint(
        &self,
        parameter: Parameter,
        category: ConstraintCategory,
    ) -> Result<CollectionConstraint> {
        let mut ptr: *const sys::PicamCollectionConstraint = std::ptr::null();
        picam_call!(sys::Picam_GetParameterCollectionConstraint(
            self.handle,
            parameter.to_raw(),
            category.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamCollectionConstraint`.
        let constraint = CollectionConstraint::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyCollectionConstraints(ptr))?;
        Ok(constraint)
    }

    /// Gets the range constraint on `parameter`'s value, if any.
    ///
    /// `category` selects which kind of constraint to query: the values
    /// PICam is capable of accepting, the values currently required, or
    /// the values currently recommended.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ParameterHasInvalidConstraintType`]
    /// if `parameter` isn't range-constrained, or an error if PICam
    /// is unable to query this.
    pub fn range_constraint(
        &self,
        parameter: Parameter,
        category: ConstraintCategory,
    ) -> Result<RangeConstraint> {
        let mut ptr: *const sys::PicamRangeConstraint = std::ptr::null();
        picam_call!(sys::Picam_GetParameterRangeConstraint(
            self.handle,
            parameter.to_raw(),
            category.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamRangeConstraint`.
        let constraint = RangeConstraint::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyRangeConstraints(ptr))?;
        Ok(constraint)
    }

    /// Gets the ROI constraint on `parameter`'s value, if any.
    ///
    /// `category` selects which kind of constraint to query: the values
    /// PICam is capable of accepting, the values currently required, or
    /// the values currently recommended.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::ParameterHasInvalidConstraintType`]
    /// if `parameter` isn't ROI-constrained, or an error if PICam is
    /// unable to query this.
    pub fn rois_constraint(
        &self,
        parameter: Parameter,
        category: ConstraintCategory,
    ) -> Result<RoisConstraint> {
        let mut ptr: *const sys::PicamRoisConstraint = std::ptr::null();
        picam_call!(sys::Picam_GetParameterRoisConstraint(
            self.handle,
            parameter.to_raw(),
            category.to_raw(),
            &mut ptr
        ))?;

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
    /// Returns [`crate::error::Error::ParameterHasInvalidConstraintType`]
    /// if `parameter` isn't pulse-constrained, or an error if PICam is
    /// unable to query this.
    pub fn pulse_constraint(
        &self,
        parameter: Parameter,
        category: ConstraintCategory,
    ) -> Result<PulseConstraint> {
        let mut ptr: *const sys::PicamPulseConstraint = std::ptr::null();
        picam_call!(sys::Picam_GetParameterPulseConstraint(
            self.handle,
            parameter.to_raw(),
            category.to_raw(),
            &mut ptr
        ))?;

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
    /// Returns [`crate::error::Error::ParameterHasInvalidConstraintType`]
    /// if `parameter` isn't modulations-constrained, or an error if PICam is
    /// unable to query this.
    pub fn modulations_constraint(
        &self,
        parameter: Parameter,
        category: ConstraintCategory,
    ) -> Result<ModulationsConstraint> {
        let mut ptr: *const sys::PicamModulationsConstraint = std::ptr::null();
        picam_call!(sys::Picam_GetParameterModulationsConstraint(
            self.handle,
            parameter.to_raw(),
            category.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamModulationsConstraint`.
        let constraint = ModulationsConstraint::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyModulationsConstraints(ptr))?;
        Ok(constraint)
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
    fn from_raw(raw: &sys::PicamCameraID) -> Self {
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
    /// Returns [`crate::error::Error::ValueTooLong`] if `sensor_name` or
    /// `serial_number` (plus its nul terminator) doesn't fit in PICam's
    /// fixed-size buffers.
    fn to_raw(&self) -> Result<sys::PicamCameraID> {
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
    pub fn connect_demo(model: Model, serial_number: &str) -> Result<CameraId> {
        tracing::debug!("connecting demo camera: {serial_number}");
        let c_serial_number = std::ffi::CString::new(serial_number)
            .map_err(|_| crate::error::Error::InvalidString)?;

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
