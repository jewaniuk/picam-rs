//! User state: a number PICam keeps with each open camera or accessory
//! (`picam_advanced.h`, PICam manual 7.4.7).
//!
//! PICam stores it as a `void*`, for C callers to reach their own state from
//! callbacks. This crate's callbacks don't need that, so it's offered as a
//! plain number, which neither this crate nor PICam ever dereferences.

use std::ffi::c_void;

use crate::error::{Result, picam_call};
use crate::sys;

/// Reads `handle`'s user state (manual 7.4.7.1).
pub(crate) fn get(handle: sys::PicamHandle) -> Result<usize> {
    let mut state: *mut c_void = std::ptr::null_mut();
    picam_call!(sys::PicamAdvanced_GetUserState(handle, &mut state))?;
    Ok(state.addr())
}

/// Sets `handle`'s user state (manual 7.4.7.2).
pub(crate) fn set(handle: sys::PicamHandle, state: usize) -> Result<()> {
    picam_call!(sys::PicamAdvanced_SetUserState(
        handle,
        std::ptr::without_provenance_mut(state)
    ))
}
