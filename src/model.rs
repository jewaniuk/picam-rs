//! Hardware models, for cameras and accessories.

use std::fmt;

use crate::convert::enumeration_string;
use crate::error::Result;
use crate::library::Library;
use crate::sys;

/// A hardware model, as PICam identifies it: a camera, or an accessory such
/// as a lamp or laser (PICam manual 4.1).
///
/// Every model in the PICam SDK this crate was generated from (5.15) has a
/// constant here, named after PICam's own name for it, so
/// `PicamModel_Pixis100F` is [`Model::PIXIS_100F`]. Any other value, such as a
/// model added by a newer PICam runtime, is still a valid `Model`: it just
/// has no constant yet, and [`Model::name`] still works for it.
///
/// Some values are series: a family of models rather than a single model, which
/// PICam may use for older hardware whose exact model it doesn't know (PICam
/// manual 4.1). [`Model::is_series`] tells them apart.
///
/// ```no_run
/// use picam_rs::{Camera, Library, Model};
///
/// # fn main() -> picam_rs::Result<()> {
/// let library = Library::new()?;
/// for model in Camera::available_demo_models(&library)? {
///     if model == Model::PIXIS_100F {
///         println!("a PIXIS 100F demo camera is available");
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Model(i32);

/// Defines a constant for every model, plus `is_series` and the constant
/// names `Debug` uses. Invoked once, from the generated list.
macro_rules! define_models {
    (
        models {
            $($(#[$meta:meta])* $name:ident = $raw:ident,)*
        }
        series {
            $($series:ident,)*
        }
    ) => {
        impl Model {
            $(
                $(#[$meta])*
                pub const $name: Model = Model(sys::PicamModel::$raw.0);
            )*

            /// Reports whether this is a series: a family of models rather
            /// than a single model.
            ///
            /// This comes from this crate's own list, taken from the PICam
            /// SDK it was generated from (5.15), where a series is a model
            /// whose PICam name ends in `Series`. PICam has no call that
            /// answers it, so a model added by a newer runtime always gives
            /// `false`.
            pub fn is_series(self) -> bool {
                matches!(self, $(Model::$series)|*)
            }

            /// Returns the name of this model's constant, if it has one.
            fn constant_name(self) -> Option<&'static str> {
                match self {
                    $(Model::$name => Some(stringify!($name)),)*
                    _ => None,
                }
            }
        }
    };
}

include!("generated/models.rs");

impl Model {
    /// Converts a raw `PicamModel`. Any value is a valid `Model`.
    pub(crate) fn from_raw(raw: sys::PicamModel) -> Self {
        Model(raw.0)
    }

    /// Converts back to a raw `PicamModel`.
    pub(crate) fn to_raw(self) -> sys::PicamModel {
        sys::PicamModel(self.0)
    }

    /// Gets PICam's name for this model.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to name this model.
    pub fn name(self, library: &Library) -> Result<String> {
        enumeration_string(
            library,
            sys::PicamEnumeratedType::PicamEnumeratedType_Model,
            self.0,
        )
    }
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.constant_name() {
            Some(name) => write!(f, "Model::{name}"),
            None => write!(f, "Model({})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_code_round_trips() {
        for code in [-1, 0, 2, 1400, 3300, i32::MAX] {
            assert_eq!(Model::from_raw(sys::PicamModel(code)).to_raw().0, code);
        }
    }

    #[test]
    fn constants_match_picam() {
        assert_eq!(
            Model::PIXIS_100F.to_raw(),
            sys::PicamModel::PicamModel_Pixis100F
        );
        assert_eq!(
            Model::RETIGA_E7.to_raw(),
            sys::PicamModel::PicamModel_RetigaE7
        );
    }

    #[test]
    fn tells_series_apart() {
        assert!(Model::PIXIS_SERIES.is_series());
        assert!(Model::PIXIS_100_SERIES.is_series());
        assert!(!Model::PIXIS_100F.is_series());
        // 9999 isn't a model in sdk 5.15
        assert!(!Model::from_raw(sys::PicamModel(9999)).is_series());
    }

    #[test]
    fn debug_shows_the_constant_name() {
        assert_eq!(format!("{:?}", Model::PIXIS_100F), "Model::PIXIS_100F");
        assert_eq!(
            format!("{:?}", Model::from_raw(sys::PicamModel(9999))),
            "Model(9999)"
        );
    }
}
