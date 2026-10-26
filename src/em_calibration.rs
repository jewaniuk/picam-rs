//! EM gain calibration: opening a camera for calibration and managing its
//! sensor temperature (`picam_em_calibration.h`, PICam manual chapter 8).
//!
//! Running the calibration itself, `PicamEMCalibration_Calibrate`, reports
//! progress through a callback, so it waits for the callbacks chunk.

use std::sync::Arc;

use crate::camera::CameraId;
use crate::constraint::RangeConstraint;
use crate::convert::collect_raw_array;
use crate::enums::SensorTemperatureStatus;
use crate::error::{Error, Result, picam_call};
use crate::library::Library;
use crate::sys;

/// A camera opened for EM gain calibration (PICam manual chapter 8).
///
/// Opening a camera for calibration is mutually exclusive with opening it
/// normally (manual 8.4.1.1). It closes the calibration on drop.
#[derive(Debug)]
pub struct EmCalibration {
    handle: sys::PicamHandle,
    // keeps the library alive for as long as any `EmCalibration` exists
    _library: Arc<Library>,
}

impl EmCalibration {
    /// Opens the camera `id` for EM gain calibration.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::DemoNotSupported`] for a demo camera (seen with
    /// SDK 5.15), [`crate::Error::ValueTooLong`] if `id`'s fields don't fit in
    /// PICam's fixed-size buffers, or an error if PICam is unable to open it,
    /// for example because the camera is already open.
    pub fn open(id: &CameraId, library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening camera for em calibration: {}", id.serial_number);
        let raw_id = id.to_raw()?;
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::PicamEMCalibration_OpenCalibration(
            &raw_id,
            &mut handle
        ))?;
        Ok(Self {
            handle,
            _library: library,
        })
    }

    /// Lists every calibration handle currently open in this process, whether
    /// or not it was opened through this crate.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate open calibrations.
    pub fn list_open_calibrations(library: &Library) -> Result<Vec<EmCalibrationHandle<'_>>> {
        tracing::debug!("enumerating open em calibrations");
        let _ = library; // ties the returned lifetime to `library`, see `EmCalibrationHandle`
        let mut array: *const sys::PicamHandle = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::PicamEMCalibration_GetOpenCalibrations(
            &mut array, &mut count
        ))?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let handles = unsafe {
            collect_raw_array(array, count, |&handle| EmCalibrationHandle {
                handle,
                _marker: std::marker::PhantomData,
            })
        };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyHandles(array))?;
        }
        Ok(handles)
    }

    /// Returns the [`CameraId`] of the camera being calibrated.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's identity.
    pub fn camera_id(&self) -> Result<CameraId> {
        handle_camera_id(self.handle)
    }

    /// Gets the date of the camera's last EM gain calibration.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to get it.
    pub fn calibration_date(&self) -> Result<EmCalibrationDate> {
        let mut raw = sys::PicamEMCalibrationDate {
            year: 0,
            month: 0,
            day: 0,
        };
        picam_call!(sys::PicamEMCalibration_GetCalibrationDate(
            self.handle,
            &mut raw
        ))?;
        Ok(EmCalibrationDate {
            year: raw.year,
            month: raw.month,
            day: raw.day,
        })
    }

    /// Reads the sensor temperature, in degrees Celsius, directly from the
    /// hardware.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read it.
    pub fn read_sensor_temperature(&self) -> Result<f64> {
        let mut value: sys::piflt = 0.0;
        picam_call!(sys::PicamEMCalibration_ReadSensorTemperatureReading(
            self.handle,
            &mut value
        ))?;
        Ok(value)
    }

    /// Reads the sensor temperature status (unlocked, locked or faulted)
    /// directly from the hardware.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read it.
    pub fn read_sensor_temperature_status(&self) -> Result<SensorTemperatureStatus> {
        let mut value = sys::PicamSensorTemperatureStatus(0);
        picam_call!(sys::PicamEMCalibration_ReadSensorTemperatureStatus(
            self.handle,
            &mut value
        ))?;
        Ok(SensorTemperatureStatus::from_raw(value))
    }

    /// Gets the sensor temperature set point, in degrees Celsius.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to get it.
    pub fn sensor_temperature_set_point(&self) -> Result<f64> {
        let mut value: sys::piflt = 0.0;
        picam_call!(sys::PicamEMCalibration_GetSensorTemperatureSetPoint(
            self.handle,
            &mut value
        ))?;
        Ok(value)
    }

    /// Sets the sensor temperature set point, in degrees Celsius.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to set it, for example because
    /// `value` is outside
    /// [`EmCalibration::sensor_temperature_set_point_constraint`].
    pub fn set_sensor_temperature_set_point(&self, value: f64) -> Result<()> {
        picam_call!(sys::PicamEMCalibration_SetSensorTemperatureSetPoint(
            self.handle,
            value
        ))
    }

    /// Gets the range the sensor temperature set point must fall within.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no constraint, or an error if PICam is unable to
    /// get it.
    pub fn sensor_temperature_set_point_constraint(&self) -> Result<RangeConstraint> {
        let mut ptr: *const sys::PicamRangeConstraint = std::ptr::null();
        picam_call!(
            sys::PicamEMCalibration_GetSensorTemperatureSetPointConstraint(self.handle, &mut ptr)
        )?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamRangeConstraint`.
        let constraint = RangeConstraint::from_raw(unsafe { &*ptr });
        // manual 8.4.3.1 says to free this with
        // `Picam_DestroyCollectionConstraints`, but the header returns a
        // `PicamRangeConstraint`, which only the range destroy takes
        picam_call!(sys::Picam_DestroyRangeConstraints(ptr))?;
        Ok(constraint)
    }
}

impl Drop for EmCalibration {
    fn drop(&mut self) {
        // nowhere to hand a `Result` back to from `Drop`, so just report it
        if let Err(e) = picam_call!(sys::PicamEMCalibration_CloseCalibration(self.handle)) {
            tracing::error!("failed to close PICam em calibration: {e}");
        }
    }
}

/// A non-owning view of a calibration handle that's open somewhere in the
/// process, returned by [`EmCalibration::list_open_calibrations`].
///
/// It never closes the handle. Its lifetime ties it to the [`Library`] the
/// handle was listed with, so it can't outlive PICam itself.
#[derive(Debug, Clone, Copy)]
pub struct EmCalibrationHandle<'a> {
    handle: sys::PicamHandle,
    _marker: std::marker::PhantomData<&'a Library>,
}

impl EmCalibrationHandle<'_> {
    /// Returns the [`CameraId`] of the camera being calibrated.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the camera's identity,
    /// for example if the handle has since been closed.
    pub fn camera_id(&self) -> Result<CameraId> {
        handle_camera_id(self.handle)
    }
}

fn handle_camera_id(handle: sys::PicamHandle) -> Result<CameraId> {
    // SAFETY: `PicamCameraID` is a plain-data struct with no invariant
    // that forbids an all-zero bit pattern, `model`/`computer_interface`
    // are newtypes (any i32 is a valid value), and the name fields are
    // just byte arrays, `PicamEMCalibration_GetCameraID` fully overwrites
    // this before we read any of it back
    let mut raw: sys::PicamCameraID = unsafe { std::mem::zeroed() };
    picam_call!(sys::PicamEMCalibration_GetCameraID(handle, &mut raw))?;
    Ok(CameraId::from_raw(&raw))
}

/// The date of a camera's last EM gain calibration (PICam manual 8.2.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmCalibrationDate {
    /// The year.
    pub year: i32,
    /// The month, as a number.
    pub month: i32,
    /// The day of the month.
    pub day: i32,
}
