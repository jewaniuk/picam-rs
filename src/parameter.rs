//! PICam parameter identifiers and value access.

use crate::convert::{collect_raw_array, define_picam_enum};
use crate::error::{Result, picam_call};
use crate::sys;

define_picam_enum!(
    ComputerInterface,
    sys::PicamComputerInterface,
    Usb2 = PicamComputerInterface_Usb2,
    Usb3 = PicamComputerInterface_Usb3,
    GigabitEthernet = PicamComputerInterface_GigabitEthernet,
);
define_picam_enum!(
    ValueType,
    sys::PicamValueType,
    Integer = PicamValueType_Integer,
    Boolean = PicamValueType_Boolean,
    Enumeration = PicamValueType_Enumeration,
    LargeInteger = PicamValueType_LargeInteger,
    FloatingPoint = PicamValueType_FloatingPoint,
    Rois = PicamValueType_Rois,
    Pulse = PicamValueType_Pulse,
    Modulations = PicamValueType_Modulations,
);
define_picam_enum!(
    ValueAccess,
    sys::PicamValueAccess,
    ReadOnly = PicamValueAccess_ReadOnly,
    ReadWriteTrivial = PicamValueAccess_ReadWriteTrivial,
    ReadWrite = PicamValueAccess_ReadWrite,
);

macro_rules! define_parameters {
    ($($name:ident = $raw:ident),*$(,)?) => {
        /// Identifies a specific camera parameter (exposure time, ROI,
        /// gain, and so on).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Parameter{
            $($name,)*
            /// A parameter this crate doesn't recognize, most likely
            /// because the installed PICam runtime is newer than the
            /// SDK version these bindings were generated against.
            Unknown(i32),
        }

        impl Parameter {
            pub(crate) fn from_raw(raw: sys::PicamParameter) -> Self {
                match raw {
                    $(sys::PicamParameter::$raw => Parameter::$name,)*
                    other => Parameter::Unknown(other.0),
                }
            }

            pub(crate) fn to_raw(self) -> sys::PicamParameter {
                match self {
                    $(Parameter::$name => sys::PicamParameter::$raw,)*
                    Parameter::Unknown(code) => sys::PicamParameter(code),
                }
            }
        }
    };
}

