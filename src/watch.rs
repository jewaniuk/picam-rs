//! Watching parameters, statuses, discovery and acquisition states through
//! PICam's callbacks, and the callback behind
//! [`Camera::start_acquisition_stream`](crate::Camera::start_acquisition_stream)
//! (`picam_advanced.h` and `picam_accessory.h`, PICam manual 7.3, 7.4.1,
//! 7.4.2, 7.4.8 to 7.4.12, 7.4.15 and 7.4.16).
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
//!
//! Discovery callbacks have no handle or parameter, so they're keyed by a null
//! handle and parameter 0, and the library clears them before it
//! uninitializes. Acquisition state callbacks have no parameter, so they're
//! keyed by the device handle, parameter 0 and the state. So are acquisition
//! updates, by the device handle and parameter 0, but their sink is the
//! streaming acquisition's own: it copies the readouts while the trampoline
//! dispatches the update, and the trampoline then checks the overrun flag
//! outside `SINKS` and dispatches that too. A status notification fires once:
//! its trampoline takes the key's sinks as it sends, so a key with no sinks
//! has nothing registered with PICam, whatever `REGISTRATIONS` still counts.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::accessory::AccessoryId;
use crate::acquisition::{
    AcquisitionState, AcquisitionStateErrors, AcquisitionStateUpdate, has_overrun,
};
use crate::camera::CameraId;
use crate::constraint::{
    CollectionConstraint, ModulationsConstraint, PulseConstraint, RangeConstraint, RoisConstraint,
};
use crate::convert::collect_raw_array;
use crate::discovery::{AccessoryDiscovery, CameraDiscovery, DiscoveryAction};
use crate::error::{Error, Result, picam_call};
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
    CollectionConstraint,
    RangeConstraint,
    RoisConstraint,
    PulseConstraint,
    ModulationsConstraint,
    /// a one-shot notification for the status value it holds
    Status(i32),
    CameraDiscovery,
    AccessoryDiscovery,
    /// the acquisition state it watches for
    AcquisitionState(i32),
    AcquisitionUpdated,
}

