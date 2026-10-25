use crate::convert::{collect_raw_array, define_picam_enum};
use crate::sys;

define_picam_enum!(
    ConstraintType,
    sys::PicamConstraintType,
    None = PicamConstraintType_None,
    Range = PicamConstraintType_Range,
    Collection = PicamConstraintType_Collection,
    Rois = PicamConstraintType_Rois,
    Pulse = PicamConstraintType_Pulse,
    Modulations = PicamConstraintType_Modulations,
);
define_picam_enum!(
    ConstraintScope,
    sys::PicamConstraintScope,
    Independent = PicamConstraintScope_Independent,
    Dependent = PicamConstraintScope_Dependent,
);
define_picam_enum!(
    ConstraintCategory,
    sys::PicamConstraintCategory,
    Capable = PicamConstraintCategory_Capable,
    Required = PicamConstraintCategory_Required,
    Recommended = PicamConstraintCategory_Recommended,
);

/// A collection-style constraint: a parameter's value must be one of a
/// fixed set of values.
///
/// Field meanings follow PICam manual 5.3.2.1.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionConstraint {
    /// Whether the constraint is fixed or depends on other parameters' values.
    pub scope: sys::PicamConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: sys::PicamConstraintSeverity,
    /// The values the parameter may take.
    pub values: Vec<sys::piflt>,
}

impl CollectionConstraint {
    pub(crate) fn from_raw(raw: &sys::PicamCollectionConstraint) -> Self {
        Self {
            scope: raw.scope,
            severity: raw.severity,
            // SAFETY: PICam guarantees `values_array` has `values_count`
            // valid, contiguous entries whenever this struct is valid
            values: unsafe { collect_raw_array(raw.values_array, raw.values_count, |&v| v) },
        }
    }
}

/// A range-style constraint: a parameter's value must fall within a
/// min/max range (in increments of some step), excluding certain values.
///
/// Field meanings follow PICam manual 5.3.2.2.
#[derive(Debug, Clone, PartialEq)]
pub struct RangeConstraint {
    /// Whether the constraint is fixed or depends on other parameters' values.
    pub scope: sys::PicamConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: sys::PicamConstraintSeverity,
    /// `true` when no valid values exist, in which case only `scope` and `severity` mean anything.
    pub empty_set: bool,
    /// The smallest value in the range. `outlying_values` may hold a smaller valid value.
    pub minimum: sys::piflt,
    /// The largest value in the range. `outlying_values` may hold a larger valid value.
    pub maximum: sys::piflt,
    /// The step between consecutive values in the range.
    pub increment: sys::piflt,
    /// Values strictly between `minimum` and `maximum` that are not valid. Empty when every value
    /// in the range is valid.
    pub excluded_values: Vec<sys::piflt>,
    /// Valid values that lie outside the range. Empty when there are none.
    pub outlying_values: Vec<sys::piflt>,
}

impl RangeConstraint {
    pub(crate) fn from_raw(raw: &sys::PicamRangeConstraint) -> Self {
        Self {
            scope: raw.scope,
            severity: raw.severity,
            empty_set: raw.empty_set != 0,
            minimum: raw.minimum,
            maximum: raw.maximum,
            increment: raw.increment,
            // SAFETY: PICam guarantees `excluded_values_array` has
            // `excluded_values_count` valid, contiguous entries
            // whenever this struct is valid
            excluded_values: unsafe {
                collect_raw_array(raw.excluded_values_array, raw.excluded_values_count, |&v| v)
            },
            // SAFETY: PICam guarantees `outlying_values_array` has
            // `outlying_values_count` valid, contiguous entries
            // whenever this struct is valid
            outlying_values: unsafe {
                collect_raw_array(raw.outlying_values_array, raw.outlying_values_count, |&v| v)
            },
        }
    }
}

/// A constraint on the regions of interest ("ROIs") that can be configured
/// for a parameter.
///
/// Field meanings follow PICam manual 5.3.1.1.
#[derive(Debug, Clone, PartialEq)]
pub struct RoisConstraint {
    /// Whether the constraint is fixed or depends on other parameters' values.
    pub scope: sys::PicamConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: sys::PicamConstraintSeverity,
    /// `true` when no valid regions of interest exist, in which case only `scope` and `severity`
    /// mean anything.
    pub empty_set: bool,
    /// Extra rules the set of regions must follow, such as symmetry or uniform binning.
    pub rules: sys::PicamRoisConstraintRulesMask,
    /// The largest number of regions allowed.
    pub maximum_roi_count: sys::piint,
    /// The constraint on [`crate::Roi::x`].
    pub x_constraint: RangeConstraint,
    /// The constraint on [`crate::Roi::width`].
    pub width_constraint: RangeConstraint,
    /// Extra limits on [`crate::Roi::x_binning`]: the values it may take, or
    /// empty when there are none. Either way the binning must always divide
    /// evenly into the width.
    pub x_binning_limits: Vec<sys::piint>,
    /// The constraint on [`crate::Roi::y`].
    pub y_constraint: RangeConstraint,
    /// The constraint on [`crate::Roi::height`].
    pub height_constraint: RangeConstraint,
    /// Extra limits on [`crate::Roi::y_binning`]: the values it may take, or
    /// empty when there are none. Either way the binning must always divide
    /// evenly into the height.
    pub y_binning_limits: Vec<sys::piint>,
}