define_parameters! {
    ExposureTime = PicamParameter_ExposureTime,
    ExposureTimeResult = PicamParameter_ExposureTimeResult,
    ShutterTimingMode = PicamParameter_ShutterTimingMode,
    ShutterOpeningDelay = PicamParameter_ShutterOpeningDelay,
    ShutterClosingDelay = PicamParameter_ShutterClosingDelay,
    ShutterDelayResolution = PicamParameter_ShutterDelayResolution,
    InternalShutterType = PicamParameter_InternalShutterType,
    InternalShutterStatus = PicamParameter_InternalShutterStatus,
    ExternalShutterType = PicamParameter_ExternalShutterType,
    ExternalShutterStatus = PicamParameter_ExternalShutterStatus,
    ActiveShutter = PicamParameter_ActiveShutter,
    InactiveShutterTimingModeResult = PicamParameter_InactiveShutterTimingModeResult,
    GatingMode = PicamParameter_GatingMode,
    RepetitiveGate = PicamParameter_RepetitiveGate,
    SequentialStartingGate = PicamParameter_SequentialStartingGate,
    SequentialEndingGate = PicamParameter_SequentialEndingGate,
    SequentialGateStepCount = PicamParameter_SequentialGateStepCount,
    SequentialGateStepIterations = PicamParameter_SequentialGateStepIterations,
    DifStartingGate = PicamParameter_DifStartingGate,
    DifEndingGate = PicamParameter_DifEndingGate,
    EnableIntensifier = PicamParameter_EnableIntensifier,
    IntensifierStatus = PicamParameter_IntensifierStatus,
    IntensifierGain = PicamParameter_IntensifierGain,
    EMIccdGainControlMode = PicamParameter_EMIccdGainControlMode,
    EMIccdGain = PicamParameter_EMIccdGain,
    PhosphorDecayDelay = PicamParameter_PhosphorDecayDelay,
    PhosphorDecayDelayResolution = PicamParameter_PhosphorDecayDelayResolution,
    BracketGating = PicamParameter_BracketGating,
    IntensifierOptions = PicamParameter_IntensifierOptions,
    EnableModulation = PicamParameter_EnableModulation,
    ModulationDuration = PicamParameter_ModulationDuration,
    ModulationFrequency = PicamParameter_ModulationFrequency,
    RepetitiveModulationPhase = PicamParameter_RepetitiveModulationPhase,
    SequentialStartingModulationPhase = PicamParameter_SequentialStartingModulationPhase,
    SequentialEndingModulationPhase = PicamParameter_SequentialEndingModulationPhase,
    CustomModulationSequence = PicamParameter_CustomModulationSequence,
    PhotocathodeSensitivity = PicamParameter_PhotocathodeSensitivity,
    GatingSpeed = PicamParameter_GatingSpeed,
    PhosphorType = PicamParameter_PhosphorType,
    IntensifierDiameter = PicamParameter_IntensifierDiameter,
    AdcSpeed = PicamParameter_AdcSpeed,
    AdcBitDepth = PicamParameter_AdcBitDepth,
    AdcAnalogGain = PicamParameter_AdcAnalogGain,
    AdcQuality = PicamParameter_AdcQuality,
    AdcEMGain = PicamParameter_AdcEMGain,
    CorrectPixelBias = PicamParameter_CorrectPixelBias,
    TriggerSource = PicamParameter_TriggerSource,
    TriggerResponse = PicamParameter_TriggerResponse,
    TriggerDetermination = PicamParameter_TriggerDetermination,
    TriggerFrequency = PicamParameter_TriggerFrequency,
    TriggerTermination = PicamParameter_TriggerTermination,
    TriggerCoupling = PicamParameter_TriggerCoupling,
    TriggerThreshold = PicamParameter_TriggerThreshold,
    TriggerDelay = PicamParameter_TriggerDelay,
    OutputSignal = PicamParameter_OutputSignal,
    InvertOutputSignal = PicamParameter_InvertOutputSignal,
    OutputSignal2 = PicamParameter_OutputSignal2,
    InvertOutputSignal2 = PicamParameter_InvertOutputSignal2,
    EnableAuxOutput = PicamParameter_EnableAuxOutput,
    AuxOutput = PicamParameter_AuxOutput,
    EnableSyncMaster = PicamParameter_EnableSyncMaster,
    SyncMaster2Delay = PicamParameter_SyncMaster2Delay,
    EnableModulationOutputSignal = PicamParameter_EnableModulationOutputSignal,
    ModulationOutputSignalFrequency = PicamParameter_ModulationOutputSignalFrequency,
    ModulationOutputSignalAmplitude = PicamParameter_ModulationOutputSignalAmplitude,
    AnticipateTrigger = PicamParameter_AnticipateTrigger,
    DelayFromPreTrigger = PicamParameter_DelayFromPreTrigger,
    ReadoutControlMode = PicamParameter_ReadoutControlMode,
    ReadoutTimeCalculation = PicamParameter_ReadoutTimeCalculation,
    ReadoutPortCount = PicamParameter_ReadoutPortCount,
    ReadoutOrientation = PicamParameter_ReadoutOrientation,
    KineticsWindowHeight = PicamParameter_KineticsWindowHeight,
    SeNsRWindowHeight = PicamParameter_SeNsRWindowHeight,
    VerticalShiftRate = PicamParameter_VerticalShiftRate,
    RowTimeResult = PicamParameter_RowTimeResult,
    Accumulations = PicamParameter_Accumulations,
    EnableNondestructiveReadout = PicamParameter_EnableNondestructiveReadout,
    NondestructiveReadoutPeriod = PicamParameter_NondestructiveReadoutPeriod,
    Rois = PicamParameter_Rois,
    NormalizeOrientation = PicamParameter_NormalizeOrientation,
    DisableDataFormatting = PicamParameter_DisableDataFormatting,
    ReadoutCount = PicamParameter_ReadoutCount,
    ExactReadoutCountMaximum = PicamParameter_ExactReadoutCountMaximum,
    PixelFormat = PicamParameter_PixelFormat,
    FrameSize = PicamParameter_FrameSize,
    FrameStride = PicamParameter_FrameStride,
    FramesPerReadout = PicamParameter_FramesPerReadout,
    ReadoutStride = PicamParameter_ReadoutStride,
    PixelBitDepth = PicamParameter_PixelBitDepth,
    ReadoutRateCalculation = PicamParameter_ReadoutRateCalculation,
    OnlineReadoutRateCalculation = PicamParameter_OnlineReadoutRateCalculation,
    FrameRateCalculation = PicamParameter_FrameRateCalculation,
    Orientation = PicamParameter_Orientation,
    TimeStamps = PicamParameter_TimeStamps,
    TimeStampResolution = PicamParameter_TimeStampResolution,
    TimeStampBitDepth = PicamParameter_TimeStampBitDepth,
    TrackFrames = PicamParameter_TrackFrames,
    FrameTrackingBitDepth = PicamParameter_FrameTrackingBitDepth,
    GateTracking = PicamParameter_GateTracking,
    GateTrackingBitDepth = PicamParameter_GateTrackingBitDepth,
    ModulationTracking = PicamParameter_ModulationTracking,
    ModulationTrackingBitDepth = PicamParameter_ModulationTrackingBitDepth,
    PhotonDetectionMode = PicamParameter_PhotonDetectionMode,
    PhotonDetectionThreshold = PicamParameter_PhotonDetectionThreshold,
    CorrectForDarkCurrent = PicamParameter_CorrectForDarkCurrent,
    Despeckle = PicamParameter_Despeckle,
    SensorType = PicamParameter_SensorType,
    CcdCharacteristics = PicamParameter_CcdCharacteristics,
    SensorActiveWidth = PicamParameter_SensorActiveWidth,
    SensorActiveHeight = PicamParameter_SensorActiveHeight,
    SensorActiveExtendedHeight = PicamParameter_SensorActiveExtendedHeight,
    SensorActiveLeftMargin = PicamParameter_SensorActiveLeftMargin,
    SensorActiveTopMargin = PicamParameter_SensorActiveTopMargin,
    SensorActiveRightMargin = PicamParameter_SensorActiveRightMargin,
    SensorActiveBottomMargin = PicamParameter_SensorActiveBottomMargin,
    SensorMaskedHeight = PicamParameter_SensorMaskedHeight,
    SensorMaskedTopMargin = PicamParameter_SensorMaskedTopMargin,
    SensorMaskedBottomMargin = PicamParameter_SensorMaskedBottomMargin,
    SensorSecondaryMaskedHeight = PicamParameter_SensorSecondaryMaskedHeight,
    SensorSecondaryActiveHeight = PicamParameter_SensorSecondaryActiveHeight,
    PixelWidth = PicamParameter_PixelWidth,
    PixelHeight = PicamParameter_PixelHeight,
    PixelGapWidth = PicamParameter_PixelGapWidth,
    PixelGapHeight = PicamParameter_PixelGapHeight,
    ApplicableStarDefectMapID = PicamParameter_ApplicableStarDefectMapID,
    ActiveWidth = PicamParameter_ActiveWidth,
    ActiveHeight = PicamParameter_ActiveHeight,
    ActiveExtendedHeight = PicamParameter_ActiveExtendedHeight,
    ActiveLeftMargin = PicamParameter_ActiveLeftMargin,
    ActiveTopMargin = PicamParameter_ActiveTopMargin,
    ActiveRightMargin = PicamParameter_ActiveRightMargin,
    ActiveBottomMargin = PicamParameter_ActiveBottomMargin,
    MaskedHeight = PicamParameter_MaskedHeight,
    MaskedTopMargin = PicamParameter_MaskedTopMargin,
    MaskedBottomMargin = PicamParameter_MaskedBottomMargin,
    SecondaryMaskedHeight = PicamParameter_SecondaryMaskedHeight,
    SecondaryActiveHeight = PicamParameter_SecondaryActiveHeight,
    CleanSectionFinalHeight = PicamParameter_CleanSectionFinalHeight,
    CleanSectionFinalHeightCount = PicamParameter_CleanSectionFinalHeightCount,
    CleanSerialRegister = PicamParameter_CleanSerialRegister,
    CleanCycleCount = PicamParameter_CleanCycleCount,
    CleanCycleHeight = PicamParameter_CleanCycleHeight,
    CleanBeforeExposure = PicamParameter_CleanBeforeExposure,
    CleanUntilTrigger = PicamParameter_CleanUntilTrigger,
    StopCleaningOnPreTrigger = PicamParameter_StopCleaningOnPreTrigger,
    SensorTemperatureSetPoint = PicamParameter_SensorTemperatureSetPoint,
    SensorTemperatureReading = PicamParameter_SensorTemperatureReading,
    SensorTemperatureStatus = PicamParameter_SensorTemperatureStatus,
    DisableCoolingFan = PicamParameter_DisableCoolingFan,
    CoolingFanStatus = PicamParameter_CoolingFanStatus,
    EnableSensorWindowHeater = PicamParameter_EnableSensorWindowHeater,
    VacuumStatus = PicamParameter_VacuumStatus,
    CenterWavelengthSetPoint = PicamParameter_CenterWavelengthSetPoint,
    CenterWavelengthReading = PicamParameter_CenterWavelengthReading,
    CenterWavelengthStatus = PicamParameter_CenterWavelengthStatus,
    GratingType = PicamParameter_GratingType,
    GratingCoating = PicamParameter_GratingCoating,
    GratingGrooveDensity = PicamParameter_GratingGrooveDensity,
    GratingBlazingWavelength = PicamParameter_GratingBlazingWavelength,
    FocalLength = PicamParameter_FocalLength,
    InclusionAngle = PicamParameter_InclusionAngle,
    SensorAngle = PicamParameter_SensorAngle,
    LaserOutputMode = PicamParameter_LaserOutputMode,
    LaserPower = PicamParameter_LaserPower,
    LaserWavelength = PicamParameter_LaserWavelength,
    LaserStatus = PicamParameter_LaserStatus,
    InputTriggerStatus = PicamParameter_InputTriggerStatus,
    LightSource = PicamParameter_LightSource,
    LightSourceStatus = PicamParameter_LightSourceStatus,
    Age = PicamParameter_Age,
    LifeExpectancy = PicamParameter_LifeExpectancy,
}

