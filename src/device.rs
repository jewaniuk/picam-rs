//! The parameter methods cameras, camera devices and accessories share.

/// Implements the parameter methods cameras and accessories share on `$ty`,
/// whose `handle` field is its PICam handle: the calls `picam_accessory.h`
/// lists for accessories, which cameras support too. `$noun` names the type
/// in the docs. `$ty` must also have a private
/// `extrinsic_handle(&self) -> Result<sys::PicamHandle>`, the handle PICam
/// registers extrinsic value callbacks and status notifications on: a device
/// or accessory handle (PICam manual 7.4.9.3 and 7.4.9.9).
macro_rules! impl_parameter_methods {
    ($ty:ty, $noun:literal) => {
        // the method bodies name these directly, so they're imported here
        // rather than at each invocation
        const _: () = {
            use std::sync::mpsc::Receiver;
            use std::time::Duration;

            use $crate::constraint::{
                CollectionConstraint, ConstraintCategory, ConstraintType, RangeConstraint,
            };
            use $crate::convert::{collect_raw_array, duration_from_ms, timeout_ms};
            use $crate::error::{Error, Result, picam_call};
            use $crate::parameter::sealed::RawHandle;
            use $crate::parameter::{
                Dynamics, EnumeratedType, Parameter, PicamValue, ReadablePicamValue, TypedParameter,
                ValueAccess, ValueType, WaitablePicamValue, can_read, can_set_online,
                can_wait_for_status, constraint_type, enumerated_type, is_relevant, value_access,
                value_type,
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

                /// Reports whether every value set has been committed to the hardware
                /// (PICam manual 5.4.6.1). On a camera or camera device, this is the
                /// camera model's values; an accessory applies each value as it's set.
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

                /// Commits the values set with [`Camera::set`](crate::Camera::set) on
                /// the camera's model to the hardware, validating them against PICam's
                /// constraints (PICam manual 5.4.6.2). On a camera device, `set` has
                /// already reached the hardware, and this commits any values still
                /// pending on the camera's model; an accessory applies each value as
                /// it's set.
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

                /// Lists the values status `parameter` can take, a handle from
                /// [`params`](crate::params) whose value enum fixes their type
                /// (PICam manual 5.4.2.15). [`Self::can_wait_for_status`] says which
                /// parameters are waitable statuses.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to list them, for example
                /// because `parameter` isn't a waitable status.
                pub fn status_parameter_purview<T: WaitablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                ) -> Result<Vec<T>> {
                    let values = self.status_parameter_purview_untyped(parameter.parameter())?;
                    Ok(values.into_iter().map(T::from_status).collect())
                }

                /// Lists the values status `parameter` can take as PICam's integer
                /// codes.
                ///
                /// Prefer [`Self::status_parameter_purview`], which takes the type from
                /// the parameter. This is for parameters with no handle in
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to list them, for example
                /// because `parameter` isn't a waitable status.
                pub fn status_parameter_purview_untyped(
                    &self,
                    parameter: Parameter,
                ) -> Result<Vec<i32>> {
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

                /// Estimates how long status `parameter` will take to reach `value`
                /// (PICam manual 5.4.2.17), or `None` if PICam can't estimate it.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to estimate this, for example
                /// because `parameter` isn't a waitable status.
                pub fn estimate_time_to_status_parameter_value<T: WaitablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                    value: T,
                ) -> Result<Option<Duration>> {
                    self.estimate_time_to_status_parameter_value_untyped(
                        parameter.parameter(),
                        value.to_status(),
                    )
                }

                /// [`Self::estimate_time_to_status_parameter_value`] with the value as
                /// PICam's integer code, for parameters with no handle in
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to estimate this, for example
                /// because `parameter` isn't a waitable status.
                pub fn estimate_time_to_status_parameter_value_untyped(
                    &self,
                    parameter: Parameter,
                    value: i32,
                ) -> Result<Option<Duration>> {
                    let mut estimated_time: sys::piint = 0;
                    picam_call!(sys::Picam_EstimateTimeToStatusParameterValue(
                        self.handle,
                        parameter.to_raw(),
                        value,
                        &mut estimated_time
                    ))?;
                    // -1 when picam can't estimate it (manual 5.4.2.17)
                    Ok(duration_from_ms(estimated_time))
                }

                /// Blocks until status `parameter` reaches `value`, or `timeout` has
                /// elapsed, rounded up to milliseconds. `None` waits forever (PICam
                /// manual 5.4.2.18).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::TimeOutOccurred`] if `timeout` elapses
                /// first, or an error if PICam is unable to wait for this, for example
                /// because `parameter` isn't a waitable status.
                pub fn wait_for_status_parameter_value<T: WaitablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                    value: T,
                    timeout: Option<Duration>,
                ) -> Result<()> {
                    self.wait_for_status_parameter_value_untyped(
                        parameter.parameter(),
                        value.to_status(),
                        timeout,
                    )
                }

                /// [`Self::wait_for_status_parameter_value`] with the value as PICam's
                /// integer code, for parameters with no handle in
                /// [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns [`crate::Error::TimeOutOccurred`] if `timeout` elapses
                /// first, or an error if PICam is unable to wait for this, for example
                /// because `parameter` isn't a waitable status.
                pub fn wait_for_status_parameter_value_untyped(
                    &self,
                    parameter: Parameter,
                    value: i32,
                    timeout: Option<Duration>,
                ) -> Result<()> {
                    picam_call!(sys::Picam_WaitForStatusParameterValue(
                        self.handle,
                        parameter.to_raw(),
                        value,
                        timeout_ms(timeout)
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
                ) -> Result<(Watch, Receiver<T>)> {
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
                ) -> Result<(Watch, Receiver<T>)> {
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
                /// unable to register its callback.
                pub fn watch_extrinsic_value<T: ReadablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                ) -> Result<(Watch, Receiver<T>)> {
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
                /// is unable to register its callback.
                pub fn watch_extrinsic_value_untyped<T: ReadablePicamValue>(
                    &self,
                    parameter: Parameter,
                ) -> Result<(Watch, Receiver<T>)> {
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
                ) -> Result<(Watch, Receiver<bool>)> {
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
                ) -> Result<(Watch, Receiver<ValueAccess>)> {
                    watch::subscribe(self.handle, parameter.into(), Kind::ValueAccess, |event| match event {
                        Event::ValueAccess(access) => Some(*access),
                        _ => None,
                    })
                }

                /// Watches `parameter`'s collection constraint, sending the new constraint
                /// each time setting another parameter changes it (PICam manual 7.4.12.2).
                /// Each change is sent before the `set` that caused it returns.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to register its callback.
                pub fn watch_collection_constraint(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<(Watch, Receiver<CollectionConstraint>)> {
                    watch::subscribe(
                        self.handle,
                        parameter.into(),
                        Kind::CollectionConstraint,
                        |event| match event {
                            Event::CollectionConstraint(constraint) => Some(constraint.clone()),
                            _ => None,
                        },
                    )
                }

                /// Watches `parameter`'s range constraint, sending the new constraint each
                /// time setting another parameter changes it (PICam manual 7.4.12.5). Each
                /// change is sent before the `set` that caused it returns.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to register its callback.
                pub fn watch_range_constraint(
                    &self,
                    parameter: impl Into<Parameter>,
                ) -> Result<(Watch, Receiver<RangeConstraint>)> {
                    watch::subscribe(
                        self.handle,
                        parameter.into(),
                        Kind::RangeConstraint,
                        |event| match event {
                            Event::RangeConstraint(constraint) => Some(constraint.clone()),
                            _ => None,
                        },
                    )
                }

                /// Notifies once when status `parameter` reaches `value`, or when an error
                /// stops it from doing so (PICam manual 7.4.9.9). The receiver gets `Ok(())`
                /// or the error, then reports the channel disconnected. Dropping the watch
                /// first cancels the notification. [`Self::can_wait_for_status`] says which
                /// parameters can be waited on, and
                /// [`Self::wait_for_status_parameter_value`] waits in place instead.
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to set the notification, for example
                /// because `parameter` isn't a status that can be waited on.
                pub fn notify_when_status<T: WaitablePicamValue>(
                    &self,
                    parameter: TypedParameter<T>,
                    value: T,
                ) -> Result<(Watch, Receiver<Result<()>>)> {
                    self.notify_when_status_untyped(parameter.parameter(), value.to_status())
                }

                /// [`Self::notify_when_status`] with the value as PICam's integer
                /// code, for parameters with no handle in [`params`](crate::params).
                ///
                /// # Errors
                ///
                /// Returns an error if PICam is unable to set the notification, for
                /// example because `parameter` isn't a status that can be waited on.
                pub fn notify_when_status_untyped(
                    &self,
                    parameter: Parameter,
                    value: i32,
                ) -> Result<(Watch, Receiver<Result<()>>)> {
                    watch::subscribe(
                        self.extrinsic_handle()?,
                        parameter,
                        Kind::Status(value),
                        |event| match event {
                            Event::Status(result) => Some(*result),
                            _ => None,
                        },
                    )
                }
            }
        };
    };
}
pub(crate) use impl_parameter_methods;
