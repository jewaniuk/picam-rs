//! Watching parameters for changes through PICam's callbacks
//! (`picam_advanced.h`, PICam manual 7.3, 7.4.8, 7.4.9 and 7.4.10).
//!
//! PICam passes a callback only the handle, the parameter and the new value,
//! never a pointer of the caller's. So there is one trampoline per kind of
//! callback, and it finds the watches in a global registry keyed by all three.
//! A trampoline only copies the value and sends it down each watch's channel,
//! so no user code runs inside PICam.
//!
//! Two locks keep this free of deadlocks:
//! - `REGISTRATIONS` counts each key's watches. It is held while calling PICam
//!   to register or unregister the key's callback, which happens once per key.
//! - `SINKS` holds the senders, and is the only lock a trampoline takes. It's
//!   never held across a PICam call, so a PICam unregister that waits for a
//!   running callback can't wait on this lock.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::convert::collect_raw_array;
use crate::error::{Result, picam_call};
use crate::parameter::{Modulation, Parameter, Pulse, Roi, ValueAccess};
use crate::sys;

/// The kind of PICam callback a watch is registered through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Integer,
    ExtrinsicInteger,
    LargeInteger,
    FloatingPoint,
    ExtrinsicFloatingPoint,
    Rois,
    Pulse,
    Modulations,
    IsRelevant,
    ValueAccess,
}

/// A copy of what one callback reported, made before the callback returns.
pub(crate) enum Event {
    Integer(i32),
    LargeInteger(i64),
    FloatingPoint(f64),
    Rois(Vec<Roi>),
    Pulse(Pulse),
    Modulations(Vec<Modulation>),
    IsRelevant(bool),
    ValueAccess(ValueAccess),
}

/// Sends one watch's events down its channel, as its value type.
type Sink = Box<dyn Fn(&Event) + Send>;

/// What a callback is registered for: a handle, kept as an address since
/// raw pointers can't be shared between threads, a parameter and a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    handle: usize,
    parameter: i32,
    kind: Kind,
}

impl Key {
    fn new(handle: sys::PicamHandle, parameter: sys::PicamParameter, kind: Kind) -> Self {
        Self {
            handle: handle as usize,
            parameter: parameter.0,
            kind,
        }
    }
}

/// How many watches each key has. Held while calling PICam to register or
/// unregister the key's callback.
static REGISTRATIONS: Mutex<BTreeMap<Key, usize>> = Mutex::new(BTreeMap::new());

/// Each key's sinks, with the ID of the watch that owns each. The only lock a
/// trampoline takes.
static SINKS: Mutex<BTreeMap<Key, Vec<(u64, Sink)>>> = Mutex::new(BTreeMap::new());

/// The next watch's ID.
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Locks `mutex` even if a panic poisoned it, since both maps change in
/// single steps that can't be left half-done.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A watch on a parameter, which ends when dropped, or when the camera or
/// accessory it watches closes.
///
/// The `watch_*` methods of [`Camera`](crate::Camera),
/// [`CameraDevice`](crate::CameraDevice) and [`Accessory`](crate::Accessory)
/// return one with the [`Receiver`] its events arrive on. It doesn't borrow
/// what it watches, so it can be kept during an acquisition, for example to
/// follow the sensor temperature, and the receiver can move to any thread.
/// Keep it for as long as you want events, as in `let (_watch, events) = ...`.
/// Writing `let (_, events) = ...` drops it at once, and the receiver reports
/// the channel disconnected.
///
/// Dropping it unregisters PICam's callback once no other watch on the same
/// parameter needs it. When the camera or accessory closes first, it clears
/// its watches itself, and their receivers report the channel disconnected.
/// Dropping a watch after that does nothing. Events already sent stay
/// readable either way, and leaking a watch with [`std::mem::forget`] leaves
/// nothing behind once its camera closes.
pub struct Watch {
    key: Key,
    id: u64,
    handle: sys::PicamHandle,
}

