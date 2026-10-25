use crate::convert::{collect_raw_array, define_picam_enum};
use crate::sys;

define_picam_enum!(
    /// The kind of constraint on a parameter's value, which decides which
    /// `Camera::*_constraint` method reads it (PICam manual 5.1.1.6).
    ConstraintType,
    sys::PicamConstraintType,
    /// No constraint, because the value is read-only.
    None = PicamConstraintType_None,
    /// A numeric range, read with [`crate::Camera::range_constraint`].
    Range = PicamConstraintType_Range,
    /// A fixed set of choices, read with [`crate::Camera::collection_constraint`].
    Collection = PicamConstraintType_Collection,
    /// Rules for regions of interest, read with [`crate::Camera::rois_constraint`].
    Rois = PicamConstraintType_Rois,
    /// Limits on a gate pulse, read with [`crate::Camera::pulse_constraint`].
    Pulse = PicamConstraintType_Pulse,
    /// Limits on a custom modulation sequence, read with
    /// [`crate::Camera::modulations_constraint`].
    Modulations = PicamConstraintType_Modulations,
);
define_picam_enum!(
    /// Whether a constraint is fixed or depends on other parameters
    /// (PICam manual 5.1.3.1).
    ConstraintScope,
    sys::PicamConstraintScope,
    /// The constraint never changes.
    Independent = PicamConstraintScope_Independent,
    /// The constraint can change with other parameters' values.
    Dependent = PicamConstraintScope_Dependent,
);
define_picam_enum!(
    /// Which set of values a constraint query describes (PICam manual 5.1.3.3).
    ConstraintCategory,
    sys::PicamConstraintCategory,
    /// Every value the camera could ever accept.
    Capable = PicamConstraintCategory_Capable,
    /// The values allowed right now, given the other parameters.
    Required = PicamConstraintCategory_Required,
    /// The values suited to most uses.
    Recommended = PicamConstraintCategory_Recommended,
);
define_picam_enum!(
    /// What it means for a value to break a constraint (PICam manual 5.1.3.2).
    ConstraintSeverity,
    sys::PicamConstraintSeverity,
    /// The value is in error.
    Error = PicamConstraintSeverity_Error,
    /// Breaking the constraint is only a warning, which deserves attention.
    Warning = PicamConstraintSeverity_Warning,
);

bitflags::bitflags! {
    /// The extra rules a set of regions of interest must follow, from a
    /// [`RoisConstraint`] (PICam manual 5.1.3.4).
    ///
    /// No flags set means there are no extra rules. Bits this crate doesn't
    /// recognize are kept, not dropped.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct RoisConstraintRules: i32 {
        /// Regions that share columns must bin them the same way, and must
        /// start on a boundary of that binning.
        const X_BINNING_ALIGNMENT =
            sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_XBinningAlignment.0;
        /// Regions that share rows must bin them the same way, and must start
        /// on a boundary of that binning.
        const Y_BINNING_ALIGNMENT =
            sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_YBinningAlignment.0;
        /// Each region must either straddle the vertical centre line of the sensor
        /// or be mirrored by another region across it.
        const HORIZONTAL_SYMMETRY =
            sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_HorizontalSymmetry.0;
        /// Each region must either straddle the horizontal centre line of the sensor
        /// or be mirrored by another region across it.
        const VERTICAL_SYMMETRY =
            sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_VerticalSymmetry.0;
        /// A region that straddles a line of symmetry may not bin pixels from
        /// both sides of it together.
        const SYMMETRY_BOUNDS_BINNING =
            sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_SymmetryBoundsBinning.0;
        /// Each region must bin as many columns as rows.
        const UNIFORM_BINNING =
            sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_UniformBinning.0;
    }
}

impl RoisConstraintRules {
    /// Converts a raw `PicamRoisConstraintRulesMask`, keeping any bits this
    /// crate doesn't recognize.
    pub(crate) fn from_raw(raw: sys::PicamRoisConstraintRulesMask) -> Self {
        Self::from_bits_retain(raw.0)
    }
}

