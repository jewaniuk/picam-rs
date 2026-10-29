//! Sensor defect maps: the defective pixels, rows and columns, and the "star"
//! defects, a camera knows about (`picam_advanced.h`, PICam manual 7.2.1 and
//! 7.4.5).

use crate::camera::Camera;
use crate::convert::collect_raw_array;
use crate::error::{Error, Result, free_on_error, picam_call};
use crate::sys;

/// A pixel's location on the sensor, counting columns (`x`) and rows (`y`)
/// from zero (PICam manual 7.2.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelLocation {
    /// The column.
    pub x: i16,
    /// The row.
    pub y: i16,
}

impl PixelLocation {
    fn from_raw(raw: &sys::PicamPixelLocation) -> Self {
        Self { x: raw.x, y: raw.y }
    }
}

/// A defective column on the sensor (PICam manual 7.2.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColumnDefect {
    /// The top-most defective pixel.
    pub start: PixelLocation,
    /// How many rows the defect spans.
    pub height: i32,
}

/// A defective row on the sensor (PICam manual 7.2.1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowDefect {
    /// The left-most defective pixel.
    pub start: PixelLocation,
    /// How many columns the defect spans.
    pub width: i32,
}

/// Every defective column, row and single pixel on a camera's sensor (PICam
/// manual 7.2.1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelDefectMap {
    /// The defective columns.
    pub column_defects: Vec<ColumnDefect>,
    /// The defective rows.
    pub row_defects: Vec<RowDefect>,
    /// The defective single pixels.
    pub point_defects: Vec<PixelLocation>,
}

/// A "star" defect: a pixel that leaks much of its charge into the pixels
/// around it (PICam manual 7.2.1.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StarDefect {
    /// The leaking pixel at the star's center.
    pub center: PixelLocation,
    /// The star's pixel values with no light.
    pub bias: f32,
    /// The fraction of an adjacent pixel's value that leaked from the center.
    pub adjacent_factor: f32,
    /// The fraction of a diagonal pixel's value that leaked from the center.
    pub diagonal_factor: f32,
}

/// The star defects that apply to one set of camera parameters (PICam manual
/// 7.2.1.6).
#[derive(Debug, Clone, PartialEq)]
pub struct StarDefectMap {
    /// The map's ID, which
    /// [`params::APPLICABLE_STAR_DEFECT_MAP_ID`](crate::params::APPLICABLE_STAR_DEFECT_MAP_ID)
    /// names when it's the one that applies.
    pub id: i32,
    /// The star defects.
    pub defects: Vec<StarDefect>,
}

impl StarDefectMap {
    fn from_raw(raw: &sys::PicamStarDefectMap) -> Self {
        Self {
            id: raw.id,
            // SAFETY: PICam guarantees `star_defect_array` has
            // `star_defect_count` valid, contiguous entries whenever this
            // struct is valid
            defects: unsafe {
                collect_raw_array(raw.star_defect_array, raw.star_defect_count, |star| {
                    StarDefect {
                        center: PixelLocation::from_raw(&star.center),
                        bias: star.bias,
                        adjacent_factor: star.adjacent_factor,
                        diagonal_factor: star.diagonal_factor,
                    }
                })
            },
        }
    }
}

impl Camera {
    /// Gets the map of the sensor's defective columns, rows and pixels
    /// (PICam manual 7.4.5.2).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no map, or an error if PICam is unable to get it.
    pub fn pixel_defect_map(&self) -> Result<PixelDefectMap> {
        let mut ptr: *const sys::PicamPixelDefectMap = std::ptr::null();
        let result = picam_call!(sys::PicamAdvanced_GetPixelDefectMap(
            self.raw_handle(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::PicamAdvanced_DestroyPixelDefectMaps)?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamPixelDefectMap`, whose arrays each have their count of valid,
        // contiguous entries (manual 7.2.1.4, see `collect_raw_array`)
        let map = unsafe {
            let raw = &*ptr;
            PixelDefectMap {
                column_defects: collect_raw_array(
                    raw.column_defect_array,
                    raw.column_defect_count,
                    |defect| ColumnDefect {
                        start: PixelLocation::from_raw(&defect.start),
                        height: defect.height,
                    },
                ),
                row_defects: collect_raw_array(
                    raw.row_defect_array,
                    raw.row_defect_count,
                    |defect| RowDefect {
                        start: PixelLocation::from_raw(&defect.start),
                        width: defect.width,
                    },
                ),
                point_defects: collect_raw_array(
                    raw.point_defect_array,
                    raw.point_defect_count,
                    PixelLocation::from_raw,
                ),
            }
        };
        picam_call!(sys::PicamAdvanced_DestroyPixelDefectMaps(ptr))?;
        Ok(map)
    }

    /// Gets the star defect map that applies to the camera's current
    /// parameters (PICam manual 7.4.5.4).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::UnexpectedNullPointer`] if PICam reports
    /// success but returns no map, or an error if PICam is unable to get it.
    pub fn star_defect_map(&self) -> Result<StarDefectMap> {
        let mut ptr: *const sys::PicamStarDefectMap = std::ptr::null();
        let result = picam_call!(sys::PicamAdvanced_GetStarDefectMap(
            self.raw_handle(),
            &mut ptr
        ));
        free_on_error!(result, ptr, sys::PicamAdvanced_DestroyStarDefectMaps)?;

        if ptr.is_null() {
            return Err(Error::UnexpectedNullPointer);
        }

        // SAFETY: just checked `ptr` is non-null; PICam guarantees a valid
        // `PicamStarDefectMap`.
        let map = StarDefectMap::from_raw(unsafe { &*ptr });
        picam_call!(sys::PicamAdvanced_DestroyStarDefectMaps(ptr))?;
        Ok(map)
    }

    /// Gets every star defect map the camera has, one per set of parameters
    /// they apply to (PICam manual 7.4.5.5).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to get them.
    pub fn star_defect_maps(&self) -> Result<Vec<StarDefectMap>> {
        let mut array: *const sys::PicamStarDefectMap = std::ptr::null();
        let mut count: sys::piint = 0;
        let result = picam_call!(sys::PicamAdvanced_GetStarDefectMaps(
            self.raw_handle(),
            &mut array,
            &mut count
        ));
        free_on_error!(result, array, sys::PicamAdvanced_DestroyStarDefectMaps)?;

        // SAFETY: see `collect_raw_array`; PICam guarantees this when `array` is non-null
        let maps = unsafe { collect_raw_array(array, count, StarDefectMap::from_raw) };

        if !array.is_null() {
            picam_call!(sys::PicamAdvanced_DestroyStarDefectMaps(array))?;
        }
        Ok(maps)
    }
}