impl Watch {
    /// Adds `sink` for `kind` callbacks on `handle`'s `parameter`, and
    /// registers PICam's callback if no other watch has.
    fn new(handle: sys::PicamHandle, parameter: Parameter, kind: Kind, sink: Sink) -> Result<Self> {
        let raw = parameter.to_raw();
        let key = Key::new(handle, raw, kind);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut registrations = lock(&REGISTRATIONS);
        // the sink goes in first, so no event between picam registering the
        // callback and this returning is lost
        lock(&SINKS).entry(key).or_default().push((id, sink));
        let count = registrations.get(&key).copied().unwrap_or(0);
        if count == 0
            && let Err(e) = register(handle, raw, kind)
        {
            remove_sink(key, id);
            return Err(e);
        }
        registrations.insert(key, count + 1);
        Ok(Self { key, id, handle })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let mut registrations = lock(&REGISTRATIONS);
        // gone already if the camera closed first, and its handle may since
        // belong to another camera, so it mustn't be touched
        if !remove_sink(self.key, self.id) {
            return;
        }
        let count = registrations.get(&self.key).copied().unwrap_or(1);
        if count <= 1 {
            registrations.remove(&self.key);
            let parameter = sys::PicamParameter(self.key.parameter);
            if let Err(e) = unregister(self.handle, parameter, self.key.kind) {
                tracing::error!("failed to unregister a PICam callback: {e}");
            }
        } else {
            registrations.insert(self.key, count - 1);
        }
    }
}

impl fmt::Debug for Watch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watch")
            .field(
                "parameter",
                &Parameter::from_raw(sys::PicamParameter(self.key.parameter)),
            )
            .field("kind", &self.key.kind)
            .finish_non_exhaustive()
    }
}

/// Removes watch `id`'s sink from `key`, dropping its sender. Returns whether
/// it was there.
fn remove_sink(key: Key, id: u64) -> bool {
    let mut sinks = lock(&SINKS);
    let Some(list) = sinks.get_mut(&key) else {
        return false;
    };
    let before = list.len();
    list.retain(|(sink_id, _)| *sink_id != id);
    let removed = list.len() < before;
    if list.is_empty() {
        sinks.remove(&key);
    }
    removed
}

/// Unregisters every callback registered on `handle` and removes its watches'
/// sinks, so their receivers report the channel disconnected. A camera or
/// accessory calls this just before it closes, while `handle` is still
/// valid. The watches themselves then do nothing when dropped.
pub(crate) fn release(handle: sys::PicamHandle) {
    let address = handle as usize;
    let mut registrations = lock(&REGISTRATIONS);
    let keys: Vec<Key> = registrations
        .keys()
        .filter(|key| key.handle == address)
        .copied()
        .collect();
    for key in keys {
        registrations.remove(&key);
        let parameter = sys::PicamParameter(key.parameter);
        if let Err(e) = unregister(handle, parameter, key.kind) {
            tracing::error!("failed to unregister a PICam callback: {e}");
        }
    }
    lock(&SINKS).retain(|key, _| key.handle != address);
}

/// Watches `parameter` on `handle` through `kind` callbacks, sending each
/// event `convert` turns into a `T`.
///
/// # Errors
///
/// Returns an error if PICam is unable to register the callback.
pub(crate) fn subscribe<T: Send + 'static>(
    handle: sys::PicamHandle,
    parameter: Parameter,
    kind: Kind,
    convert: fn(&Event) -> Option<T>,
) -> Result<(Watch, Receiver<T>)> {
    let (sender, receiver) = mpsc::channel();
    let sink: Sink = Box::new(move |event| {
        if let Some(value) = convert(event) {
            // a dropped receiver only means nobody is listening
            let _ = sender.send(value);
        }
    });
    Ok((Watch::new(handle, parameter, kind, sink)?, receiver))
}

/// Sends `event` to every sink on `handle`'s `parameter` for `kind`. Always
/// reports success to PICam, since the manual doesn't say what PICam does
/// with an error.
fn dispatch(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    kind: Kind,
    event: &Event,
) -> sys::PicamError {
    let sinks = lock(&SINKS);
    if let Some(list) = sinks.get(&Key::new(handle, parameter, kind)) {
        for (_, sink) in list {
            sink(event);
        }
    }
    sys::PicamError::PicamError_None
}