/// A value type PICam can get or set for a parameter.
pub trait PicamValue: Sized {
    /// Gets the current value of `parameter` on `handle`.
    fn get(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self>;

    /// Sets `parameter` on `handle` to `value`.
    fn set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()>;

    /// Gets the factor default value of `parameter` on `handle`.
    fn default_value(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self>;

    /// Reports whether `value` could currently be set for `parameter` on
    /// `handle`, without actually setting it.
    fn can_set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<bool>;
}

macro_rules! impl_picam_value {
    ($ty:ty, $get:ident, $set:ident, $default:ident, $can_set:ident) => {
        impl PicamValue for $ty {
            fn get(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$get(handle, parameter.to_raw(), &mut value))?;
                Ok(value)
            }

            fn set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()> {
                picam_call!(sys::$set(handle, parameter.to_raw(), value))
            }

            fn default_value(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$default(handle, parameter.to_raw(), &mut value))?;
                Ok(value)
            }

            fn can_set(
                handle: sys::PicamHandle,
                parameter: Parameter,
                value: Self,
            ) -> Result<bool> {
                let mut settable: sys::pibln = 0;
                picam_call!(sys::$can_set(
                    handle,
                    parameter.to_raw(),
                    value,
                    &mut settable
                ))?;
                Ok(settable != 0)
            }
        }
    };
}

