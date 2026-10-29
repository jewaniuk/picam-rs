//! Accessory lifecycle: identifying, opening and querying accessories such
//! as light sources (`picam_accessory.h`).

use std::sync::Arc;

use crate::convert::{
    FirmwareDetail, c_char_array_to_string, collect_raw_array, string_to_c_char_array,
};
use crate::device::impl_parameter_methods;
use crate::error::{Error, Result, picam_call};
use crate::library::Library;
use crate::model::Model;
use crate::parameter::ComputerInterface;
use crate::sys;
use crate::user_state;
use crate::watch;

/// An open connection to a PICam accessory, such as a light source.
///
/// It closes the accessory on drop. Accessories are configured through the
/// same parameter methods as [`Camera`](crate::Camera): those
/// `picam_accessory.h` lists for accessories. Accessory parameters take
/// integer, on/off, enum and floating-point values. The value methods are
/// generic, so calls with other value types (`i64`, `Vec<Roi>`, `Pulse`,
/// `Vec<Modulation>`) compile, but PICam refuses them.
///
/// Unlike a camera's, an accessory's parameter changes apply to the hardware
/// at once, so it always counts as committed (PICam manual 5.4.6):
/// [`Accessory::commit_parameters`] and
/// [`Accessory::are_parameters_committed`] are there for completeness and
/// do nothing useful.
///
/// ```no_run
/// use std::sync::Arc;
///
/// use picam_rs::{Accessory, Library, LightSource, params};
///
/// # fn main() -> picam_rs::Result<()> {
/// let library = Arc::new(Library::new()?);
/// let lamp = Accessory::open_first(library)?;
/// if lamp.does_parameter_exist(params::LIGHT_SOURCE)? {
///     // applies at once, no commit needed
///     lamp.set(params::LIGHT_SOURCE, LightSource::Qth)?;
///     println!("status: {:?}", lamp.get(params::LIGHT_SOURCE_STATUS)?);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Accessory {
    handle: sys::PicamHandle,
    // keeps the library alive for as long as any `Accessory` exists
    _library: Arc<Library>,
}