/// Defines `register` and `unregister`, which hand each kind's trampoline to
/// PICam.
macro_rules! callbacks {
    ($($kind:ident => $register:ident, $unregister:ident, $trampoline:ident;)*) => {
        /// Registers `kind`'s trampoline for `handle`'s `parameter`.
        fn register(
            handle: sys::PicamHandle,
            parameter: sys::PicamParameter,
            kind: Kind,
        ) -> Result<()> {
            match kind {
                $(Kind::$kind => picam_call!(sys::$register(handle, parameter, Some($trampoline))),)*
            }
        }

        /// Unregisters `kind`'s trampoline for `handle`'s `parameter`.
        fn unregister(
            handle: sys::PicamHandle,
            parameter: sys::PicamParameter,
            kind: Kind,
        ) -> Result<()> {
            match kind {
                $(Kind::$kind => picam_call!(sys::$unregister(handle, parameter, Some($trampoline))),)*
            }
        }
    };
}

callbacks! {
    Integer => PicamAdvanced_RegisterForIntegerValueChanged,
        PicamAdvanced_UnregisterForIntegerValueChanged, integer_changed;
    ExtrinsicInteger => PicamAdvanced_RegisterForExtrinsicIntegerValueChanged,
        PicamAdvanced_UnregisterForExtrinsicIntegerValueChanged, extrinsic_integer_changed;
    LargeInteger => PicamAdvanced_RegisterForLargeIntegerValueChanged,
        PicamAdvanced_UnregisterForLargeIntegerValueChanged, large_integer_changed;
    FloatingPoint => PicamAdvanced_RegisterForFloatingPointValueChanged,
        PicamAdvanced_UnregisterForFloatingPointValueChanged, floating_point_changed;
    ExtrinsicFloatingPoint => PicamAdvanced_RegisterForExtrinsicFloatingPointValueChanged,
        PicamAdvanced_UnregisterForExtrinsicFloatingPointValueChanged,
        extrinsic_floating_point_changed;
    Rois => PicamAdvanced_RegisterForRoisValueChanged,
        PicamAdvanced_UnregisterForRoisValueChanged, rois_changed;
    Pulse => PicamAdvanced_RegisterForPulseValueChanged,
        PicamAdvanced_UnregisterForPulseValueChanged, pulse_changed;
    Modulations => PicamAdvanced_RegisterForModulationsValueChanged,
        PicamAdvanced_UnregisterForModulationsValueChanged, modulations_changed;
    IsRelevant => PicamAdvanced_RegisterForIsRelevantChanged,
        PicamAdvanced_UnregisterForIsRelevantChanged, is_relevant_changed;
    ValueAccess => PicamAdvanced_RegisterForValueAccessChanged,
        PicamAdvanced_UnregisterForValueAccessChanged, value_access_changed;
}

/// PICam's integer value callback (manual 7.3.4.1).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamIntegerValueChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn integer_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: sys::piint,
) -> sys::PicamError {
    dispatch(handle, parameter, Kind::Integer, &Event::Integer(value))
}

/// PICam's extrinsic integer value callback (manual 7.3.4.1 and 7.4.9.3).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamIntegerValueChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn extrinsic_integer_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: sys::piint,
) -> sys::PicamError {
    dispatch(
        handle,
        parameter,
        Kind::ExtrinsicInteger,
        &Event::Integer(value),
    )
}

/// PICam's large integer value callback (manual 7.3.3.1).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamLargeIntegerValueChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn large_integer_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: sys::pi64s,
) -> sys::PicamError {
    dispatch(
        handle,
        parameter,
        Kind::LargeInteger,
        &Event::LargeInteger(value),
    )
}

/// PICam's floating-point value callback (manual 7.3.4.2).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamFloatingPointValueChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn floating_point_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: sys::piflt,
) -> sys::PicamError {
    dispatch(
        handle,
        parameter,
        Kind::FloatingPoint,
        &Event::FloatingPoint(value),
    )
}

/// PICam's extrinsic floating-point value callback (manual 7.3.4.2 and
/// 7.4.9.7).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamFloatingPointValueChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn extrinsic_floating_point_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: sys::piflt,
) -> sys::PicamError {
    dispatch(
        handle,
        parameter,
        Kind::ExtrinsicFloatingPoint,
        &Event::FloatingPoint(value),
    )
}

/// PICam's relevance callback (manual 7.3.4.4).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamIsRelevantChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn is_relevant_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    relevant: sys::pibln,
) -> sys::PicamError {
    dispatch(
        handle,
        parameter,
        Kind::IsRelevant,
        &Event::IsRelevant(relevant != 0),
    )
}