impl_picam_value!(
    sys::piint,
    Picam_GetParameterIntegerValue,
    Picam_SetParameterIntegerValue,
    Picam_GetParameterIntegerDefaultValue,
    Picam_CanSetParameterIntegerValue
);
impl_picam_value!(
    sys::pi64s,
    Picam_GetParameterLargeIntegerValue,
    Picam_SetParameterLargeIntegerValue,
    Picam_GetParameterLargeIntegerDefaultValue,
    Picam_CanSetParameterLargeIntegerValue
);
impl_picam_value!(
    sys::piflt,
    Picam_GetParameterFloatingPointValue,
    Picam_SetParameterFloatingPointValue,
    Picam_GetParameterFloatingPointDefaultValue,
    Picam_CanSetParameterFloatingPointValue
);

macro_rules! impl_bool_query {
    ($name:ident, $raw:ident) => {
        pub(crate) fn $name(handle: sys::PicamHandle, parameter: Parameter) -> Result<bool> {
            let mut result: sys::pibln = 0;
            picam_call!(sys::$raw(handle, parameter.to_raw(), &mut result))?;
            Ok(result != 0)
        }
    };
}

macro_rules! impl_parameter_info {
    ($name:ident, $raw:ident, $ty:ty) => {
        pub(crate) fn $name(handle: sys::PicamHandle, parameter: Parameter) -> Result<$ty> {
            let mut result = std::mem::MaybeUninit::<$ty>::uninit();
            picam_call!(sys::$raw(handle, parameter.to_raw(), result.as_mut_ptr()))?;
            // SAFETY: on success, PICam guarantees this out-parameter was
            // fully written with a valid value.
            Ok(unsafe { result.assume_init() })
        }
    };
}

