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
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionConstraint {
    pub scope: sys::PicamConstraintScope,
    pub severity: sys::PicamConstraintSeverity,
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
#[derive(Debug, Clone, PartialEq)]
pub struct RangeConstraint {
    pub scope: sys::PicamConstraintScope,
    pub severity: sys::PicamConstraintSeverity,
    pub empty_set: bool,
    pub minimum: sys::piflt,
    pub maximum: sys::piflt,
    pub increment: sys::piflt,
    pub excluded_values: Vec<sys::piflt>,
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
#[derive(Debug, Clone, PartialEq)]
pub struct RoisConstraint {
    pub scope: sys::PicamConstraintScope,
    pub severity: sys::PicamConstraintSeverity,
    pub empty_set: bool,
    pub rules: sys::PicamRoisConstraintRulesMask,
    pub maximum_roi_count: sys::piint,
    pub x_constraint: RangeConstraint,
    pub width_constraint: RangeConstraint,
    pub x_binning_limits: Vec<sys::piint>,
    pub y_constraint: RangeConstraint,
    pub height_constraint: RangeConstraint,
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
#[derive(Debug, Clone, PartialEq)]
pub struct PulseConstraint {
    pub scope: sys::PicamConstraintScope,
    pub severity: sys::PicamConstraintSeverity,
    pub empty_set: bool,
    pub delay_constraint: RangeConstraint,
    pub width_constraint: RangeConstraint,
    pub minimum_duration: sys::piflt,
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
#[derive(Debug, Clone, PartialEq)]
pub struct ModulationsConstraint {
    pub scope: sys::PicamConstraintScope,
    pub severity: sys::PicamConstraintSeverity,
    pub empty_set: bool,
    pub maximum_modulation_count: sys::piint,
    pub duration_constraint: RangeConstraint,
    pub frequency_constraint: RangeConstraint,
    pub phase_constraint: RangeConstraint,
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
