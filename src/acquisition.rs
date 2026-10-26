//! Acquiring data: [`Camera::acquire`] and [`Camera::start_acquisition`].

use std::borrow::Cow;
use std::ptr;
use std::slice;
use std::sync::Arc;
use std::time::Duration;

use crate::camera::Camera;
use crate::enums::PixelFormat;
use crate::error::{Error, Result, picam_call};
use crate::parameter::{Roi, params};
use crate::sys;

bitflags::bitflags! {
    /// What went wrong during an acquisition, from
    /// [`Error::AcquisitionFailed`] (PICam manual 6.2.1.1).
    ///
    /// No flags set means nothing went wrong. Bits this crate doesn't
    /// recognize are kept, not dropped.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AcquisitionErrors: i32 {
        /// Some data was lost.
        const DATA_LOST =
            sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_DataLost.0;
        /// The camera was disconnected.
        const CONNECTION_LOST =
            sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_ConnectionLost.0;
        /// Data stopped arriving from the camera.
        const DATA_NOT_ARRIVING =
            sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_DataNotArriving.0;
        /// A connected shutter overheated, and is disabled until it cools down.
        const SHUTTER_OVERHEATED =
            sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_ShutterOverheated.0;
        /// The camera has failed, and needs servicing before it can acquire
        /// again.
        const CAMERA_FAULTED =
            sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_CameraFaulted.0;
    }
}

/// How long ending an acquisition waits for each update, see
/// [`Acquisition::finish`].
const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// How many waits in a row may bring nothing before ending an acquisition
/// gives up, see [`Acquisition::finish`].
const DRAIN_ATTEMPTS: u32 = 10;

impl Camera {
    /// Acquires `readout_count` readouts, returning once all of them have
    /// arrived.
    ///
    /// Parameters must be committed first. `readout_timeout` is the longest
    /// to wait between readouts, and `None` waits forever. PICam reuses its
    /// buffer for the next acquisition (manual 6.4.1), so the data borrows
    /// the camera: copy it, for example with `readouts.bytes().to_vec()`, to
    /// keep it longer.
    ///
    /// ```compile_fail,E0499
    /// # fn f(camera: &mut picam_rs::Camera) -> picam_rs::Result<()> {
    /// let first = camera.acquire(1, None)?;
    /// // the next acquisition would overwrite `first`, so it doesn't compile
    /// let second = camera.acquire(1, None)?;
    /// drop(first);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ParametersNotCommitted`] if parameters have changed
    ///   since the last commit
    /// - [`crate::Error::TimeOutOccurred`] if a readout took longer than
    ///   `readout_timeout`, which halts the acquisition
    /// - [`crate::Error::AcquisitionFailed`] if PICam reported errors, in
    ///   which case any data is dropped
    /// - [`crate::Error::InconsistentDataLayout`] if the camera's layout
    ///   parameters don't fit together
    /// - otherwise an error if PICam is unable to acquire
    pub fn acquire(
        &mut self,
        readout_count: i64,
        readout_timeout: Option<Duration>,
    ) -> Result<Readouts<'_>> {
        let layout = Arc::new(Layout::read(self)?);
        let mut available = sys::PicamAvailableData {
            initial_readout: ptr::null_mut(),
            readout_count: 0,
        };
        let mut errors = sys::PicamAcquisitionErrorsMask(0);
        picam_call!(sys::Picam_Acquire(
            self.raw_handle(),
            readout_count,
            timeout_ms(readout_timeout),
            &mut available,
            &mut errors
        ))?;
        check_errors(errors)?;
        // SAFETY: `available` came from a successful `Picam_Acquire` on this
        // camera, whose committed parameters `layout` was read from. picam
        // keeps the data valid and unchanged until the next acquisition
        // starts or the camera closes (manual 6.4.1). both need `&mut self`
        // or ownership, and the returned `Readouts` keeps `self` mutably
        // borrowed
        unsafe { Readouts::from_raw(&available, layout) }
    }

    /// Starts acquiring in the background, stopping after
    /// [`params::READOUT_COUNT`](crate::params::READOUT_COUNT) readouts.
    ///
    /// Parameters must be committed first. Read the data with
    /// [`Acquisition::wait`] until an update says it's no longer running.
    /// The acquisition keeps the camera borrowed. Use
    /// [`Acquisition::camera`] for calls between updates.
    ///
    /// A readout count of 0, or more than the camera can count exactly,
    /// needs a buffer of your own, which this crate doesn't support yet
    /// (manual chapter 6), so PICam refuses it.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ParametersNotCommitted`] if parameters have changed
    ///   since the last commit
    /// - [`crate::Error::InconsistentDataLayout`] if the camera's layout
    ///   parameters don't fit together
    /// - otherwise an error if PICam is unable to start
    pub fn start_acquisition(&mut self) -> Result<Acquisition<'_>> {
        let layout = Arc::new(Layout::read(self)?);
        picam_call!(sys::Picam_StartAcquisition(self.raw_handle()))?;
        Ok(Acquisition {
            camera: self,
            layout,
            running: true,
        })
    }
}