impl_bool_query!(can_set_online, Picam_CanSetParameterOnline);
impl_bool_query!(can_read, Picam_CanReadParameter);
impl_bool_query!(can_wait_for_status, Picam_CanWaitForStatusParameter);
impl_bool_query!(is_relevant, Picam_IsParameterRelevant);

impl_parameter_info!(value_type, Picam_GetParameterValueType, sys::PicamValueType);
impl_parameter_info!(
    enumerated_type,
    Picam_GetParameterEnumeratedType,
    sys::PicamEnumeratedType
);
impl_parameter_info!(
    value_access,
    Picam_GetParameterValueAccess,
    sys::PicamValueAccess
);
impl_parameter_info!(
    constraint_type,
    Picam_GetParameterConstraintType,
    sys::PicamConstraintType
);

/// A value type that can be read directly from hardware, bypassing PICam's
/// committed-value cache. Implemented only for the two value types PICam
/// supports this for.
pub trait ReadablePicamValue: PicamValue {
    /// Reads the current value of `parameter` directly from hardware.
    fn read(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self>;
}

macro_rules! impl_readable_value {
    ($ty:ty, $read:ident) => {
        impl ReadablePicamValue for $ty {
            fn read(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
                let mut value: $ty = Default::default();
                picam_call!(sys::$read(handle, parameter.to_raw(), &mut value))?;
                Ok(value)
            }
        }
    };
}

impl_readable_value!(sys::piint, Picam_ReadParameterIntegerValue);
impl_readable_value!(sys::piflt, Picam_ReadParameterFloatingPointValue);

/// A value type that can be set and applied to hardware immediately,
/// without needing [`crate::camera::Camera::commit_parameters`].
/// Implemented only for the three value types Picam supports this for.
pub trait OnlineSettablePicamValue: PicamValue {
    /// Sets `parameter` to `value` and applies it immediately.

    fn set_online(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()>;
}

macro_rules! impl_online_settable_scalar {
    ($ty:ty, $set_online:ident) => {
        impl OnlineSettablePicamValue for $ty {
            fn set_online(
                handle: sys::PicamHandle,
                parameter: Parameter,
                value: Self,
            ) -> Result<()> {
                picam_call!(sys::$set_online(handle, parameter.to_raw(), value))
            }
        }
    };
}

impl_online_settable_scalar!(sys::piint, Picam_SetParameterIntegerValueOnline);
impl_online_settable_scalar!(sys::piflt, Picam_SetParameterFloatingPointValueOnline);

/// A single region of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roi {
    pub x: sys::piint,
    pub width: sys::piint,
    pub x_binning: sys::piint,
    pub y: sys::piint,
    pub height: sys::piint,
    pub y_binning: sys::piint,
}

impl Roi {
    fn from_raw(raw: &sys::PicamRoi) -> Self {
        Self {
            x: raw.x,
            width: raw.width,
            x_binning: raw.x_binning,
            y: raw.y,
            height: raw.height,
            y_binning: raw.y_binning,
        }
    }