impl RoisConstraint {
    pub(crate) fn from_raw(raw: &sys::PicamRoisConstraint) -> Self {
        Self {
            scope: raw.scope,
            severity: raw.severity,
            empty_set: raw.empty_set != 0,
            rules: raw.rules,
            maximum_roi_count: raw.maximum_roi_count,
            // `x_constraint`/`width_constraint`/etc. are embedded structs,
            // not pointers - no separate allocation to worry about, just
            // recurse into `RangeConstraint::from_raw`
            x_constraint: RangeConstraint::from_raw(&raw.x_constraint),
            width_constraint: RangeConstraint::from_raw(&raw.width_constraint),
            // SAFETY: PICam guarantees `x_binning_limits_array` has
            // `x_binning_limits_count` valid, contiguous entries whenever
            // this struct is valid
            x_binning_limits: unsafe {
                collect_raw_array(
                    raw.x_binning_limits_array,
                    raw.x_binning_limits_count,
                    |&v| v,
                )
            },
            y_constraint: RangeConstraint::from_raw(&raw.y_constraint),
            height_constraint: RangeConstraint::from_raw(&raw.height_constraint),
            // SAFETY: PICam guarantees `y_binning_limits_array` has
            // `y_binning_limits_count` valid, contiguous entries whenever
            // this struct is valid
            y_binning_limits: unsafe {
                collect_raw_array(
                    raw.y_binning_limits_array,
                    raw.y_binning_limits_count,
                    |&v| v,
                )
            },
        }
    }
}

/// A constraint on the pulse (delay/width) that can be configured for a
/// parameter.
///
/// Field meanings follow PICam manual 5.3.1.2.
#[derive(Debug, Clone, PartialEq)]
pub struct PulseConstraint {
    /// Whether the constraint is fixed or depends on other parameters' values.
    pub scope: sys::PicamConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: sys::PicamConstraintSeverity,
    /// `true` when no valid pulses exist, in which case only `scope` and `severity` mean anything.
    pub empty_set: bool,
    /// The constraint on [`crate::Pulse::delay`].
    pub delay_constraint: RangeConstraint,
    /// The constraint on [`crate::Pulse::width`].
    pub width_constraint: RangeConstraint,
    /// The smallest allowed sum of the pulse's delay and width.
    pub minimum_duration: sys::piflt,
    /// The largest allowed sum of the pulse's delay and width.
    pub maximum_duration: sys::piflt,
}

impl PulseConstraint {
    pub(crate) fn from_raw(raw: &sys::PicamPulseConstraint) -> Self {
        Self {
            scope: raw.scope,
            severity: raw.severity,
            empty_set: raw.empty_set != 0,
            delay_constraint: RangeConstraint::from_raw(&raw.delay_constraint),
            width_constraint: RangeConstraint::from_raw(&raw.width_constraint),
            minimum_duration: raw.minimum_duration,
            maximum_duration: raw.maximum_duration,
        }
    }
}

/// A constraint on the custom intensifier modulation sequence that can be
/// configured for a parameter.
///
/// Field meanings follow PICam manual 5.3.1.3.
#[derive(Debug, Clone, PartialEq)]
pub struct ModulationsConstraint {
    /// Whether the constraint is fixed or depends on other parameters' values.
    pub scope: sys::PicamConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: sys::PicamConstraintSeverity,
    /// `true` when no valid modulation sequence points exist, in which case only `scope` and
    /// `severity` mean anything.
    pub empty_set: bool,
    /// The largest number of points allowed in the sequence.
    pub maximum_modulation_count: sys::piint,
    /// The constraint on [`crate::Modulation::duration`].
    pub duration_constraint: RangeConstraint,
    /// The constraint on [`crate::Modulation::frequency`].
    pub frequency_constraint: RangeConstraint,
    /// The constraint on [`crate::Modulation::phase`].
    pub phase_constraint: RangeConstraint,
    /// The constraint on [`crate::Modulation::output_signal_frequency`].
    pub output_signal_frequency_constraint: RangeConstraint,
}

impl ModulationsConstraint {
    pub(crate) fn from_raw(raw: &sys::PicamModulationsConstraint) -> Self {
        Self {
            scope: raw.scope,
            severity: raw.severity,
            empty_set: raw.empty_set != 0,
            maximum_modulation_count: raw.maximum_modulation_count,
            duration_constraint: RangeConstraint::from_raw(&raw.duration_constraint),
            frequency_constraint: RangeConstraint::from_raw(&raw.frequency_constraint),
            phase_constraint: RangeConstraint::from_raw(&raw.phase_constraint),
            output_signal_frequency_constraint: RangeConstraint::from_raw(
                &raw.output_signal_frequency_constraint,
            ),
        }
    }
}