/// An acquisition running in the background, from
/// [`Camera::start_acquisition`].
///
/// Call [`wait`](Self::wait) until an update says the acquisition is no
/// longer running. PICam requires this even after an error or a stop (manual
/// 6.4.5).
///
/// [`finish`](Self::finish) stops it early and waits for it to end.
/// Dropping it does the same, but can only log errors. Either one gives up
/// after 10 waits of 1 second in a row bring nothing, rather than hang on a
/// camera that has stopped responding. In that case PICam may still think
/// the acquisition is running, and the next one fails with
/// [`crate::Error::AcquisitionInProgress`].
///
/// ```no_run
/// # fn f(camera: &mut picam_rs::Camera) -> picam_rs::Result<()> {
/// use std::time::Duration;
///
/// let mut acquisition = camera.start_acquisition()?;
/// loop {
///     let Some(update) = acquisition.wait(Some(Duration::from_secs(5)))? else {
///         continue; // nothing yet, the acquisition is still running
///     };
///     for readout in update.readouts().iter() {
///         for frame in readout.frames() {
///             // frame.rois(), frame.metadata()
///         }
///     }
///     if !update.running() {
///         break;
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Acquisition<'a> {
    camera: &'a mut Camera,
    layout: Arc<Layout>,
    // whether picam last said the acquisition is running
    running: bool,
}

