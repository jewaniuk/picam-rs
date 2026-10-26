//! The parameter methods cameras, camera devices and accessories share.

/// Implements the parameter methods cameras and accessories share on `$ty`,
/// whose `handle` field is its PICam handle: the calls `picam_accessory.h`
/// lists for accessories, which cameras support too. `$noun` names the type
/// in the docs. `$ty` must also have a private
/// `extrinsic_handle(&self) -> Result<sys::PicamHandle>`, the handle PICam
/// registers extrinsic value callbacks on: a device or accessory handle
/// (PICam manual 7.4.9.3).
macro_rules! impl_parameter_methods {
    ($ty:ty, $noun:literal) => {
        // the method bodies name these directly, so they're imported here
        // rather than at each invocation
        const _: () = {
            use std::sync::mpsc::Receiver;

            use $crate::constraint::{
                CollectionConstraint, ConstraintCategory, ConstraintType, RangeConstraint,
            };
            use $crate::convert::collect_raw_array;
            use $crate::error::{Error, Result, picam_call};
            use $crate::parameter::sealed::RawHandle;
            use $crate::parameter::{
                Dynamics, EnumeratedType, Parameter, PicamValue, ReadablePicamValue, TypedParameter,
                ValueAccess, ValueType, can_read, can_set_online, can_wait_for_status,
                constraint_type, enumerated_type, is_relevant, value_access, value_type,
            };
            use $crate::sys;
            use $crate::watch::{self, Event, Kind, Watch};

            impl $ty {
                /// Gets the current value of `parameter`, a handle from
                /// [`params`](crate::params) that fixes the value's type.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterDoesNotExist`] if
                #[doc = concat!(" `parameter` doesn't apply to this ", $noun, ", or an error if")]
                /// PICam is unable to read it.
                pub fn get<T: PicamValue>(&self, parameter: TypedParameter<T>) -> Result<T> {
                    T::get(RawHandle(self.handle), parameter.parameter())
                }

                /// Gets the current value of `parameter` as a `T` the caller chooses.
                ///
                /// Prefer [`Self::get`], which takes the type from the parameter. This
                /// is for parameters with no handle in [`params`](crate::params), such as
                /// [`Parameter::Unknown`] values from [`Self::parameters`].
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidValueType`] if PICam's call
                /// for `T` doesn't accept `parameter`'s value type (`i32` and `bool` share
                /// PICam's integer calls, so either is accepted for integer, on/off and
                /// enum parameters) or `T` is an enum and `parameter` isn't an enum
                /// parameter, [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum
                /// other than the one `parameter`'s values belong to,
                /// [`crate::Error::ParameterDoesNotExist`] if `parameter` doesn't apply to
                #[doc = concat!(" this ", $noun, ", or an error if PICam is unable to read it.")]
                pub fn get_untyped<T: PicamValue>(&self, parameter: Parameter) -> Result<T> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::get(RawHandle(self.handle), parameter)
                }

                /// Sets `parameter`, a handle from [`params`](crate::params), to `value`.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterValueIsReadOnly`] if
                /// `parameter` can't be set, [`crate::Error::InvalidParameterValue`]
                /// if `value` is out of range, or an error if PICam is unable to set it.
                pub fn set<T: PicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                    value: T,
                ) -> Result<()> {
                    T::set(RawHandle(self.handle), parameter.parameter(), value)
                }

                /// Sets `parameter` to a `value` of a type the caller chooses.
                ///
                /// Prefer [`Self::set`]. This is for parameters with no handle in
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidValueType`] if PICam's call
                /// for `T` doesn't accept `parameter`'s value type (`i32` and `bool` share
                /// PICam's integer calls, so either is accepted for integer, on/off and
                /// enum parameters) or `T` is an enum and `parameter` isn't an enum
                /// parameter, [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum
                /// other than the one `parameter`'s values belong to,
                /// [`crate::Error::ParameterValueIsReadOnly`] if `parameter` can't be set,
                /// [`crate::Error::InvalidParameterValue`] if `value` is out of range, or
                /// an error if PICam is unable to set it.
                pub fn set_untyped<T: PicamValue>(
                    &self,
                    parameter: Parameter,
                    value: T,
                ) -> Result<()> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::set(RawHandle(self.handle), parameter, value)
                }

                /// Gets the factory default value of `parameter`, a handle from
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to read the default value.
                pub fn default_value<T: PicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                ) -> Result<T> {
                    T::default_value(RawHandle(self.handle), parameter.parameter())
                }

                /// Gets the factory default value of `parameter` as a `T` the caller
                /// chooses.
                ///
                /// Prefer [`Self::default_value`]. This is for parameters with no handle
                /// in [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidValueType`] if PICam's call
                /// for `T` doesn't accept `parameter`'s value type (`i32` and `bool` share
                /// PICam's integer calls, so either is accepted for integer, on/off and
                /// enum parameters) or `T` is an enum and `parameter` isn't an enum
                /// parameter, [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum
                /// other than the one `parameter`'s values belong to, or an error if PICam
                /// is unable to read the default value.
                pub fn default_value_untyped<T: PicamValue>(
                    &self,
                    parameter: Parameter,
                ) -> Result<T> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::default_value(RawHandle(self.handle), parameter)
                }

                /// Reports whether `value` could currently be set for `parameter`, a
                /// handle from [`params`](crate::params), without actually setting it.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn can_set<T: PicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                    value: T,
                ) -> Result<bool> {
                    T::can_set(RawHandle(self.handle), parameter.parameter(), value)
                }

                /// Reports whether a `value` of a type the caller chooses could currently
                /// be set for `parameter`, without actually setting it.
                ///
                /// Prefer [`Self::can_set`]. This is for parameters with no handle in
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidValueType`] if PICam's call
                /// for `T` doesn't accept `parameter`'s value type (`i32` and `bool` share
                /// PICam's integer calls, so either is accepted for integer, on/off and
                /// enum parameters) or `T` is an enum and `parameter` isn't an enum
                /// parameter, [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum
                /// other than the one `parameter`'s values belong to, or an error if PICam
                /// is unable to check this.
                pub fn can_set_untyped<T: PicamValue>(
                    &self,
                    parameter: Parameter,
                    value: T,
                ) -> Result<bool> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::can_set(RawHandle(self.handle), parameter, value)
                }

                /// Reports whether `parameter` can be set online (without committing).
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn can_set_online(&self, parameter: impl Into<Parameter>) -> Result<bool> {
                    can_set_online(self.handle, parameter.into())
                }

                /// Reports whether `parameter` can be read directly from hardware
                /// via [`Self::read`].
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn can_read(&self, parameter: impl Into<Parameter>) -> Result<bool> {
                    can_read(self.handle, parameter.into())
                }

                /// Reports whether `parameter` is a waitable status parameter.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn can_wait_for_status(&self, parameter: impl Into<Parameter>) -> Result<bool> {
                    can_wait_for_status(self.handle, parameter.into())
                }

                /// Reports whether `parameter` is currently relevant to this
                #[doc = concat!(" ", $noun, "'s configuration.")]
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn is_relevant(&self, parameter: impl Into<Parameter>) -> Result<bool> {
                    is_relevant(self.handle, parameter.into())
                }

                /// Gets the underlying value type of `parameter`.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn value_type(&self, parameter: impl Into<Parameter>) -> Result<ValueType> {
                    value_type(self.handle, parameter.into()).map(ValueType::from_raw)
                }

                /// Gets the enumerated type `parameter`'s value belongs to.
                ///
                /// Only parameters whose value type is [`ValueType::Enumeration`] are
                /// valid here (PICam manual 5.4.3.6).
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn enumerated_type(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<EnumeratedType> {
                    enumerated_type(self.handle, parameter.into()).map(EnumeratedType::from_raw)
                }

                /// Gets whether/how `parameter`'s value can currently be accessed.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn value_access(&self, parameter: impl Into<Parameter>) -> Result<ValueAccess> {
                    value_access(self.handle, parameter.into()).map(ValueAccess::from_raw)
                }

                /// Gets the kind of constraint that applies to `parameter`'s value.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn constraint_type(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<ConstraintType> {
                    constraint_type(self.handle, parameter.into()).map(ConstraintType::from_raw)
                }

                #[doc = concat!(" Lists the parameters relevant to this ", $noun, ".")]
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

                    let parameters =
                        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
                        unsafe { collect_raw_array(array, count, |&raw| Parameter::from_raw(raw)) };

                    if !array.is_null() {
                        picam_call!(sys::Picam_DestroyParameters(array))?;
                    }
                    Ok(parameters)
                }

                #[doc = concat!(" Reports whether `parameter` applies to this ", $noun, ".")]
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to check this.
                pub fn does_parameter_exist(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<bool> {
                    let mut exists: sys::pibln = 0;
                    picam_call!(sys::Picam_DoesParameterExist(
                        self.handle,
                        parameter.into().to_raw(),
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

                /// Commits all pending parameter changes made via [`Self::set`],
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

                    let failed =
                        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
                        unsafe { collect_raw_array(array, count, |&raw| Parameter::from_raw(raw)) };

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
                pub fn status_parameter_purview(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<Vec<i32>> {
                    let mut ptr: *const sys::PicamStatusPurview = std::ptr::null();
                    picam_call!(sys::Picam_GetStatusParameterPurview(
                        self.handle,
                        parameter.into().to_raw(),
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
                    parameter: impl Into<Parameter>,
                    value: i32,
                ) -> Result<i32> {
                    let mut estimated_time: sys::piint = 0;
                    picam_call!(sys::Picam_EstimateTimeToStatusParameterValue(
                        self.handle,
                        parameter.into().to_raw(),
                        value,
                        &mut estimated_time
                    ))?;
                    Ok(estimated_time)
                }

                /// Blocks until `parameter` reaches `value`, or `timeout` milliseconds
                /// have elapsed.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::TimeOutOccurred`] if `timeout`
                /// elapses first, or an error if PICam is unable to wait for this.
                pub fn wait_for_status_parameter_value(
                    &self,
                    parameter: impl Into<Parameter>,
                    value: i32,
                    timeout: i32,
                ) -> Result<()> {
                    picam_call!(sys::Picam_WaitForStatusParameterValue(
                        self.handle,
                        parameter.into().to_raw(),
                        value,
                        timeout
                    ))
                }

                /// Reads the current value of `parameter`, a handle from
                /// [`params`](crate::params), directly from hardware, bypassing PICam's
                /// cached/committed value.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to read it.
                pub fn read<T: ReadablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                ) -> Result<T> {
                    T::read(RawHandle(self.handle), parameter.parameter())
                }

                /// Reads the current value of `parameter` directly from hardware as a `T`
                /// the caller chooses.
                ///
                /// Prefer [`Self::read`]. This is for parameters with no handle in
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidValueType`] if PICam's call
                /// for `T` doesn't accept `parameter`'s value type (`i32` and `bool` share
                /// PICam's integer calls, so either is accepted for integer, on/off and
                /// enum parameters) or `T` is an enum and `parameter` isn't an enum
                /// parameter, [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum
                /// other than the one `parameter`'s values belong to, or an error if PICam
                /// is unable to read it.
                pub fn read_untyped<T: ReadablePicamValue>(
                    &self,
                    parameter: Parameter,
                ) -> Result<T> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::read(RawHandle(self.handle), parameter)
                }

                /// Gets the collection constraint on `parameter`'s value, if any.
                ///
                /// `category` selects which kind of constraint to query: the values
                /// PICam is capable of accepting, the values currently required, or
                /// the values currently recommended.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidConstraintType`]
                /// if `parameter` isn't collection-constrained, or an error if PICam
                /// is unable to query this.
                pub fn collection_constraint(
                    &self,
                    parameter: impl Into<Parameter>,
                    category: ConstraintCategory,
                ) -> Result<CollectionConstraint> {
                    let mut ptr: *const sys::PicamCollectionConstraint = std::ptr::null();
                    picam_call!(sys::Picam_GetParameterCollectionConstraint(
                        self.handle,
                        parameter.into().to_raw(),
                        category.to_raw(),
                        &mut ptr
                    ))?;

                    if ptr.is_null() {
                        return Err(Error::UnexpectedNullPointer);
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
                /// Returns [`crate::Error::ParameterHasInvalidConstraintType`]
                /// if `parameter` isn't range-constrained, or an error if PICam
                /// is unable to query this.
                pub fn range_constraint(
                    &self,
                    parameter: impl Into<Parameter>,
                    category: ConstraintCategory,
                ) -> Result<RangeConstraint> {
                    let mut ptr: *const sys::PicamRangeConstraint = std::ptr::null();
                    picam_call!(sys::Picam_GetParameterRangeConstraint(
                        self.handle,
                        parameter.into().to_raw(),
                        category.to_raw(),
                        &mut ptr
                    ))?;

                    if ptr.is_null() {
                        return Err(Error::UnexpectedNullPointer);
                    }

                    // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
                    // `PicamRangeConstraint`.
                    let constraint = RangeConstraint::from_raw(unsafe { &*ptr });
                    picam_call!(sys::Picam_DestroyRangeConstraints(ptr))?;
                    Ok(constraint)
                }

                /// Reports which of `parameter`'s attributes may change (PICam
                /// manual 7.4.10.5). [`Self::extrinsic_dynamics`] reports the
                /// attributes that may change because of something outside the
                /// software.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterDoesNotExist`] if `parameter`
                #[doc = concat!(" doesn't apply to this ", $noun, ", or an error if PICam is unable to")]
                /// query this.
                pub fn dynamics(&self, parameter: impl Into<Parameter>) -> Result<Dynamics> {
                    let mut dynamics = sys::PicamDynamicsMask(0);
                    picam_call!(sys::PicamAdvanced_GetParameterDynamics(
                        self.handle,
                        parameter.into().to_raw(),
                        &mut dynamics
                    ))?;
                    Ok(Dynamics::from_bits_retain(dynamics.0))
                }

                /// Reports which of `parameter`'s attributes may change because of
                /// something outside the software, such as the state of the
                /// hardware (PICam manual 7.4.10.6).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterDoesNotExist`] if `parameter`
                #[doc = concat!(" doesn't apply to this ", $noun, ", or an error if PICam is unable to")]
                /// query this.
                pub fn extrinsic_dynamics(&self, parameter: impl Into<Parameter>) -> Result<Dynamics> {
                    let mut dynamics = sys::PicamDynamicsMask(0);
                    picam_call!(sys::PicamAdvanced_GetParameterExtrinsicDynamics(
                        self.handle,
                        parameter.into().to_raw(),
                        &mut dynamics
                    ))?;
                    Ok(Dynamics::from_bits_retain(dynamics.0))
                }

                /// Gets every collection constraint on `parameter`'s value
                /// (PICam manual 7.4.12.1).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidConstraintType`]
                /// if `parameter` isn't collection-constrained, or an error if PICam
                /// is unable to query this.
                pub fn collection_constraints(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<Vec<CollectionConstraint>> {
                    let mut array: *const sys::PicamCollectionConstraint = std::ptr::null();
                    let mut count: sys::piint = 0;
                    picam_call!(sys::PicamAdvanced_GetParameterCollectionConstraints(
                        self.handle,
                        parameter.into().to_raw(),
                        &mut array,
                        &mut count
                    ))?;

                    let constraints =
                        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
                        unsafe { collect_raw_array(array, count, CollectionConstraint::from_raw) };

                    if !array.is_null() {
                        picam_call!(sys::Picam_DestroyCollectionConstraints(array))?;
                    }
                    Ok(constraints)
                }

                /// Gets every range constraint on `parameter`'s value (PICam manual
                /// 7.4.12.4).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterHasInvalidConstraintType`]
                /// if `parameter` isn't range-constrained, or an error if PICam
                /// is unable to query this.
                pub fn range_constraints(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<Vec<RangeConstraint>> {
                    let mut array: *const sys::PicamRangeConstraint = std::ptr::null();
                    let mut count: sys::piint = 0;
                    picam_call!(sys::PicamAdvanced_GetParameterRangeConstraints(
                        self.handle,
                        parameter.into().to_raw(),
                        &mut array,
                        &mut count
                    ))?;

                    let constraints =
                        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
                        unsafe { collect_raw_array(array, count, RangeConstraint::from_raw) };

                    if !array.is_null() {
                        picam_call!(sys::Picam_DestroyRangeConstraints(array))?;
                    }
                    Ok(constraints)
                }

                /// Watches `parameter`, a handle from [`params`](crate::params), sending
                /// each value it's set to, including when setting another parameter changes
                /// it (PICam manual 7.4.8 and 7.4.9.1). Each value is sent before the call
                /// that set it returns. Values the hardware changes by itself aren't sent,
                /// see [`Self::watch_extrinsic_value`].
                ///
                /// Values arrive on the returned receiver while the [`Watch`] lives. Keep it
                /// alive, as in `let (_watch, values) = ...`.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterDoesNotExist`] if `parameter`
                #[doc = concat!(" doesn't apply to this ", $noun, ", or an error if PICam is")]
                /// unable to register its callback.
                pub fn watch_value<T: PicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                ) -> Result<(Watch<'_>, Receiver<T>)> {
                    T::watch(RawHandle(self.handle), parameter.parameter())
                }

                /// Watches `parameter` like [`Self::watch_value`], as a `T` the caller
                /// chooses, for parameters with no handle in [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum other
                /// than the one `parameter` uses, [`crate::Error::ParameterDoesNotExist`] if
                #[doc = concat!(" `parameter` doesn't apply to this ", $noun, ", or an error if PICam")]
                /// is unable to register its callback.
                pub fn watch_value_untyped<T: PicamValue>(
                    &self,
                    parameter: Parameter,
                ) -> Result<(Watch<'_>, Receiver<T>)> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::watch(RawHandle(self.handle), parameter)
                }

                /// Watches a value the hardware changes by itself, such as the sensor
                /// temperature reading, sending each new value (PICam manual 7.4.9.3 and
                /// 7.4.9.7). Values are sent from a thread of PICam's, while the [`Watch`]
                /// lives.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::ParameterDoesNotExist`] if `parameter`
                #[doc = concat!(" doesn't apply to this ", $noun, ", or an error if PICam is")]
                /// unable to find the handle to register on or to register its callback.
                pub fn watch_extrinsic_value<T: ReadablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                ) -> Result<(Watch<'_>, Receiver<T>)> {
                    T::watch_extrinsic(RawHandle(self.extrinsic_handle()?), parameter.parameter())
                }

                /// Watches `parameter` like [`Self::watch_extrinsic_value`], as a `T` the
                /// caller chooses.
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::EnumeratedTypeMismatch`] if `T` is an enum other
                /// than the one `parameter` uses, [`crate::Error::ParameterDoesNotExist`] if
                #[doc = concat!(" `parameter` doesn't apply to this ", $noun, ", or an error if PICam")]
                /// is unable to find the handle to register on or to register its callback.
                pub fn watch_extrinsic_value_untyped<T: ReadablePicamValue>(
                    &self,
                    parameter: Parameter,
                ) -> Result<(Watch<'_>, Receiver<T>)> {
                    T::check_type(RawHandle(self.handle), parameter)?;
                    T::watch_extrinsic(RawHandle(self.extrinsic_handle()?), parameter)
                }

                /// Watches whether `parameter` is relevant, sending each change, such as
                /// when another parameter's value makes it relevant (PICam manual 7.4.10.1).
                /// Each change is sent before the call that caused it returns.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to register its callback.
                pub fn watch_relevance(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<(Watch<'_>, Receiver<bool>)> {
                    watch::subscribe(self.handle, parameter.into(), Kind::IsRelevant, |event| match event {
                        Event::IsRelevant(relevant) => Some(*relevant),
                        _ => None,
                    })
                }

                /// Watches how `parameter`'s value can be accessed, sending each change
                /// (PICam manual 7.4.10.3). Each change is sent before the call that caused
                /// it returns.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to register its callback.
                pub fn watch_value_access(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<(Watch<'_>, Receiver<ValueAccess>)> {
                    watch::subscribe(self.handle, parameter.into(), Kind::ValueAccess, |event| match event {
                        Event::ValueAccess(access) => Some(*access),
                        _ => None,
                    })
                }
            }
        };
    };
}
pub(crate) use impl_parameter_methods;
