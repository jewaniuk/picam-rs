//! A camera's device handle, the physical camera as opposed to the model
//! [`Camera`] holds, and moving parameter values between the two
//! (`picam_advanced.h`, PICam manual 2.6.1, 7.4.3 and 7.4.13).

use std::marker::PhantomData;

use crate::camera::Camera;
use crate::device::impl_parameter_methods;
use crate::error::{Result, picam_call};
use crate::parameter::Parameter;
use crate::sys;

/// A camera's device handle: the physical camera, as opposed to the model
/// [`Camera`] holds, whose parameter values can be set without reaching the
/// hardware until they're committed (PICam manual 2.6.1).
///
/// It borrows the camera, so it can't outlive it. Setting a value through it
/// also sets it on the camera's model (manual 5.4.1.3).
///
/// During an acquisition, one can be made between updates through
/// [`crate::Acquisition::camera`]. PICam then refuses every change through it
/// with [`crate::Error::AcquisitionInProgress`], except to the parameters it
/// can set online, as [`Camera::set_online`] does (seen on demo cameras with
/// SDK 5.15), so the acquisition's data layout can't change.
#[derive(Debug)]
pub struct CameraDevice<'a> {
    handle: sys::PicamHandle,
    _camera: PhantomData<&'a Camera>,
}

impl CameraDevice<'_> {
    /// The handle PICam registers extrinsic value callbacks on, this device's
    /// own (PICam manual 7.4.9.3), for `impl_parameter_methods!`.
    fn extrinsic_handle(&self) -> Result<sys::PicamHandle> {
        Ok(self.handle)
    }
}

impl_parameter_methods!(CameraDevice<'_>, "camera device");

impl Camera {
    /// The camera's device handle, for parameter calls on the physical
    /// camera (PICam manual 7.4.3.5).
    ///
    /// # Errors
    ///
    /// Doesn't fail now; the `Result` is kept so the signature stays the same.
    pub fn device(&self) -> Result<CameraDevice<'_>> {
        Ok(CameraDevice {
            handle: self.device_handle(),
            _camera: PhantomData,
        })
    }

    /// Configures the device with every value stored in the camera's model
    /// (PICam manual 7.4.13.7). If it fails, the device is left as it was.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidParameterValue`] if a value fails
    /// validation (seen on a demo camera with SDK 5.15), or an error if PICam
    /// is unable to configure the device.
    pub fn commit_parameters_to_device(&self) -> Result<()> {
        picam_call!(sys::PicamAdvanced_CommitParametersToCameraDevice(
            self.raw_handle()
        ))
    }

    /// Replaces `parameter`'s value in the camera's model with the device's,
    /// discarding any pending change to it (PICam manual 7.4.13.8).
    ///
    /// Afterwards, [`Camera::are_parameters_committed`] can still report
    /// `false`, even with no other pending change, until
    /// [`Camera::refresh_parameters_from_device`] or
    /// [`Camera::commit_parameters`] (seen on a demo camera with SDK 5.15).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to refresh it.
    pub fn refresh_parameter_from_device(&self, parameter: impl Into<Parameter>) -> Result<()> {
        picam_call!(sys::PicamAdvanced_RefreshParameterFromCameraDevice(
            self.raw_handle(),
            parameter.into().to_raw()
        ))
    }

    /// Replaces every value in the camera's model with the device's,
    /// discarding all pending changes (PICam manual 7.4.13.9).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to refresh them.
    pub fn refresh_parameters_from_device(&self) -> Result<()> {
        picam_call!(sys::PicamAdvanced_RefreshParametersFromCameraDevice(
            self.raw_handle()
        ))
    }
}