impl Acquisition<'_> {
    /// Waits up to `timeout` for new data or a change of status, where
    /// `None` waits forever.
    ///
    /// Returns `Ok(None)` if nothing happened in time, and the acquisition
    /// carries on (manual 6.4.5). Each update's data lasts until the next
    /// call.
    ///
    /// ```compile_fail,E0499
    /// # fn f(acquisition: &mut picam_rs::Acquisition<'_>) -> picam_rs::Result<()> {
    /// let first = acquisition.wait(None)?;
    /// // the next wait would invalidate `first`, so it doesn't compile
    /// let second = acquisition.wait(None)?;
    /// drop(first);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`crate::Error::AcquisitionFailed`] if PICam reported errors, which
    ///   halt the acquisition. Keep waiting, or call
    ///   [`finish`](Self::finish), until it has ended.
    /// - [`crate::Error::InconsistentDataLayout`] if the data doesn't fit the
    ///   layout read when the acquisition started
    /// - otherwise an error if PICam is unable to wait
    pub fn wait(&mut self, timeout: Option<Duration>) -> Result<Option<Update<'_>>> {
        let mut available = sys::PicamAvailableData {
            initial_readout: ptr::null_mut(),
            readout_count: 0,
        };
        let mut status = sys::PicamAcquisitionStatus {
            running: 0,
            errors: sys::PicamAcquisitionErrorsMask(0),
            readout_rate: 0.0,
        };
        match picam_call!(sys::Picam_WaitForAcquisitionUpdate(
            self.camera.raw_handle(),
            timeout_ms(timeout),
            &mut available,
            &mut status
        )) {
            // still running, and `available` and `status` mean nothing
            // (manual 6.4.5)
            Err(Error::TimeOutOccurred) => return Ok(None),
            result => result?,
        }
        self.running = status.running != 0;
        check_errors(status.errors)?;
        // SAFETY: `available` came from a successful wait on this camera,
        // whose committed parameters `layout` was read from when the
        // acquisition started. picam keeps the data valid and unchanged until
        // the next wait (manual 6.4.5). that needs `&mut self`, as do `stop`
        // and `finish`, and the returned `Update` keeps `self` mutably
        // borrowed
        let readouts = unsafe { Readouts::from_raw(&available, Arc::clone(&self.layout)) }?;
        Ok(Some(Update {
            readouts,
            running: self.running,
            readout_rate: status.readout_rate,
        }))
    }

    /// Asks PICam to stop the acquisition, which happens some time later.
    ///
    /// Keep calling [`wait`](Self::wait) until an update says it's no longer
    /// running, or call [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to stop it.
    pub fn stop(&mut self) -> Result<()> {
        picam_call!(sys::Picam_StopAcquisition(self.camera.raw_handle()))
    }

    /// Reports whether PICam says the acquisition is running.
    ///
    /// Use [`wait`](Self::wait), not this, to wait for data (manual 6.4.4).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn is_running(&self) -> Result<bool> {
        let mut running: sys::pibln = 0;
        picam_call!(sys::Picam_IsAcquisitionRunning(
            self.camera.raw_handle(),
            &mut running
        ))?;
        Ok(running != 0)
    }

    /// The camera, for calls between updates such as [`Camera::read`] or
    /// [`Camera::set_online`].
    pub fn camera(&self) -> &Camera {
        self.camera
    }

    /// Stops the acquisition if it's still running, and waits for it to end.
    ///
    /// # Errors
    ///
    /// Returns the first error met while stopping or waiting, or
    /// [`crate::Error::TimeOutOccurred`] if it still hadn't ended after 10
    /// waits of 1 second in a row brought nothing.
    pub fn finish(mut self) -> Result<()> {
        self.drain()
    }

    /// Stops the acquisition and waits for it to end, see `finish`.
    fn drain(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        let mut first_error = match self.stop() {
            // it may have ended without being waited on yet, which the
            // waits below still take care of
            Err(Error::AcquisitionNotInProgress) => None,
            result => result.err(),
        };
        let mut failures = 0;
        while self.running {
            match self.wait(Some(DRAIN_TIMEOUT)) {
                Ok(Some(_)) => failures = 0,
                Ok(None) => failures += 1,
                Err(e) => {
                    first_error.get_or_insert(e);
                    failures += 1;
                }
            }
            if self.running && failures >= DRAIN_ATTEMPTS {
                // give up rather than hang on a camera that has stopped
                // responding
                self.running = false;
                return Err(first_error.unwrap_or(Error::TimeOutOccurred));
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

impl Drop for Acquisition<'_> {
    fn drop(&mut self) {
        // nowhere to hand a `Result` back to from `Drop`, so just report it
        if let Err(e) = self.drain() {
            tracing::error!("failed to end acquisition: {e}");
        }
    }
}

/// What one [`Acquisition::wait`] brought: new data and the acquisition's
/// status.
#[derive(Debug)]
pub struct Update<'a> {
    readouts: Readouts<'a>,
    running: bool,
    readout_rate: f64,
}

impl<'a> Update<'a> {
    /// The readouts that arrived. There may be none if only the status
    /// changed.
    pub fn readouts(&self) -> &Readouts<'a> {
        &self.readouts
    }

    /// Whether the acquisition is still running. Once it's false, the
    /// acquisition has ended and there's no need to wait again.
    pub fn running(&self) -> bool {
        self.running
    }

    /// How fast readouts are arriving, in readouts per second, when more
    /// than one is acquired (manual 6.3.2).
    pub fn readout_rate(&self) -> f64 {
        self.readout_rate
    }
}

/// Acquired readouts, borrowed from PICam's buffer.
///
/// Each readout holds one or more frames, and each frame holds the pixels of
/// every region of interest followed by its metadata (manual 6.1).
#[derive(Debug)]
pub struct Readouts<'a> {
    // every readout, from the first byte of the first to the last byte of
    // the last, without padding after it
    bytes: &'a [u8],
    count: usize,
    layout: Arc<Layout>,
}