/// A copy of what one callback reported, made before the callback returns,
/// except an acquisition update, which points at data valid only while it's
/// dispatched.
pub(crate) enum Event {
    Integer(i32),
    LargeInteger(i64),
    FloatingPoint(f64),
    Rois(Vec<Roi>),
    Pulse(Pulse),
    Modulations(Vec<Modulation>),
    IsRelevant(bool),
    ValueAccess(ValueAccess),
    CollectionConstraint(CollectionConstraint),
    RangeConstraint(RangeConstraint),
    RoisConstraint(RoisConstraint),
    PulseConstraint(PulseConstraint),
    ModulationsConstraint(ModulationsConstraint),
    Status(Result<()>),
    CameraDiscovery(CameraDiscovery),
    AccessoryDiscovery(AccessoryDiscovery),
    AcquisitionState(AcquisitionStateUpdate),
    /// What an acquisition-updated callback passed. `available` points at
    /// data valid only while this event is dispatched, inside the callback
    AcquisitionUpdated {
        available: Option<sys::PicamAvailableData>,
        status: sys::PicamAcquisitionStatus,
    },
    /// Whether the camera's buffer had overrun once the sinks copied the
    /// last acquisition update
    AcquisitionChecked(Result<bool>),
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

/// A watch on a parameter, a status, discovery or an acquisition state, which
/// ends when dropped, or when the camera, accessory or library it watches
/// closes.
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
///
/// A status notification
/// ([`Camera::notify_when_status`](crate::Camera::notify_when_status)) sends
/// one result and then disconnects. A discovery watch
/// ([`Camera::watch_discovery`](crate::Camera::watch_discovery)) belongs to
/// the library, which clears it before it uninitializes. An acquisition state
/// watch
/// ([`Camera::watch_acquisition_state`](crate::Camera::watch_acquisition_state))
/// is on the camera's device, and ends when the camera closes. An
/// [`AcquisitionStream`](crate::AcquisitionStream) keeps one for its callback.
pub struct Watch {
    key: Key,
    id: u64,
    handle: sys::PicamHandle,
}

impl Watch {
    /// Adds `sink` for `kind` callbacks on `handle`'s `parameter`, and
    /// registers PICam's callback if the key has nothing registered.
    fn new(
        handle: sys::PicamHandle,
        parameter: sys::PicamParameter,
        kind: Kind,
        sink: Sink,
    ) -> Result<Self> {
        let key = Key::new(handle, parameter, kind);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut registrations = lock(&REGISTRATIONS);
        // the sink goes in first, so no event between picam registering the
        // callback and this returning is lost. a key with no sinks has
        // nothing registered with picam, even if counted: a status
        // notification that fired took them all
        let live = {
            let mut sinks = lock(&SINKS);
            let list = sinks.entry(key).or_default();
            let live = !list.is_empty();
            list.push((id, sink));
            live
        };
        let count = if live {
            registrations.get(&key).copied().unwrap_or(0)
        } else {
            0
        };
        if count == 0
            && let Err(e) = register(handle, parameter, kind)
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
        // a status notification that fired has no sinks left, and nothing
        // registered with picam to cancel
        if !lock(&SINKS).contains_key(&key) {
            continue;
        }
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
    subscribe_raw(handle, parameter.to_raw(), kind, convert)
}

/// Watches discovery through `kind` callbacks, which have no handle or
/// parameter, so they're keyed by a null handle and parameter 0.
/// [`crate::Library`] clears them before it uninitializes.
///
/// # Errors
///
/// Returns an error if PICam is unable to register the callback.
pub(crate) fn subscribe_discovery<T: Send + 'static>(
    kind: Kind,
    convert: fn(&Event) -> Option<T>,
) -> Result<(Watch, Receiver<T>)> {
    subscribe_raw(std::ptr::null_mut(), sys::PicamParameter(0), kind, convert)
}

/// Watches `device` for `state` through acquisition state callbacks, which
/// have no parameter, so they're keyed by parameter 0.
///
/// # Errors
///
/// Returns an error if PICam is unable to register the callback.
pub(crate) fn subscribe_acquisition_state<T: Send + 'static>(
    device: sys::PicamHandle,
    state: AcquisitionState,
    convert: fn(&Event) -> Option<T>,
) -> Result<(Watch, Receiver<T>)> {
    subscribe_raw(
        device,
        sys::PicamParameter(0),
        Kind::AcquisitionState(state.to_raw().0),
        convert,
    )
}

/// Adds `sink` for `device`'s acquisition-updated callbacks, which have no
/// parameter, so they're keyed by parameter 0. A streaming acquisition
/// handles the events itself, rather than through a channel of copies.
///
/// # Errors
///
/// Returns an error if PICam is unable to register the callback.
pub(crate) fn subscribe_acquisition_updated(
    device: sys::PicamHandle,
    sink: impl Fn(&Event) + Send + 'static,
) -> Result<Watch> {
    Watch::new(
        device,
        sys::PicamParameter(0),
        Kind::AcquisitionUpdated,
        Box::new(sink),
    )
}

/// Reports whether `device` has an acquisition-updated callback registered.
pub(crate) fn is_acquisition_updated_registered(device: sys::PicamHandle) -> bool {
    lock(&REGISTRATIONS).contains_key(&Key::new(
        device,
        sys::PicamParameter(0),
        Kind::AcquisitionUpdated,
    ))
}

/// `subscribe`'s body, taking the raw parameter.
fn subscribe_raw<T: Send + 'static>(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
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