    fn to_raw(self) -> sys::PicamRoi {
        sys::PicamRoi {
            x: self.x,
            width: self.width,
            x_binning: self.x_binning,
            y: self.y,
            height: self.height,
            y_binning: self.y_binning,
        }
    }
}

impl PicamValue for Vec<Roi> {
    fn get(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamRois = std::ptr::null();
        picam_call!(sys::Picam_GetParameterRoisValue(
            handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let rois = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `roi_array` has `roi_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(raw.roi_array, raw.roi_count, Roi::from_raw)
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyRois(ptr))?;
        }
        Ok(rois)
    }

    fn set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()> {
        // `raw` holds a pointer into the buffer for `raw_rois` via `.as_mut_ptr()`,
        // which the borrow checker can't see through; `raw_rois` must stay alive
        // and unmodified (no push/reallocation, which would invalidate the
        // pointer) until after the call below returns; PICam only reads through
        // `raw` for the duration of this one call and doesn't retain the pointer
        let mut raw_rois: Vec<sys::PicamRoi> = value.iter().map(|roi| roi.to_raw()).collect();
        let raw = sys::PicamRois {
            roi_array: raw_rois.as_mut_ptr(),
            roi_count: raw_rois.len() as sys::piint,
        };
        picam_call!(sys::Picam_SetParameterRoisValue(
            handle,
            parameter.to_raw(),
            &raw
        ))
    }

    fn default_value(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamRois = std::ptr::null();
        picam_call!(sys::Picam_GetParameterRoisDefaultValue(
            handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let rois = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `roi_array` has `roi_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(raw.roi_array, raw.roi_count, Roi::from_raw)
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyRois(ptr))?;
        }
        Ok(rois)
    }

    fn can_set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<bool> {
        let mut raw_rois: Vec<sys::PicamRoi> = value.iter().map(|roi| roi.to_raw()).collect();
        let raw = sys::PicamRois {
            roi_array: raw_rois.as_mut_ptr(),
            roi_count: raw_rois.len() as sys::piint,
        };
        let mut settable: sys::pibln = 0;
        picam_call!(sys::Picam_CanSetParameterRoisValue(
            handle,
            parameter.to_raw(),
            &raw,
            &mut settable
        ))?;
        Ok(settable != 0)
    }
}

/// A single step in a custom intensifier modulation sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modulation {
    pub duration: sys::piflt,
    pub frequency: sys::piflt,
    pub phase: sys::piflt,
    pub output_signal_frequency: sys::piflt,
}

impl Modulation {
    fn from_raw(raw: &sys::PicamModulation) -> Self {
        Self {
            duration: raw.duration,
            frequency: raw.frequency,
            phase: raw.phase,
            output_signal_frequency: raw.output_signal_frequency,
        }
    }

    fn to_raw(self) -> sys::PicamModulation {
        sys::PicamModulation {
            duration: self.duration,
            frequency: self.frequency,
            phase: self.phase,
            output_signal_frequency: self.output_signal_frequency,
        }
    }
}