impl<'a> Readouts<'a> {
    /// Views `available`'s readouts.
    ///
    /// # Safety
    ///
    /// `available` must come from a successful `Picam_Acquire` or
    /// `Picam_WaitForAcquisitionUpdate` on a camera whose committed
    /// parameters `layout` was read from, and its data must stay valid and
    /// unchanged for `'a`.
    unsafe fn from_raw(available: &sys::PicamAvailableData, layout: Arc<Layout>) -> Result<Self> {
        let count =
            usize::try_from(available.readout_count).map_err(|_| Error::InconsistentDataLayout)?;
        if count == 0 || available.initial_readout.is_null() {
            return Ok(Self {
                bytes: &[],
                count: 0,
                layout,
            });
        }
        let len = (count - 1)
            .checked_mul(layout.readout_stride)
            .and_then(|start| start.checked_add(layout.readout_len))
            .ok_or(Error::InconsistentDataLayout)?;
        // SAFETY: picam puts `count` readouts at `initial_readout`, each
        // `readout_stride` bytes after the last and laid out as `layout`
        // describes (manual 6.1 and 6.3.1), so the `len` bytes from there are
        // its data. the caller guarantees they stay valid and unchanged for
        // `'a`
        let bytes = unsafe {
            slice::from_raw_parts(available.initial_readout.cast::<u8>().cast_const(), len)
        };
        Ok(Self {
            bytes,
            count,
            layout,
        })
    }

    /// How many readouts there are.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Reports whether there are no readouts.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Every readout's bytes as one block, including any padding between
    /// readouts.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The readout at `index`, or `None` if there isn't one.
    pub fn get(&self, index: usize) -> Option<Readout<'_>> {
        (index < self.count).then(|| self.readout(index))
    }

    /// Every readout, in order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Readout<'_>> {
        (0..self.count).map(|index| self.readout(index))
    }

    // `index` must be below `count`, and `from_raw` checked the bytes cover it
    fn readout(&self, index: usize) -> Readout<'_> {
        let start = index * self.layout.readout_stride;
        Readout {
            bytes: &self.bytes[start..start + self.layout.readout_len],
            layout: &self.layout,
        }
    }
}

/// One readout: one or more frames.
#[derive(Debug, Clone, Copy)]
pub struct Readout<'a> {
    bytes: &'a [u8],
    layout: &'a Layout,
}

impl<'a> Readout<'a> {
    /// The readout's bytes, from its first frame to the end of its last,
    /// without padding after it.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// How many frames the readout holds.
    pub fn frame_count(&self) -> usize {
        self.layout.frames_per_readout
    }

    /// The frame at `index`, or `None` if there isn't one.
    pub fn frame(&self, index: usize) -> Option<Frame<'a>> {
        (index < self.layout.frames_per_readout).then(|| self.frame_at(index))
    }

    /// Every frame, in order.
    pub fn frames(&self) -> impl ExactSizeIterator<Item = Frame<'a>> + 'a {
        let readout = *self;
        (0..self.layout.frames_per_readout).map(move |index| readout.frame_at(index))
    }

    // `index` must be below `frames_per_readout`, and `Layout::new` checked
    // the readout covers it
    fn frame_at(&self, index: usize) -> Frame<'a> {
        let start = index * self.layout.frame_stride;
        Frame {
            // a frame is its whole stride, pixels then metadata
            bytes: &self.bytes[start..start + self.layout.frame_stride],
            layout: self.layout,
        }
    }
}

/// One frame: the pixels of every region of interest, then the frame's
/// metadata.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    bytes: &'a [u8],
    layout: &'a Layout,
}

impl<'a> Frame<'a> {
    /// The frame's bytes, its pixels and metadata.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// How many regions of interest the frame holds.
    pub fn roi_count(&self) -> usize {
        self.layout.rois.len()
    }