/// PICam's value access callback (manual 7.3.4.5).
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match `PicamValueAccessChangedCallback`, the type
/// bindgen gives PICam's callback.
unsafe extern "C" fn value_access_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    access: sys::PicamValueAccess,
) -> sys::PicamError {
    dispatch(
        handle,
        parameter,
        Kind::ValueAccess,
        &Event::ValueAccess(ValueAccess::from_raw(access)),
    )
}

/// PICam's ROI value callback (manual 7.3.3.2).
///
/// # Safety
///
/// `value` must be null, or point to a valid `PicamRois` whose `roi_array`
/// has `roi_count` valid entries, for the duration of the call.
unsafe extern "C" fn rois_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: *const sys::PicamRois,
) -> sys::PicamError {
    let rois = if value.is_null() {
        Vec::new()
    } else {
        // SAFETY: the caller guarantees `value` and its array are valid for
        // this call, and the regions are copied before it returns
        unsafe {
            let raw = &*value;
            collect_raw_array(raw.roi_array, raw.roi_count, Roi::from_raw)
        }
    };
    dispatch(handle, parameter, Kind::Rois, &Event::Rois(rois))
}

/// PICam's pulse value callback (manual 7.3.3.3).
///
/// # Safety
///
/// `value` must be null or point to a valid `PicamPulse` for the duration of
/// the call.
unsafe extern "C" fn pulse_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: *const sys::PicamPulse,
) -> sys::PicamError {
    if value.is_null() {
        tracing::warn!("PICam sent a pulse value callback with no value");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `value` is non-null, and the caller guarantees it
    // points to a valid `PicamPulse` for this call, copied before it returns
    let pulse = Pulse::from_raw(unsafe { &*value });
    dispatch(handle, parameter, Kind::Pulse, &Event::Pulse(pulse))
}

/// PICam's modulations value callback (manual 7.3.3.4).
///
/// # Safety
///
/// `value` must be null, or point to a valid `PicamModulations` whose
/// `modulation_array` has `modulation_count` valid entries, for the duration
/// of the call.
unsafe extern "C" fn modulations_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: *const sys::PicamModulations,
) -> sys::PicamError {
    let modulations = if value.is_null() {
        Vec::new()
    } else {
        // SAFETY: the caller guarantees `value` and its array are valid for
        // this call, and the modulations are copied before it returns
        unsafe {
            let raw = &*value;
            collect_raw_array(
                raw.modulation_array,
                raw.modulation_count,
                Modulation::from_raw,
            )
        }
    };
    dispatch(
        handle,
        parameter,
        Kind::Modulations,
        &Event::Modulations(modulations),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::TryRecvError;

    use super::*;

    /// A handle address no real handle has, distinct per test so tests
    /// running in parallel never share a key.
    fn fake_handle(address: usize) -> sys::PicamHandle {
        std::ptr::without_provenance_mut(address)
    }

    /// Adds a sink straight to the registry, without PICam, sending each
    /// `convert`ed event down a new channel.
    fn add_sink<T: Send + 'static>(
        key: Key,
        convert: fn(&Event) -> Option<T>,
    ) -> (u64, Receiver<T>) {
        let (sender, receiver) = mpsc::channel();
        let sink: Sink = Box::new(move |event| {
            if let Some(value) = convert(event) {
                let _ = sender.send(value);
            }
        });
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        lock(&SINKS).entry(key).or_default().push((id, sink));
        (id, receiver)
    }

    fn integer(event: &Event) -> Option<i32> {
        match event {
            Event::Integer(value) => Some(*value),
            _ => None,
        }
    }

    fn rois(event: &Event) -> Option<Vec<Roi>> {
        match event {
            Event::Rois(rois) => Some(rois.clone()),
            _ => None,
        }
    }

    #[test]
    fn trampoline_sends_to_every_sink_on_its_key() {
        let handle = fake_handle(0x1001);
        let parameter = Parameter::ExposureTime.to_raw();
        let other = Parameter::ReadoutCount.to_raw();
        let key = Key::new(handle, parameter, Kind::Integer);
        let (first, first_values) = add_sink(key, integer);
        let (second, second_values) = add_sink(key, integer);
        let extrinsic_key = Key::new(handle, parameter, Kind::ExtrinsicInteger);
        let (extrinsic, extrinsic_values) = add_sink(extrinsic_key, integer);
        let other_key = Key::new(handle, other, Kind::Integer);
        let (unrelated, unrelated_values) = add_sink(other_key, integer);

        // SAFETY: a scalar trampoline has no requirements beyond being
        // called with plain values, as here
        let error = unsafe { integer_changed(handle, parameter, 7) };
        assert_eq!(error, sys::PicamError::PicamError_None);
        assert_eq!(first_values.try_recv(), Ok(7));
        assert_eq!(second_values.try_recv(), Ok(7));
        assert_eq!(extrinsic_values.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(unrelated_values.try_recv(), Err(TryRecvError::Empty));

        assert!(remove_sink(key, first));
        assert!(remove_sink(key, second));
        assert!(remove_sink(extrinsic_key, extrinsic));
        assert!(remove_sink(other_key, unrelated));
    }

    #[test]
    fn rois_trampoline_copies_the_regions() {
        let handle = fake_handle(0x1002);
        let parameter = Parameter::Rois.to_raw();
        let key = Key::new(handle, parameter, Kind::Rois);
        let (id, values) = add_sink(key, rois);

        let regions = vec![
            Roi {
                x: 0,
                width: 10,
                x_binning: 1,
                y: 5,
                height: 20,
                y_binning: 2,
            },
            Roi {
                x: 10,
                width: 4,
                x_binning: 2,
                y: 0,
                height: 8,
                y_binning: 1,
            },
        ];
        let mut raw_rois: Vec<sys::PicamRoi> = regions
            .iter()
            .map(|roi| sys::PicamRoi {
                x: roi.x,
                width: roi.width,
                x_binning: roi.x_binning,
                y: roi.y,
                height: roi.height,
                y_binning: roi.y_binning,
            })
            .collect();
        let raw = sys::PicamRois {
            roi_array: raw_rois.as_mut_ptr(),
            roi_count: raw_rois.len() as sys::piint,
        };
        // SAFETY: `raw` points to `raw_rois`, which stays alive and unchanged
        // until the call returns
        unsafe { rois_changed(handle, parameter, &raw) };
        assert_eq!(values.try_recv(), Ok(regions));

        // SAFETY: a null value is allowed
        unsafe { rois_changed(handle, parameter, std::ptr::null()) };
        assert_eq!(values.try_recv(), Ok(vec![]));

        assert!(remove_sink(key, id));
    }

    #[test]
    fn removing_a_sink_disconnects_its_receiver() {
        let handle = fake_handle(0x1003);
        let key = Key::new(handle, Parameter::ExposureTime.to_raw(), Kind::Integer);
        let (id, values) = add_sink(key, integer);

        assert!(remove_sink(key, id));
        assert_eq!(values.try_recv(), Err(TryRecvError::Disconnected));
        assert!(!lock(&SINKS).contains_key(&key));
        assert!(!remove_sink(key, id));
    }

    #[test]
    fn releasing_a_handle_disconnects_its_receivers_only() {
        let released = fake_handle(0x1004);
        let kept = fake_handle(0x1005);
        let exposure = Parameter::ExposureTime.to_raw();
        let count = Parameter::ReadoutCount.to_raw();
        let first_key = Key::new(released, exposure, Kind::Integer);
        let second_key = Key::new(released, count, Kind::ExtrinsicInteger);
        let kept_key = Key::new(kept, exposure, Kind::Integer);
        let (first, first_values) = add_sink(first_key, integer);
        let (second, second_values) = add_sink(second_key, integer);
        let (other, kept_values) = add_sink(kept_key, integer);

        // no registration was made with picam, so this calls no picam function
        release(released);
        assert_eq!(first_values.try_recv(), Err(TryRecvError::Disconnected));
        assert_eq!(second_values.try_recv(), Err(TryRecvError::Disconnected));
        assert_eq!(kept_values.try_recv(), Err(TryRecvError::Empty));
        // a watch dropped after its camera closed finds nothing to remove
        assert!(!remove_sink(first_key, first));
        assert!(!remove_sink(second_key, second));

        assert!(remove_sink(kept_key, other));
    }
}
