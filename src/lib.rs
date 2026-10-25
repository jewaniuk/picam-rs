//! A safe Rust wrapper around the Teledyne Princeton Instruments PICam C API,
//! which controls their scientific cameras.
//!
//! **This crate is experimental.** Much of it has not been tested against
//! real hardware, and its API may change.
//!
//! It only runs on Windows, and needs the PICam SDK installed: the build
//! reads its headers and links `Picam.lib`, either from `PICAM_DIR` or from
//! the SDK's default install path. No SDK files are shipped with this crate.
//!
//! Everything starts with a [`Library`], which initializes PICam and keeps it
//! initialized for as long as it or any [`Camera`] opened with it is alive:
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use picam_rs::{Camera, Library, params};
//!
//! # fn main() -> picam_rs::Result<()> {
//! let library = Arc::new(Library::new()?);
//! let camera = Camera::open_first(library)?;
//! let exposure_time = camera.get(params::EXPOSURE_TIME)?;
//! println!("exposure time: {exposure_time}");
//! # Ok(())
//! # }
//! ```

mod camera;
mod constraint;
mod convert;
mod error;
mod library;
mod model;
mod parameter;
mod sys;

pub use camera::{Camera, CameraHandle, CameraId};
pub use constraint::{
    CollectionConstraint, ConstraintCategory, ConstraintScope, ConstraintSeverity, ConstraintType,
    ModulationsConstraint, PulseConstraint, RangeConstraint, RoisConstraint, RoisConstraintRules,
};
pub use convert::FirmwareDetail;
pub use error::{Error, Result};
pub use library::Library;
pub use model::Model;
pub use parameter::{
    ComputerInterface, EnumeratedType, Modulation, OnlineSettablePicamValue, Parameter, PicamValue,
    Pulse, ReadablePicamValue, Roi, TypedParameter, ValueAccess, ValueType, params,
};