    /// The data of the region of interest at `index`, in the order the
    /// regions were set, or `None` if there isn't one.
    pub fn roi(&self, index: usize) -> Option<RoiData<'a>> {
        self.layout.rois.get(index).map(|roi| self.roi_data(roi))
    }

    /// The data of every region of interest, in the order they were set.
    pub fn rois(&self) -> impl ExactSizeIterator<Item = RoiData<'a>> + 'a {
        let frame = *self;
        frame.layout.rois.iter().map(move |roi| frame.roi_data(roi))
    }

    /// The frame's metadata, the bytes after its pixels (`FrameSize`) up to
    /// the next frame (`FrameStride`), not decoded. It holds whichever of
    /// time stamps, frame tracking, gate tracking and modulation tracking
    /// are turned on (manual 6.1).
    pub fn metadata(&self) -> &'a [u8] {
        &self.bytes[self.layout.frame_size..]
    }

    // `Layout::new` checked the frame covers every region
    fn roi_data(&self, roi: &'a RoiLayout) -> RoiData<'a> {
        RoiData {
            bytes: &self.bytes[roi.offset..roi.offset + roi.len],
            roi,
            pixel_format: self.layout.pixel_format,
            pixel_size: self.layout.pixel_size,
        }
    }
}

/// One region of interest's pixels in one frame.
#[derive(Debug, Clone, Copy)]
pub struct RoiData<'a> {
    bytes: &'a [u8],
    roi: &'a RoiLayout,
    pixel_format: PixelFormat,
    pixel_size: usize,
}

impl<'a> RoiData<'a> {
    /// The region of interest, as set when the acquisition started.
    pub fn roi(&self) -> &'a Roi {
        &self.roi.roi
    }

    /// How many pixels wide the data is, after binning.
    pub fn width(&self) -> usize {
        self.roi.width
    }

    /// How many pixels high the data is, after binning.
    pub fn height(&self) -> usize {
        self.roi.height
    }

    /// The region's pixels as bytes.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The region's pixels, in the order PICam stores them, or `None` for a
    /// pixel format this crate doesn't know.
    ///
    /// They're borrowed from PICam's buffer when it's aligned for the pixel
    /// type, and copied when it isn't, since PICam doesn't document its
    /// alignment.
    pub fn pixels(&self) -> Option<Pixels<'a>> {
        match (self.pixel_format, self.pixel_size) {
            (PixelFormat::Monochrome16Bit, 2) => Some(Pixels::Mono16(as_u16s(self.bytes))),
            (PixelFormat::Monochrome32Bit, 4) => Some(Pixels::Mono32(as_u32s(self.bytes))),
            _ => None,
        }
    }
}

/// One region of interest's pixels, [`RoiData::width`] ×
/// [`RoiData::height`] of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pixels<'a> {
    /// 16-bit monochrome pixels ([`PixelFormat::Monochrome16Bit`]).
    Mono16(Cow<'a, [u16]>),
    /// 32-bit monochrome pixels ([`PixelFormat::Monochrome32Bit`]).
    Mono32(Cow<'a, [u32]>),
}

/// Defines a function reading bytes as pixels of one type.
macro_rules! pixels_from_bytes {
    ($name:ident, $ty:ty) => {
        /// Reads `bytes` as pixels, borrowing them when they're aligned and
        /// copying them otherwise.
        fn $name(bytes: &[u8]) -> Cow<'_, [$ty]> {
            // SAFETY: every bit pattern is a valid pixel, and `align_to` only
            // puts whole, correctly aligned pixels in `middle`
            let (prefix, middle, suffix) = unsafe { bytes.align_to::<$ty>() };
            if prefix.is_empty() && suffix.is_empty() {
                Cow::Borrowed(middle)
            } else {
                // picam doesn't document its alignment, so copy rather than fail
                Cow::Owned(
                    bytes
                        .chunks_exact(size_of::<$ty>())
                        .map(|pixel| {
                            <$ty>::from_ne_bytes(pixel.try_into().expect("chunks are exact"))
                        })
                        .collect(),
                )
            }
        }
    };
}

pixels_from_bytes!(as_u16s, u16);
pixels_from_bytes!(as_u32s, u32);