impl Accessory {
    /// Opens the first available accessory.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::NoAccessoriesAvailable`] if no accessory is
    /// connected (seen with SDK 5.15), or an error if PICam is unable to open
    /// it.
    pub fn open_first(library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening first available accessory");
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::PicamAccessory_OpenFirstAccessory(&mut handle))?;
        Ok(Self {
            handle,
            _library: library,
        })
    }

    /// Opens a specific accessory, as identified by
    /// [`Accessory::available_ids`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidAccessoryId`] if `id` doesn't match an
    /// available accessory (seen with SDK 5.15),
    /// [`crate::Error::ValueTooLong`] if `id`'s serial number doesn't fit in
    /// PICam's buffer, or an error if PICam is unable to open it.
    pub fn open(id: &AccessoryId, library: Arc<Library>) -> Result<Self> {
        tracing::debug!("opening accessory: {}", id.serial_number);
        let raw_id = id.to_raw()?;
        let mut handle: sys::PicamHandle = std::ptr::null_mut();
        picam_call!(sys::PicamAccessory_OpenAccessory(&raw_id, &mut handle))?;
        Ok(Self {
            handle,
            _library: library,
        })
    }

    /// Lists every accessory handle currently open in this process, whether
    /// or not it was opened through this crate.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate open accessories.
    pub fn list_open_accessories(library: &Library) -> Result<Vec<AccessoryHandle<'_>>> {
        tracing::debug!("enumerating open accessories");
        let _ = library; // ties the returned lifetime to `library`, see `AccessoryHandle`
        let mut array: *const sys::PicamHandle = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::PicamAccessory_GetOpenAccessories(
            &mut array, &mut count
        ))?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let handles = unsafe {
            collect_raw_array(array, count, |&handle| AccessoryHandle {
                handle,
                _marker: std::marker::PhantomData,
            })
        };

        if !array.is_null() {
            picam_call!(sys::Picam_DestroyHandles(array))?;
        }
        Ok(handles)
    }

    /// Lists the currently available (connectable, not yet open)
    /// accessories.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate accessories.
    pub fn available_ids(library: &Library) -> Result<Vec<AccessoryId>> {
        tracing::debug!("enumerating available accessories");
        let _ = library; // only taken to prove picam is initialized
        let mut array: *const sys::PicamAccessoryID = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::PicamAccessory_GetAvailableAccessoryIDs(
            &mut array, &mut count
        ))?;
        collect_and_destroy_ids(array, count)
    }

    /// Lists accessories that were previously available but have since been
    /// disconnected or opened elsewhere.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to enumerate accessories.
    pub fn unavailable_ids(library: &Library) -> Result<Vec<AccessoryId>> {
        tracing::debug!("enumerating unavailable accessories");
        let _ = library; // only taken to prove picam is initialized
        let mut array: *const sys::PicamAccessoryID = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::PicamAccessory_GetUnavailableAccessoryIDs(
            &mut array, &mut count
        ))?;
        collect_and_destroy_ids(array, count)
    }

    /// Reports whether this accessory is still connected.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the accessory's
    /// connection status.
    pub fn is_connected(&self) -> Result<bool> {
        handle_is_connected(self.handle)
    }

    /// Returns the [`AccessoryId`] identifying this open accessory.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the accessory's identity.
    pub fn id(&self) -> Result<AccessoryId> {
        handle_id(self.handle)
    }

    /// The number [`Accessory::set_user_state`] last stored with this
    /// accessory, or 0 if none has been (PICam manual 7.4.7.1).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read it.
    pub fn user_state(&self) -> Result<usize> {
        user_state::get(self.handle)
    }

    /// Stores `state`, a number of your choosing, with this accessory, for
    /// example to recognize it later among [`Accessory::list_open_accessories`]
    /// (PICam manual 7.4.7.2), where [`AccessoryHandle::user_state`] reads it
    /// back.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to store it.
    pub fn set_user_state(&self, state: usize) -> Result<()> {
        user_state::set(self.handle, state)
    }

    /// Gets the accessory's light source reference: the intensity in counts
    /// (`y`) at each wavelength in nanometres (`x`) (PICam manual 7.4.6.1).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no reference, or an error if PICam is unable to
    /// get it.
    pub fn light_source_reference(&self) -> Result<Vec<CalibrationPoint>> {
        let mut ptr: *const sys::PicamCalibration = std::ptr::null();
        picam_call!(sys::PicamAccessory_GetLightSourceReference(
            self.handle,
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamCalibration`, whose `point_array` has `point_count` valid,
        // contiguous entries (see `collect_raw_array`)
        let points = unsafe {
            let calibration = &*ptr;
            collect_raw_array(
                calibration.point_array,
                calibration.point_count,
                CalibrationPoint::from_raw,
            )
        };
        picam_call!(sys::Picam_DestroyCalibrations(ptr))?;
        Ok(points)
    }
}

impl Accessory {
    /// The handle PICam registers extrinsic value callbacks on, the
    /// accessory's own (PICam manual 7.4.9.3), for `impl_parameter_methods!`.
    fn extrinsic_handle(&self) -> Result<sys::PicamHandle> {
        Ok(self.handle)
    }
}

impl_parameter_methods!(Accessory, "accessory");

impl Drop for Accessory {
    fn drop(&mut self) {
        watch::release(self.handle);
        // nowhere to hand a `Result` back to from `Drop`, so just report it
        if let Err(e) = picam_call!(sys::PicamAccessory_CloseAccessory(self.handle)) {
            tracing::error!("failed to close PICam accessory: {e}");
        }
    }
}

/// A non-owning view of an accessory handle that's open somewhere in the
/// process, returned by [`Accessory::list_open_accessories`].
///
/// It never closes the handle. Its lifetime ties it to the [`Library`] the
/// handle was listed with, so it can't outlive PICam itself.
#[derive(Debug, Clone, Copy)]
pub struct AccessoryHandle<'a> {
    handle: sys::PicamHandle,
    _marker: std::marker::PhantomData<&'a Library>,
}

impl AccessoryHandle<'_> {
    /// Reports whether this accessory is still connected.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the accessory's
    /// connection status, for example if the handle has since been closed.
    pub fn is_connected(&self) -> Result<bool> {
        handle_is_connected(self.handle)
    }

    /// Returns the [`AccessoryId`] identifying this open accessory.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to query the accessory's identity,
    /// for example if the handle has since been closed.
    pub fn id(&self) -> Result<AccessoryId> {
        handle_id(self.handle)
    }

    /// The number stored with this accessory by [`Accessory::set_user_state`],
    /// or 0 if none has been (PICam manual 7.4.7.1).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to read it, for example if the
    /// handle has since been closed.
    pub fn user_state(&self) -> Result<usize> {
        user_state::get(self.handle)
    }
}

fn handle_is_connected(handle: sys::PicamHandle) -> Result<bool> {
    let mut connected: sys::pibln = 0;
    picam_call!(sys::PicamAccessory_IsAccessoryConnected(
        handle,
        &mut connected
    ))?;
    Ok(connected != 0)
}

fn handle_id(handle: sys::PicamHandle) -> Result<AccessoryId> {
    // SAFETY: `PicamAccessoryID` is a plain-data struct with no invariant
    // that forbids an all-zero bit pattern, `model`/`computer_interface`
    // are newtypes (any i32 is a valid value), and the serial number is just
    // a byte array, `PicamAccessory_GetAccessoryID` fully overwrites this
    // before we read any of it back
    let mut raw: sys::PicamAccessoryID = unsafe { std::mem::zeroed() };
    picam_call!(sys::PicamAccessory_GetAccessoryID(handle, &mut raw))?;
    Ok(AccessoryId::from_raw(&raw))
}

/// Identifies an accessory, whether or not it is open (PICam manual 4.2.2).
#[derive(Debug, Clone)]
pub struct AccessoryId {
    /// The accessory's model.
    pub model: Model,
    /// How the accessory is connected to the computer.
    pub computer_interface: ComputerInterface,
    /// The accessory's serial number.
    pub serial_number: String,
}

impl AccessoryId {
    pub(crate) fn from_raw(raw: &sys::PicamAccessoryID) -> Self {
        Self {
            model: Model::from_raw(raw.model),
            computer_interface: ComputerInterface::from_raw(raw.computer_interface),
            serial_number: c_char_array_to_string(&raw.serial_number),
        }
    }

    fn to_raw(&self) -> Result<sys::PicamAccessoryID> {
        Ok(sys::PicamAccessoryID {
            model: self.model.to_raw(),
            computer_interface: self.computer_interface.to_raw(),
            serial_number: string_to_c_char_array(&self.serial_number)?,
        })
    }

    /// Reports whether this accessory is connected.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidAccessoryId`] if this ID matches no
    /// accessory PICam knows of (seen with SDK 5.15),
    /// [`crate::Error::ValueTooLong`] if its serial number doesn't fit in
    /// PICam's buffer, or an error if PICam is unable to query this.
    pub fn is_connected(&self, library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
        let raw = self.to_raw()?;
        let mut connected: sys::pibln = 0;
        picam_call!(sys::PicamAccessory_IsAccessoryIDConnected(
            &raw,
            &mut connected
        ))?;
        Ok(connected != 0)
    }

    /// Reports whether this accessory is open in another process.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidAccessoryId`] if this ID matches no
    /// accessory PICam knows of (seen with SDK 5.15),
    /// [`crate::Error::ValueTooLong`] if its serial number doesn't fit in
    /// PICam's buffer, or an error if PICam is unable to query this.
    pub fn is_open_elsewhere(&self, library: &Library) -> Result<bool> {
        let _ = library; // only taken to prove picam is initialized
        let raw = self.to_raw()?;
        let mut open_elsewhere: sys::pibln = 0;
        picam_call!(sys::PicamAccessory_IsAccessoryIDOpenElsewhere(
            &raw,
            &mut open_elsewhere
        ))?;
        Ok(open_elsewhere != 0)
    }

    /// Lists this accessory's firmware components.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidAccessoryId`] if this ID matches no
    /// accessory PICam knows of (seen with SDK 5.15),
    /// [`crate::Error::ValueTooLong`] if its serial number doesn't fit in
    /// PICam's buffer, or an error if PICam is unable to query this.
    pub fn firmware_details(&self, library: &Library) -> Result<Vec<FirmwareDetail>> {
        let _ = library; // only taken to prove picam is initialized
        let raw_id = self.to_raw()?;
        let mut array: *const sys::PicamFirmwareDetail = std::ptr::null();
        let mut count: sys::piint = 0;
        picam_call!(sys::PicamAccessory_GetFirmwareDetails(
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

/// Copies PICam's accessory ID array into owned values, then frees it.
fn collect_and_destroy_ids(
    array: *const sys::PicamAccessoryID,
    count: sys::piint,
) -> Result<Vec<AccessoryId>> {
    // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
    let ids = unsafe { collect_raw_array(array, count, AccessoryId::from_raw) };
    if !array.is_null() {
        picam_call!(sys::PicamAccessory_DestroyAccessoryIDs(array))?;
    }
    Ok(ids)
}

/// One point of a PICam calibration (PICam manual 4.2.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationPoint {
    /// The point's x coordinate.
    pub x: f64,
    /// The point's y coordinate.
    pub y: f64,
}

impl CalibrationPoint {
    fn from_raw(raw: &sys::PicamCalibrationPoint) -> Self {
        Self { x: raw.x, y: raw.y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        let id = AccessoryId {
            model: Model::FERGIE_QTH,
            computer_interface: ComputerInterface::Usb2,
            serial_number: "ACC0001".to_string(),
        };
        let raw = id.to_raw().expect("a short serial number should fit");
        let back = AccessoryId::from_raw(&raw);
        assert_eq!(back.model, id.model);
        assert_eq!(back.computer_interface, id.computer_interface);
        assert_eq!(back.serial_number, id.serial_number);
    }

    #[test]
    fn long_serial_numbers_are_refused() {
        let id = AccessoryId {
            model: Model::FERGIE_QTH,
            computer_interface: ComputerInterface::Usb2,
            serial_number: "X".repeat(1000),
        };
        assert_eq!(id.to_raw().err(), Some(Error::ValueTooLong));
    }

    #[test]
    fn calibration_points_convert() {
        let raw = sys::PicamCalibrationPoint {
            x: 435.8,
            y: 1200.0,
        };
        assert_eq!(
            CalibrationPoint::from_raw(&raw),
            CalibrationPoint {
                x: 435.8,
                y: 1200.0
            }
        );
    }
}