impl PicamValue for Vec<Modulation> {
    fn get(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamModulations = std::ptr::null();
        picam_call!(sys::Picam_GetParameterModulationsValue(
            handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let modulations = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `modulation_array` has `modulation_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(
                    raw.modulation_array,
                    raw.modulation_count,
                    Modulation::from_raw,
                )
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyModulations(ptr))?;
        }
        Ok(modulations)
    }

    fn set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()> {
        // `raw` holds a pointer into the buffer for `raw_modulations` via `.as_mut_ptr()`,
        // which the borrow checker can't see through; `raw_modulations` must stay alive
        // and unmodified (no push/reallocation, which would invalidate the
        // pointer) until after the call below returns; PICam only reads through
        // `raw` for the duration of this one call and doesn't retain the pointer
        let mut raw_modulations: Vec<sys::PicamModulation> =
            value.iter().map(|m| m.to_raw()).collect();
        let raw = sys::PicamModulations {
            modulation_array: raw_modulations.as_mut_ptr(),
            modulation_count: raw_modulations.len() as sys::piint,
        };
        picam_call!(sys::Picam_SetParameterModulationsValue(
            handle,
            parameter.to_raw(),
            &raw
        ))
    }

    fn default_value(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamModulations = std::ptr::null();
        picam_call!(sys::Picam_GetParameterModulationsDefaultValue(
            handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        let modulations = if ptr.is_null() {
            Vec::new()
        } else {
            // SAFETY: on success with a non-null pointer, PICam guarantees
            // `modulation_array` has `modulation_count` valid, contiguous entries
            unsafe {
                let raw = &*ptr;
                collect_raw_array(
                    raw.modulation_array,
                    raw.modulation_count,
                    Modulation::from_raw,
                )
            }
        };

        if !ptr.is_null() {
            picam_call!(sys::Picam_DestroyModulations(ptr))?;
        }
        Ok(modulations)
    }

    fn can_set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<bool> {
        let mut raw_modulations: Vec<sys::PicamModulation> =
            value.iter().map(|m| m.to_raw()).collect();
        let raw = sys::PicamModulations {
            modulation_array: raw_modulations.as_mut_ptr(),
            modulation_count: raw_modulations.len() as sys::piint,
        };
        let mut settable: sys::pibln = 0;
        picam_call!(sys::Picam_CanSetParameterModulationsValue(
            handle,
            parameter.to_raw(),
            &raw,
            &mut settable
        ))?;
        Ok(settable != 0)
    }
}

/// A single pulse's delay and width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pulse {
    pub delay: sys::piflt,
    pub width: sys::piflt,
}

impl Pulse {
    fn from_raw(raw: &sys::PicamPulse) -> Self {
        Self {
            delay: raw.delay,
            width: raw.width,
        }
    }

    fn to_raw(self) -> sys::PicamPulse {
        sys::PicamPulse {
            delay: self.delay,
            width: self.width,
        }
    }
}

impl PicamValue for Pulse {
    fn get(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamPulse = std::ptr::null();
        picam_call!(sys::Picam_GetParameterPulseValue(
            handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees it points to
        // one valid `PicamPulse` when non-null
        let value = Pulse::from_raw(unsafe { &*ptr });

        picam_call!(sys::Picam_DestroyPulses(ptr))?;
        Ok(value)
    }

    fn set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()> {
        let raw = value.to_raw();
        picam_call!(sys::Picam_SetParameterPulseValue(
            handle,
            parameter.to_raw(),
            &raw
        ))
    }

    fn default_value(handle: sys::PicamHandle, parameter: Parameter) -> Result<Self> {
        let mut ptr: *const sys::PicamPulse = std::ptr::null();
        picam_call!(sys::Picam_GetParameterPulseDefaultValue(
            handle,
            parameter.to_raw(),
            &mut ptr
        ))?;

        if ptr.is_null() {
            return Err(crate::error::Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees it points to
        // one valid `PicamPulse` when non-null
        let value = Pulse::from_raw(unsafe { &*ptr });

        picam_call!(sys::Picam_DestroyPulses(ptr))?;
        Ok(value)
    }

    fn can_set(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<bool> {
        let raw = value.to_raw();
        let mut settable: sys::pibln = 0;
        picam_call!(sys::Picam_CanSetParameterPulseValue(
            handle,
            parameter.to_raw(),
            &raw,
            &mut settable
        ))?;
        Ok(settable != 0)
    }
}

impl OnlineSettablePicamValue for Pulse {
    fn set_online(handle: sys::PicamHandle, parameter: Parameter, value: Self) -> Result<()> {
        let raw = value.to_raw();
        picam_call!(sys::Picam_SetParameterPulseValueOnline(
            handle,
            parameter.to_raw(),
            &raw
        ))
    }
}