/// Where acquired data sits in PICam's buffer (manual 6.1), from the
/// camera's committed parameters.
#[derive(Debug)]
struct Layout {
    readout_stride: usize,
    // a readout's length without padding: up to the end of its last frame's
    // stride, metadata included
    readout_len: usize,
    frames_per_readout: usize,
    // a frame's pixels and metadata
    frame_stride: usize,
    // a frame's pixels alone, metadata starts here
    frame_size: usize,
    pixel_format: PixelFormat,
    // bytes per pixel
    pixel_size: usize,
    rois: Vec<RoiLayout>,
}

/// Where one region of interest's pixels sit in a frame.
#[derive(Debug)]
struct RoiLayout {
    roi: Roi,
    // after binning
    width: usize,
    height: usize,
    offset: usize,
    len: usize,
}

impl Layout {
    /// Reads the layout from `camera`'s parameters, which must be committed,
    /// since those are what picam acquires with.
    fn read(camera: &Camera) -> Result<Self> {
        if !camera.are_parameters_committed()? {
            return Err(Error::ParametersNotCommitted);
        }
        Self::new(
            camera.get(params::READOUT_STRIDE)?,
            camera.get(params::FRAMES_PER_READOUT)?,
            camera.get(params::FRAME_STRIDE)?,
            camera.get(params::FRAME_SIZE)?,
            camera.get(params::PIXEL_FORMAT)?,
            camera.get(params::PIXEL_BIT_DEPTH)?,
            camera.get(params::ROIS)?,
        )
    }

    /// Checks the parts fit together, so the views never slice out of
    /// bounds.
    fn new(
        readout_stride: i32,
        frames_per_readout: i32,
        frame_stride: i32,
        frame_size: i32,
        pixel_format: PixelFormat,
        pixel_bit_depth: i32,
        rois: Vec<Roi>,
    ) -> Result<Self> {
        let size = |value: i32| usize::try_from(value).map_err(|_| Error::InconsistentDataLayout);
        let readout_stride = size(readout_stride)?;
        let frames_per_readout = size(frames_per_readout)?;
        let frame_stride = size(frame_stride)?;
        let frame_size = size(frame_size)?;
        let pixel_bit_depth = size(pixel_bit_depth)?;
        if pixel_bit_depth == 0 || pixel_bit_depth % 8 != 0 || frames_per_readout == 0 {
            return Err(Error::InconsistentDataLayout);
        }
        let pixel_size = pixel_bit_depth / 8;

        // each region holds its binned pixels, one after another (manual 6.1)
        let mut offset = 0;
        let mut roi_layouts = Vec::with_capacity(rois.len());
        for roi in rois {
            let width = size(roi.width)?
                .checked_div(size(roi.x_binning)?)
                .ok_or(Error::InconsistentDataLayout)?;
            let height = size(roi.height)?
                .checked_div(size(roi.y_binning)?)
                .ok_or(Error::InconsistentDataLayout)?;
            let len = width
                .checked_mul(height)
                .and_then(|pixels| pixels.checked_mul(pixel_size))
                .ok_or(Error::InconsistentDataLayout)?;
            roi_layouts.push(RoiLayout {
                roi,
                width,
                height,
                offset,
                len,
            });
            offset = offset
                .checked_add(len)
                .ok_or(Error::InconsistentDataLayout)?;
        }

        // a readout runs to the end of its last frame's stride, metadata
        // included (`FrameSize` covers only the pixels, seen with sdk 5.15)
        let readout_len = frames_per_readout
            .checked_mul(frame_stride)
            .ok_or(Error::InconsistentDataLayout)?;
        if offset > frame_size || frame_size > frame_stride || readout_len > readout_stride {
            return Err(Error::InconsistentDataLayout);
        }
        Ok(Self {
            readout_stride,
            readout_len,
            frames_per_readout,
            frame_stride,
            frame_size,
            pixel_format,
            pixel_size,
            rois: roi_layouts,
        })
    }
}

/// Converts a timeout into PICam's milliseconds, rounding up, where -1 waits
/// forever (manual 6.4.1).
fn timeout_ms(timeout: Option<Duration>) -> i32 {
    match timeout {
        None => -1,
        // beyond picam's limit, about 24 days, waits as long as it can
        Some(timeout) => i32::try_from(timeout.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX),
    }
}

