//! Parameter validation: checking pending parameter values against their
//! constraints without committing them (`picam_advanced.h`, PICam manual
//! 7.4.13).

use crate::camera::Camera;
use crate::constraint::ConstraintScope;
use crate::convert::collect_raw_array;
use crate::error::{Error, Result, picam_call};
use crate::parameter::Parameter;
use crate::sys;

/// Reads the one value `ptr` points to, or `None` when it's null.
///
/// # Safety
///
/// `ptr` must be null or point to a valid `T`.
unsafe fn read_optional<T: Copy>(ptr: *const T) -> Option<T> {
    // SAFETY: just checked `ptr` is non-null, and the caller guarantees it
    // then points to a valid `T`
    (!ptr.is_null()).then(|| unsafe { *ptr })
}

/// The result of validating one parameter (PICam manual 7.2.2.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// Whether the parameter passed validation.
    pub is_valid: bool,
    /// The parameter that failed validation, `None` when it passed.
    pub failed_parameter: Option<Parameter>,
    /// The scope of the error constraint that failed, `None` when validation
    /// passed or only a warning constraint failed.
    pub failed_error_constraint_scope: Option<ConstraintScope>,
    /// The scope of the warning constraint that failed, `None` when
    /// validation passed or only an error constraint failed.
    pub failed_warning_constraint_scope: Option<ConstraintScope>,
    /// The parameters constraining the failed parameter, when a dependent
    /// error constraint failed. Empty otherwise.
    pub error_constraining_parameters: Vec<Parameter>,
    /// The parameters constraining the failed parameter, when a dependent
    /// warning constraint failed. Empty otherwise.
    pub warning_constraining_parameters: Vec<Parameter>,
}

impl ValidationResult {
    pub(crate) fn from_raw(raw: &sys::PicamValidationResult) -> Self {
        // SAFETY: PICam guarantees each pointer is null or points to one
        // valid value, and that each array has its count of valid, contiguous
        // entries whenever this struct is valid (manual 7.2.2.1)
        unsafe {
            Self {
                is_valid: raw.is_valid != 0,
                failed_parameter: read_optional(raw.failed_parameter).map(Parameter::from_raw),
                failed_error_constraint_scope: read_optional(raw.failed_error_constraint_scope)
                    .map(ConstraintScope::from_raw),
                failed_warning_constraint_scope: read_optional(raw.failed_warning_constraint_scope)
                    .map(ConstraintScope::from_raw),
                error_constraining_parameters: collect_raw_array(
                    raw.error_constraining_parameter_array,
                    raw.error_constraining_parameter_count,
                    |&parameter| Parameter::from_raw(parameter),
                ),
                warning_constraining_parameters: collect_raw_array(
                    raw.warning_constraining_parameter_array,
                    raw.warning_constraining_parameter_count,
                    |&parameter| Parameter::from_raw(parameter),
                ),
            }
        }
    }
}

/// The result of validating every parameter (PICam manual 7.2.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResults {
    /// Whether every parameter passed validation.
    pub is_valid: bool,
    /// A result for each parameter that failed validation, empty when all
    /// passed.
    pub results: Vec<ValidationResult>,
}

/// A parameter that failed validation because of the value of the parameter
/// constraining it (PICam manual 7.2.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailedDependentParameter {
    /// The parameter that failed validation.
    pub parameter: Parameter,
    /// The scope of the error constraint that failed, `None` when only a
    /// warning constraint failed.
    pub failed_error_constraint_scope: Option<ConstraintScope>,
    /// The scope of the warning constraint that failed, `None` when only an
    /// error constraint failed.
    pub failed_warning_constraint_scope: Option<ConstraintScope>,
}

/// The result of validating the parameters one parameter constrains (PICam
/// manual 7.2.2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependentValidationResult {
    /// Whether every parameter it constrains passed validation.
    pub is_valid: bool,
    /// The parameter whose value constrains the others.
    pub constraining_parameter: Parameter,
    /// The constrained parameters that failed validation, empty when all
    /// passed.
    pub failed_dependent_parameters: Vec<FailedDependentParameter>,
}

impl Camera {
    /// Validates `parameter`'s pending value against all its constraints,
    /// without committing it (PICam manual 7.4.13.3).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no result, or an error if PICam is unable to
    /// validate it, for example because `parameter` doesn't apply to this
    /// camera.
    pub fn validate_parameter(&self, parameter: impl Into<Parameter>) -> Result<ValidationResult> {
        let mut ptr: *const sys::PicamValidationResult = std::ptr::null();
        picam_call!(sys::PicamAdvanced_ValidateParameter(
            self.raw_handle(),
            parameter.into().to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamValidationResult`.
        let result = ValidationResult::from_raw(unsafe { &*ptr });
        picam_call!(sys::Picam_DestroyValidationResult(ptr))?;
        Ok(result)
    }

    /// Validates every parameter's pending value against its constraints,
    /// without committing them (PICam manual 7.4.13.4).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no results, or an error if PICam is unable to
    /// validate them.
    pub fn validate_parameters(&self) -> Result<ValidationResults> {
        let mut ptr: *const sys::PicamValidationResults = std::ptr::null();
        picam_call!(sys::PicamAdvanced_ValidateParameters(
            self.raw_handle(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamValidationResults`, whose `validation_result_array` has
        // `validation_result_count` valid, contiguous entries (manual
        // 7.2.2.2, see `collect_raw_array`)
        let results = unsafe {
            let raw = &*ptr;
            ValidationResults {
                is_valid: raw.is_valid != 0,
                results: collect_raw_array(
                    raw.validation_result_array,
                    raw.validation_result_count,
                    ValidationResult::from_raw,
                ),
            }
        };
        picam_call!(sys::Picam_DestroyValidationResults(ptr))?;
        Ok(results)
    }

    /// Validates the pending values of every parameter `parameter`
    /// constrains, against the constraints `parameter`'s value sets, without
    /// committing them (PICam manual 7.4.13.6).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no result, or an error if PICam is unable to
    /// validate them, for example because `parameter` doesn't apply to this
    /// camera.
    pub fn validate_dependent_parameter(
        &self,
        parameter: impl Into<Parameter>,
    ) -> Result<DependentValidationResult> {
        let mut ptr: *const sys::PicamDependentValidationResult = std::ptr::null();
        picam_call!(sys::PicamAdvanced_ValidateDependentParameter(
            self.raw_handle(),
            parameter.into().to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamDependentValidationResult`, whose array has its count of
        // valid, contiguous entries, each of whose scope pointers is null or
        // points to one valid scope (manual 7.2.2.3 and 7.2.2.4)
        let result = unsafe {
            let raw = &*ptr;
            DependentValidationResult {
                is_valid: raw.is_valid != 0,
                constraining_parameter: Parameter::from_raw(raw.constraining_parameter),
                failed_dependent_parameters: collect_raw_array(
                    raw.failed_dependent_parameter_array,
                    raw.failed_dependent_parameter_count,
                    |failed| FailedDependentParameter {
                        parameter: Parameter::from_raw(failed.failed_parameter),
                        failed_error_constraint_scope: read_optional(
                            failed.failed_error_constraint_scope,
                        )
                        .map(ConstraintScope::from_raw),
                        failed_warning_constraint_scope: read_optional(
                            failed.failed_warning_constraint_scope,
                        )
                        .map(ConstraintScope::from_raw),
                    },
                ),
            }
        };
        picam_call!(sys::Picam_DestroyDependentValidationResult(ptr))?;
        Ok(result)
    }
}