/// A collection-style constraint: a parameter's value must be one of a
/// fixed set of values.
///
/// Field meanings follow PICam manual 5.3.2.1.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionConstraint {
    /// Whether the constraint is fixed or depends on other parameters' values.
    pub scope: ConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: ConstraintSeverity,
    /// The values the parameter may take.
    pub values: Vec<sys::piflt>,
}

impl CollectionConstraint {
    pub(crate) fn from_raw(raw: &sys::PicamCollectionConstraint) -> Self {
        Self {
            scope: ConstraintScope::from_raw(raw.scope),
            severity: ConstraintSeverity::from_raw(raw.severity),
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
    pub scope: ConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: ConstraintSeverity,
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
            scope: ConstraintScope::from_raw(raw.scope),
            severity: ConstraintSeverity::from_raw(raw.severity),
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
    pub scope: ConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: ConstraintSeverity,
    /// `true` when no valid regions of interest exist, in which case only `scope` and `severity`
    /// mean anything.
    pub empty_set: bool,
    /// Extra rules the set of regions must follow, such as symmetry or uniform binning.
    pub rules: RoisConstraintRules,
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
            scope: ConstraintScope::from_raw(raw.scope),
            severity: ConstraintSeverity::from_raw(raw.severity),
            empty_set: raw.empty_set != 0,
            rules: RoisConstraintRules::from_raw(raw.rules),
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
    pub scope: ConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: ConstraintSeverity,
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
            scope: ConstraintScope::from_raw(raw.scope),
            severity: ConstraintSeverity::from_raw(raw.severity),
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
    pub scope: ConstraintScope,
    /// Whether a value that breaks the constraint is an error or only a warning.
    pub severity: ConstraintSeverity,
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
            scope: ConstraintScope::from_raw(raw.scope),
            severity: ConstraintSeverity::from_raw(raw.severity),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::assert_round_trips;

    #[test]
    fn constraint_enums_round_trip() {
        assert_round_trips!(ConstraintType, sys::PicamConstraintType, 6);
        assert_round_trips!(ConstraintScope, sys::PicamConstraintScope, 2);
        assert_round_trips!(ConstraintCategory, sys::PicamConstraintCategory, 3);
        assert_round_trips!(ConstraintSeverity, sys::PicamConstraintSeverity, 2);
    }

    #[test]
    fn rois_constraint_rules_match_picam() {
        let rules = [
            (
                sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_XBinningAlignment,
                RoisConstraintRules::X_BINNING_ALIGNMENT,
            ),
            (
                sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_YBinningAlignment,
                RoisConstraintRules::Y_BINNING_ALIGNMENT,
            ),
            (
                sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_HorizontalSymmetry,
                RoisConstraintRules::HORIZONTAL_SYMMETRY,
            ),
            (
                sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_VerticalSymmetry,
                RoisConstraintRules::VERTICAL_SYMMETRY,
            ),
            (
                sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_SymmetryBoundsBinning,
                RoisConstraintRules::SYMMETRY_BOUNDS_BINNING,
            ),
            (
                sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_UniformBinning,
                RoisConstraintRules::UNIFORM_BINNING,
            ),
        ];
        for (raw, flag) in rules {
            assert_eq!(RoisConstraintRules::from_raw(raw), flag);
        }

        let none = sys::PicamRoisConstraintRulesMask::PicamRoisConstraintRulesMask_None;
        assert!(RoisConstraintRules::from_raw(none).is_empty());
    }

    #[test]
    fn rois_constraint_rules_keep_unknown_bits() {
        // 0x40 isn't a rule in sdk 5.15, so it must survive as an unnamed bit
        let rules = RoisConstraintRules::from_raw(sys::PicamRoisConstraintRulesMask(0x41));
        assert!(rules.contains(RoisConstraintRules::X_BINNING_ALIGNMENT));
        assert_eq!(rules.bits(), 0x41);
    }
}