/// Sends `event` to every sink on `handle`'s `parameter` for `kind`, then
/// drops them, for a callback PICam calls only once. Always reports success
/// to PICam.
fn dispatch_once(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    kind: Kind,
    event: &Event,
) -> sys::PicamError {
    let list = lock(&SINKS).remove(&Key::new(handle, parameter, kind));
    for (_, sink) in list.into_iter().flatten() {
        sink(event);
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
                Kind::Status(value) => picam_call!(
                    sys::PicamAdvanced_NotifyWhenStatusParameterValue(
                        handle, parameter, value, Some(status_reached)
                    )
                ),
                Kind::CameraDiscovery => {
                    picam_call!(sys::PicamAdvanced_RegisterForDiscovery(Some(camera_discovered)))
                }
                Kind::AccessoryDiscovery => {
                    picam_call!(sys::PicamAccessory_RegisterForDiscovery(Some(accessory_discovered)))
                }
                Kind::AcquisitionState(state) => picam_call!(
                    sys::PicamAdvanced_RegisterForAcquisitionStateUpdated(
                        handle,
                        sys::PicamAcquisitionState(state),
                        Some(acquisition_state_updated)
                    )
                ),
                Kind::AcquisitionUpdated => picam_call!(
                    sys::PicamAdvanced_RegisterForAcquisitionUpdated(
                        handle,
                        Some(acquisition_updated)
                    )
                ),
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
                Kind::Status(value) => picam_call!(
                    sys::PicamAdvanced_CancelNotifyWhenStatusParameterValue(
                        handle, parameter, value, Some(status_reached)
                    )
                ),
                Kind::CameraDiscovery => {
                    picam_call!(sys::PicamAdvanced_UnregisterForDiscovery(Some(camera_discovered)))
                }
                Kind::AccessoryDiscovery => {
                    picam_call!(sys::PicamAccessory_UnregisterForDiscovery(Some(accessory_discovered)))
                }
                Kind::AcquisitionState(state) => picam_call!(
                    sys::PicamAdvanced_UnregisterForAcquisitionStateUpdated(
                        handle,
                        sys::PicamAcquisitionState(state),
                        Some(acquisition_state_updated)
                    )
                ),
                Kind::AcquisitionUpdated => picam_call!(
                    sys::PicamAdvanced_UnregisterForAcquisitionUpdated(
                        handle,
                        Some(acquisition_updated)
                    )
                ),
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
    CollectionConstraint => PicamAdvanced_RegisterForDependentCollectionConstraintChanged,
        PicamAdvanced_UnregisterForDependentCollectionConstraintChanged,
        collection_constraint_changed;
    RangeConstraint => PicamAdvanced_RegisterForDependentRangeConstraintChanged,
        PicamAdvanced_UnregisterForDependentRangeConstraintChanged, range_constraint_changed;
    RoisConstraint => PicamAdvanced_RegisterForDependentRoisConstraintChanged,
        PicamAdvanced_UnregisterForDependentRoisConstraintChanged, rois_constraint_changed;
    PulseConstraint => PicamAdvanced_RegisterForDependentPulseConstraintChanged,
        PicamAdvanced_UnregisterForDependentPulseConstraintChanged, pulse_constraint_changed;
    ModulationsConstraint => PicamAdvanced_RegisterForDependentModulationsConstraintChanged,
        PicamAdvanced_UnregisterForDependentModulationsConstraintChanged,
        modulations_constraint_changed;
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

/// PICam's dependent collection constraint callback (manual 7.3.6.1).
///
/// # Safety
///
/// `constraint` must be null or point to a valid `PicamCollectionConstraint`,
/// whose array is valid too, for the duration of the call.
unsafe extern "C" fn collection_constraint_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    constraint: *const sys::PicamCollectionConstraint,
) -> sys::PicamError {
    if constraint.is_null() {
        tracing::warn!("PICam sent a collection constraint callback with no constraint");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `constraint` is non-null, and the caller
    // guarantees it and its array are valid for this call, copied before it
    // returns
    let constraint = CollectionConstraint::from_raw(unsafe { &*constraint });
    dispatch(
        handle,
        parameter,
        Kind::CollectionConstraint,
        &Event::CollectionConstraint(constraint),
    )
}

/// PICam's dependent range constraint callback (manual 7.3.6.2).
///
/// # Safety
///
/// `constraint` must be null or point to a valid `PicamRangeConstraint`,
/// whose arrays are valid too, for the duration of the call.
unsafe extern "C" fn range_constraint_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    constraint: *const sys::PicamRangeConstraint,
) -> sys::PicamError {
    if constraint.is_null() {
        tracing::warn!("PICam sent a range constraint callback with no constraint");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `constraint` is non-null, and the caller
    // guarantees it and its arrays are valid for this call, copied before it
    // returns
    let constraint = RangeConstraint::from_raw(unsafe { &*constraint });
    dispatch(
        handle,
        parameter,
        Kind::RangeConstraint,
        &Event::RangeConstraint(constraint),
    )
}

/// PICam's dependent ROIs constraint callback (manual 7.3.5.1).
///
/// # Safety
///
/// `constraint` must be null or point to a valid `PicamRoisConstraint`, whose
/// arrays are valid too, for the duration of the call.
unsafe extern "C" fn rois_constraint_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    constraint: *const sys::PicamRoisConstraint,
) -> sys::PicamError {
    if constraint.is_null() {
        tracing::warn!("PICam sent a ROIs constraint callback with no constraint");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `constraint` is non-null, and the caller
    // guarantees it and its arrays are valid for this call, copied before it
    // returns
    let constraint = RoisConstraint::from_raw(unsafe { &*constraint });
    dispatch(
        handle,
        parameter,
        Kind::RoisConstraint,
        &Event::RoisConstraint(constraint),
    )
}

/// PICam's dependent pulse constraint callback (manual 7.3.5.2).
///
/// # Safety
///
/// `constraint` must be null or point to a valid `PicamPulseConstraint` for
/// the duration of the call.
unsafe extern "C" fn pulse_constraint_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    constraint: *const sys::PicamPulseConstraint,
) -> sys::PicamError {
    if constraint.is_null() {
        tracing::warn!("PICam sent a pulse constraint callback with no constraint");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `constraint` is non-null, and the caller
    // guarantees it's valid for this call, copied before it returns
    let constraint = PulseConstraint::from_raw(unsafe { &*constraint });
    dispatch(
        handle,
        parameter,
        Kind::PulseConstraint,
        &Event::PulseConstraint(constraint),
    )
}

/// PICam's dependent modulations constraint callback (manual 7.3.5.3).
///
/// # Safety
///
/// `constraint` must be null or point to a valid `PicamModulationsConstraint`,
/// whose arrays are valid too, for the duration of the call.
unsafe extern "C" fn modulations_constraint_changed(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    constraint: *const sys::PicamModulationsConstraint,
) -> sys::PicamError {
    if constraint.is_null() {
        tracing::warn!("PICam sent a modulations constraint callback with no constraint");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `constraint` is non-null, and the caller
    // guarantees it and its arrays are valid for this call, copied before it
    // returns
    let constraint = ModulationsConstraint::from_raw(unsafe { &*constraint });
    dispatch(
        handle,
        parameter,
        Kind::ModulationsConstraint,
        &Event::ModulationsConstraint(constraint),
    )
}

/// PICam's status notification callback (manual 7.3.4.3), which PICam calls
/// once per notification, when the value is reached or an error stops it.
///
/// # Safety
///
/// None: it reads no pointer. It's `unsafe` only to match
/// `PicamWhenStatusParameterValueCallback`, the type bindgen gives PICam's
/// callback.
unsafe extern "C" fn status_reached(
    handle: sys::PicamHandle,
    parameter: sys::PicamParameter,
    value: sys::piint,
    error: sys::PicamError,
) -> sys::PicamError {
    let result = if error == sys::PicamError::PicamError_None {
        Ok(())
    } else {
        Err(Error::from_raw(error))
    };
    dispatch_once(
        handle,
        parameter,
        Kind::Status(value),
        &Event::Status(result),
    )
}

/// PICam's camera discovery callback (manual 7.3.1.1).
///
/// # Safety
///
/// `id` must be null or point to a valid `PicamCameraID` for the duration of
/// the call.
unsafe extern "C" fn camera_discovered(
    id: *const sys::PicamCameraID,
    device: sys::PicamHandle,
    action: sys::PicamDiscoveryAction,
) -> sys::PicamError {
    if id.is_null() {
        tracing::warn!("PICam sent a camera discovery callback with no camera");
        return sys::PicamError::PicamError_None;
    }
    let discovery = CameraDiscovery {
        // SAFETY: just checked `id` is non-null, and the caller guarantees it
        // points to a valid `PicamCameraID` for this call, copied before it
        // returns
        id: CameraId::from_raw(unsafe { &*id }),
        action: DiscoveryAction::from_raw(action),
        // picam passes the device handle only when the camera is open in
        // this process (manual 7.3.1.1)
        open_in_this_process: !device.is_null(),
    };
    dispatch(
        std::ptr::null_mut(),
        sys::PicamParameter(0),
        Kind::CameraDiscovery,
        &Event::CameraDiscovery(discovery),
    )
}

/// PICam's accessory discovery callback (manual 7.3.2.1).
///
/// # Safety
///
/// `id` must be null or point to a valid `PicamAccessoryID` for the duration
/// of the call.
unsafe extern "C" fn accessory_discovered(
    id: *const sys::PicamAccessoryID,
    accessory: sys::PicamHandle,
    action: sys::PicamDiscoveryAction,
) -> sys::PicamError {
    if id.is_null() {
        tracing::warn!("PICam sent an accessory discovery callback with no accessory");
        return sys::PicamError::PicamError_None;
    }
    let discovery = AccessoryDiscovery {
        // SAFETY: just checked `id` is non-null, and the caller guarantees it
        // points to a valid `PicamAccessoryID` for this call, copied before
        // it returns
        id: AccessoryId::from_raw(unsafe { &*id }),
        action: DiscoveryAction::from_raw(action),
        // picam passes the accessory handle only when the accessory is open
        // in this process (manual 7.3.2.1)
        open_in_this_process: !accessory.is_null(),
    };
    dispatch(
        std::ptr::null_mut(),
        sys::PicamParameter(0),
        Kind::AccessoryDiscovery,
        &Event::AccessoryDiscovery(discovery),
    )
}

/// PICam's acquisition state callback (manual 7.3.7.2).
///
/// # Safety
///
/// `counters` must be null or point to a valid
/// `PicamAcquisitionStateCounters` for the duration of the call.
unsafe extern "C" fn acquisition_state_updated(
    device: sys::PicamHandle,
    current: sys::PicamAcquisitionState,
    counters: *const sys::PicamAcquisitionStateCounters,
    errors: sys::PicamAcquisitionStateErrorsMask,
) -> sys::PicamError {
    if counters.is_null() {
        tracing::warn!("PICam sent an acquisition state callback with no counters");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `counters` is non-null, and the caller guarantees
    // it points to a valid `PicamAcquisitionStateCounters` for this call,
    // copied before it returns
    let counters = unsafe { *counters };
    let update = AcquisitionStateUpdate {
        state: AcquisitionState::from_raw(current),
        readout_started_count: counters.readout_started_count,
        readout_ended_count: counters.readout_ended_count,
        errors: AcquisitionStateErrors::from_bits_retain(errors.0),
    };
    dispatch(
        device,
        sys::PicamParameter(0),
        Kind::AcquisitionState(current.0),
        &Event::AcquisitionState(update),
    )
}

/// PICam's acquisition-updated callback (manual 7.3.7.1).
///
/// # Safety
///
/// `available` must be null or point to a valid `PicamAvailableData`, whose
/// readouts stay valid for the duration of the call, and `status` must be
/// null or point to a valid `PicamAcquisitionStatus`.
unsafe extern "C" fn acquisition_updated(
    device: sys::PicamHandle,
    available: *const sys::PicamAvailableData,
    status: *const sys::PicamAcquisitionStatus,
) -> sys::PicamError {
    if status.is_null() {
        tracing::warn!("PICam sent an acquisition update with no status");
        return sys::PicamError::PicamError_None;
    }
    // SAFETY: just checked `status` is non-null, and the caller guarantees it
    // points to a valid `PicamAcquisitionStatus` for this call
    let status = unsafe { *status };
    // SAFETY: the caller guarantees `available` is null or valid for this call
    let available = unsafe { available.as_ref() }.copied();
    let parameter = sys::PicamParameter(0);
    let kind = Kind::AcquisitionUpdated;
    dispatch(
        device,
        parameter,
        kind,
        &Event::AcquisitionUpdated { available, status },
    );
    if available.is_some() {
        // the sinks have copied the readouts. one picam wrote over midway
        // shows in the overrun flag, read here since the sink lock is never
        // held across a picam call. picam answers it inside its callback
        // (seen with sdk 5.15)
        let overran = has_overrun(device);
        dispatch(device, parameter, kind, &Event::AcquisitionChecked(overran));
    }
    sys::PicamError::PicamError_None
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

    fn status(event: &Event) -> Option<Result<()>> {
        match event {
            Event::Status(result) => Some(*result),
            _ => None,
        }
    }

    fn range(event: &Event) -> Option<RangeConstraint> {
        match event {
            Event::RangeConstraint(constraint) => Some(constraint.clone()),
            _ => None,
        }
    }

    fn camera_discovery(event: &Event) -> Option<CameraDiscovery> {
        match event {
            Event::CameraDiscovery(discovery) => Some(discovery.clone()),
            _ => None,
        }
    }

    #[test]
    fn status_trampoline_sends_once_then_disconnects() {
        let handle = fake_handle(0x1006);
        let parameter = Parameter::SensorTemperatureStatus.to_raw();
        let key = Key::new(handle, parameter, Kind::Status(2));
        let (_, first) = add_sink(key, status);
        let (_, second) = add_sink(key, status);

        // SAFETY: a scalar trampoline has no requirements beyond being
        // called with plain values, as here
        let error =
            unsafe { status_reached(handle, parameter, 2, sys::PicamError::PicamError_None) };
        assert_eq!(error, sys::PicamError::PicamError_None);
        assert_eq!(first.try_recv(), Ok(Ok(())));
        assert_eq!(second.try_recv(), Ok(Ok(())));
        assert_eq!(first.try_recv(), Err(TryRecvError::Disconnected));
        assert_eq!(second.try_recv(), Err(TryRecvError::Disconnected));
        assert!(!lock(&SINKS).contains_key(&key));
        // SAFETY: as above
        unsafe { status_reached(handle, parameter, 2, sys::PicamError::PicamError_None) };

        let failing_key = Key::new(handle, parameter, Kind::Status(1));
        let (_, failing) = add_sink(failing_key, status);
        // SAFETY: as above
        unsafe {
            status_reached(
                handle,
                parameter,
                1,
                sys::PicamError::PicamError_TimeOutOccurred,
            )
        };
        assert_eq!(failing.try_recv(), Ok(Err(Error::TimeOutOccurred)));
        assert_eq!(failing.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn range_constraint_trampoline_copies_the_constraint() {
        let handle = fake_handle(0x1007);
        let parameter = Parameter::ExposureTime.to_raw();
        let key = Key::new(handle, parameter, Kind::RangeConstraint);
        let (id, constraints) = add_sink(key, range);

        let excluded = [5.0];
        let raw = sys::PicamRangeConstraint {
            scope: sys::PicamConstraintScope::PicamConstraintScope_Dependent,
            severity: sys::PicamConstraintSeverity::PicamConstraintSeverity_Error,
            empty_set: 0,
            minimum: 1.0,
            maximum: 10.0,
            increment: 1.0,
            excluded_values_array: excluded.as_ptr(),
            excluded_values_count: 1,
            outlying_values_array: std::ptr::null(),
            outlying_values_count: 0,
        };
        // SAFETY: `raw` points to `excluded`, and both stay alive and
        // unchanged until the call returns
        unsafe { range_constraint_changed(handle, parameter, &raw) };
        let sent = constraints
            .try_recv()
            .expect("the constraint should be sent");
        assert_eq!(
            (sent.minimum, sent.maximum, sent.increment),
            (1.0, 10.0, 1.0)
        );
        assert_eq!(sent.excluded_values, vec![5.0]);
        assert!(sent.outlying_values.is_empty());
        assert!(!sent.empty_set);

        // SAFETY: a null constraint is allowed
        unsafe { range_constraint_changed(handle, parameter, std::ptr::null()) };
        assert_eq!(constraints.try_recv(), Err(TryRecvError::Empty));

        assert!(remove_sink(key, id));
    }

    #[test]
    fn camera_discovery_trampoline_copies_the_id() {
        let key = Key::new(
            std::ptr::null_mut(),
            sys::PicamParameter(0),
            Kind::CameraDiscovery,
        );
        let (id, events) = add_sink(key, camera_discovery);
        let camera = CameraId {
            model: crate::Model::PIXIS_100F,
            computer_interface: crate::ComputerInterface::Usb2,
            sensor_name: "sensor".to_string(),
            serial_number: "DISCOVERY1".to_string(),
        };
        let raw = camera.to_raw().expect("the id should convert");

        // SAFETY: `raw` stays alive and unchanged until each call returns
        unsafe {
            camera_discovered(
                &raw,
                fake_handle(0x1008),
                sys::PicamDiscoveryAction::PicamDiscoveryAction_Found,
            )
        };
        let found = events.try_recv().expect("the event should be sent");
        assert_eq!(found.id.serial_number, "DISCOVERY1");
        assert_eq!(found.id.sensor_name, "sensor");
        assert_eq!(found.action, DiscoveryAction::Found);
        assert!(found.open_in_this_process);
        // SAFETY: as above
        unsafe {
            camera_discovered(
                &raw,
                std::ptr::null_mut(),
                sys::PicamDiscoveryAction::PicamDiscoveryAction_Lost,
            )
        };
        let lost = events.try_recv().expect("the event should be sent");
        assert_eq!(lost.action, DiscoveryAction::Lost);
        assert!(!lost.open_in_this_process);

        // releasing the null handle, as the library does, clears discovery
        // watches only. it's checked here because the discovery key is
        // global, so a separate test running in parallel could clear this
        // one's sink
        let kept_key = Key::new(
            fake_handle(0x1009),
            Parameter::ExposureTime.to_raw(),
            Kind::Integer,
        );
        let (kept, kept_values) = add_sink(kept_key, integer);
        // no registration was made with picam, so this calls no picam function
        release(std::ptr::null_mut());
        assert_eq!(events.try_recv().err(), Some(TryRecvError::Disconnected));
        assert_eq!(kept_values.try_recv(), Err(TryRecvError::Empty));
        assert!(!remove_sink(key, id));

        assert!(remove_sink(kept_key, kept));
    }

    fn acquisition_state(event: &Event) -> Option<AcquisitionStateUpdate> {
        match event {
            Event::AcquisitionState(update) => Some(*update),
            _ => None,
        }
    }

    #[test]
    fn acquisition_state_trampoline_copies_the_counters() {
        let ended = sys::PicamAcquisitionState::PicamAcquisitionState_ReadoutEnded;
        let key = Key::new(
            fake_handle(0x100A),
            sys::PicamParameter(0),
            Kind::AcquisitionState(ended.0),
        );
        let (id, updates) = add_sink(key, acquisition_state);
        let counters = sys::PicamAcquisitionStateCounters {
            readout_started_count: 3,
            readout_ended_count: 2,
        };

        // SAFETY: `counters` stays alive and unchanged until the call returns
        unsafe {
            acquisition_state_updated(
                fake_handle(0x100A),
                ended,
                &counters,
                sys::PicamAcquisitionStateErrorsMask::PicamAcquisitionStateErrorsMask_LostCount,
            )
        };
        assert_eq!(
            updates.try_recv(),
            Ok(AcquisitionStateUpdate {
                state: AcquisitionState::ReadoutEnded,
                readout_started_count: 3,
                readout_ended_count: 2,
                errors: AcquisitionStateErrors::LOST_COUNT,
            })
        );
        // SAFETY: a null pointer is allowed, and nothing is read through it
        unsafe {
            acquisition_state_updated(
                fake_handle(0x100A),
                ended,
                std::ptr::null(),
                sys::PicamAcquisitionStateErrorsMask::PicamAcquisitionStateErrorsMask_None,
            )
        };
        assert_eq!(updates.try_recv(), Err(TryRecvError::Empty));

        assert!(remove_sink(key, id));
    }
}