/// Turns acquisition errors PICam reported into an `Err`.
fn check_errors(errors: sys::PicamAcquisitionErrorsMask) -> Result<()> {
    let errors = AcquisitionErrors::from_bits_retain(errors.0);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::AcquisitionFailed(errors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A region of `width` × `height` pixels at the sensor's corner.
    fn roi(width: i32, height: i32, x_binning: i32, y_binning: i32) -> Roi {
        Roi {
            x: 0,
            width,
            x_binning,
            y: 0,
            height,
            y_binning,
        }
    }

    /// Two 16-bit regions: 2 × 1 pixels, and 4 × 2 binned to 2 × 2. Their
    /// pixels take 12 bytes, the frame size, and 4 bytes of metadata after
    /// them make a 16-byte frame stride. Two frames take 32 bytes, padded to
    /// 40 per readout.
    fn test_layout() -> Layout {
        Layout::new(
            40,
            2,
            16,
            12,
            PixelFormat::Monochrome16Bit,
            16,
            vec![roi(2, 1, 1, 1), roi(4, 2, 2, 1)],
        )
        .expect("the test layout fits together")
    }

    /// The test pixel value for readout `r`, frame `f`, region `k`, pixel
    /// `p`.
    fn pixel(r: usize, f: usize, k: usize, p: usize) -> u16 {
        (r * 1000 + f * 100 + k * 10 + p) as u16
    }

    /// Fills `bytes` with two readouts in `test_layout`'s shape: the test
    /// pixels, metadata `[r, f, 0xaa, 0xbb]` and padding `0xee`.
    fn fill(bytes: &mut [u8]) {
        bytes.fill(0xee);
        for r in 0..2 {
            for f in 0..2 {
                let frame = r * 40 + f * 16;
                let mut offset = frame;
                for (k, pixels) in [2, 4].into_iter().enumerate() {
                    for p in 0..pixels {
                        bytes[offset..offset + 2].copy_from_slice(&pixel(r, f, k, p).to_ne_bytes());
                        offset += 2;
                    }
                }
                bytes[offset..offset + 4].copy_from_slice(&[r as u8, f as u8, 0xaa, 0xbb]);
            }
        }
    }

    #[test]
    fn timeouts_round_up_to_milliseconds() {
        assert_eq!(timeout_ms(None), -1);
        assert_eq!(timeout_ms(Some(Duration::ZERO)), 0);
        assert_eq!(timeout_ms(Some(Duration::from_micros(1500))), 2);
        assert_eq!(timeout_ms(Some(Duration::from_secs(5))), 5000);
        assert_eq!(
            timeout_ms(Some(Duration::from_secs(40 * 24 * 60 * 60))),
            i32::MAX
        );
    }

    #[test]
    fn views_slice_the_layout() {
        // two readouts without the second's padding, at an aligned start
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        let bytes = &mut storage[start..start + 72];
        fill(bytes);
        let readouts = Readouts {
            bytes,
            count: 2,
            layout: Arc::new(test_layout()),
        };

        assert_eq!(readouts.len(), 2);
        assert!(!readouts.is_empty());
        assert!(readouts.get(2).is_none());
        assert_eq!(readouts.iter().len(), 2);
        for (r, readout) in readouts.iter().enumerate() {
            // no padding after the last frame's stride
            assert_eq!(readout.bytes().len(), 32);
            assert_eq!(readout.frame_count(), 2);
            assert!(readout.frame(2).is_none());
            for (f, frame) in readout.frames().enumerate() {
                assert_eq!(frame.bytes().len(), 16);
                assert_eq!(frame.metadata(), &[r as u8, f as u8, 0xaa, 0xbb]);
                assert_eq!(frame.roi_count(), 2);
                assert!(frame.roi(2).is_none());
                let sizes: Vec<(usize, usize)> = frame
                    .rois()
                    .map(|data| (data.width(), data.height()))
                    .collect();
                assert_eq!(sizes, [(2, 1), (2, 2)]);
                for (k, data) in frame.rois().enumerate() {
                    assert_eq!(data.bytes().len(), data.width() * data.height() * 2);
                    let expected: Vec<u16> = (0..data.width() * data.height())
                        .map(|p| pixel(r, f, k, p))
                        .collect();
                    let Some(Pixels::Mono16(pixels)) = data.pixels() else {
                        panic!("16-bit pixels should be read as u16s");
                    };
                    assert!(
                        matches!(pixels, Cow::Borrowed(_)),
                        "aligned pixels are borrowed"
                    );
                    assert_eq!(*pixels, *expected);
                }
            }
        }
        // the second readout starts one stride in
        assert_eq!(readouts.bytes().len(), 72);
        assert_eq!(
            readouts.get(1).map(|readout| readout.bytes()),
            Some(&readouts.bytes()[40..72])
        );
    }

    #[test]
    fn misaligned_pixels_are_copied() {
        let values: [u16; 4] = [1, 0x0203, 0xfffe, 7];
        let mut storage = [0u8; 8 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        // one byte past an aligned start, so never aligned for u16
        let misaligned = start + 1;
        for (i, value) in values.iter().enumerate() {
            storage[misaligned + 2 * i..misaligned + 2 * i + 2]
                .copy_from_slice(&value.to_ne_bytes());
        }

        let copied = as_u16s(&storage[misaligned..misaligned + 8]);
        assert!(
            matches!(copied, Cow::Owned(_)),
            "misaligned pixels are copied"
        );
        assert_eq!(*copied, values);

        storage.copy_within(misaligned..misaligned + 8, start);
        let borrowed = as_u16s(&storage[start..start + 8]);
        assert!(
            matches!(borrowed, Cow::Borrowed(_)),
            "aligned pixels are borrowed"
        );
        assert_eq!(*borrowed, values);

        let wide = as_u32s(&storage[start..start + 8]);
        assert_eq!(wide.len(), 2);
    }

    #[test]
    fn inconsistent_layouts_are_refused() {
        let mono16 = PixelFormat::Monochrome16Bit;
        let cases = [
            // a region bigger than the frame
            Layout::new(40, 1, 20, 16, mono16, 16, vec![roi(10, 1, 1, 1)]),
            // a frame bigger than its stride
            Layout::new(40, 1, 20, 24, mono16, 16, vec![roi(2, 1, 1, 1)]),
            // frames that overrun the readout stride
            Layout::new(40, 3, 20, 16, mono16, 16, vec![roi(2, 1, 1, 1)]),
            // zero binning
            Layout::new(40, 1, 20, 16, mono16, 16, vec![roi(2, 1, 0, 1)]),
            // a 12-bit depth
            Layout::new(40, 1, 20, 16, mono16, 12, vec![roi(2, 1, 1, 1)]),
            // a negative stride
            Layout::new(-40, 1, 20, 16, mono16, 16, vec![roi(2, 1, 1, 1)]),
            // zero frames per readout
            Layout::new(40, 0, 20, 16, mono16, 16, vec![roi(2, 1, 1, 1)]),
            // an overflowing size
            Layout::new(
                i32::MAX,
                i32::MAX,
                i32::MAX,
                i32::MAX,
                mono16,
                16,
                vec![roi(2, 1, 1, 1)],
            ),
        ];
        for (index, case) in cases.into_iter().enumerate() {
            assert!(
                matches!(case, Err(Error::InconsistentDataLayout)),
                "case {index} should be refused, got {case:?}"
            );
        }
        assert!(Layout::new(40, 2, 20, 16, mono16, 16, vec![roi(2, 1, 1, 1)]).is_ok());
    }

    #[test]
    fn acquisition_errors_become_err() {
        assert_eq!(
            check_errors(sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_None),
            Ok(())
        );
        assert_eq!(
            check_errors(sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_DataLost),
            Err(Error::AcquisitionFailed(AcquisitionErrors::DATA_LOST))
        );
        let unknown = 0x100;
        assert_eq!(
            check_errors(sys::PicamAcquisitionErrorsMask(unknown)),
            Err(Error::AcquisitionFailed(
                AcquisitionErrors::from_bits_retain(unknown)
            ))
        );
    }
}
