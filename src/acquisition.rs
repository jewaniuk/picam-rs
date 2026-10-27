//! Acquiring data: [`Camera::acquire`] and [`Camera::start_acquisition`],
//! into PICam's memory or a buffer the camera keeps
//! ([`Camera::set_acquisition_buffer`]), and watching acquisition states
//! (`picam_advanced.h`, PICam manual chapter 6 and 7.4.14 to 7.4.17).

use std::alloc;
use std::borrow::Cow;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::camera::Camera;
use crate::convert::define_picam_enum;
use crate::enums::PixelFormat;
use crate::error::{Error, Result, picam_call};
use crate::parameter::{Roi, params};
use crate::sys;
use crate::watch::{self, Event, Watch};

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

define_picam_enum!(
    /// A point in each readout that [`Camera::watch_acquisition_state`] can
    /// report (PICam manual 7.1.4.1).
    AcquisitionState,
    sys::PicamAcquisitionState,
    PicamEnumeratedType_AcquisitionState,
    /// The camera has started reading out.
    ReadoutStarted = PicamAcquisitionState_ReadoutStarted,
    /// The camera has finished reading out.
    ReadoutEnded = PicamAcquisitionState_ReadoutEnded,
);

bitflags::bitflags! {
    /// What went wrong while watching acquisition states, from
    /// [`AcquisitionStateUpdate::errors`] (PICam manual 7.1.4.2).
    ///
    /// No flags set means nothing went wrong. Bits this crate doesn't
    /// recognize are kept, not dropped.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AcquisitionStateErrors: i32 {
        /// One or more state changes were missed.
        const LOST_COUNT =
            sys::PicamAcquisitionStateErrorsMask::PicamAcquisitionStateErrorsMask_LostCount.0;
    }
}

/// The camera reaching a state while acquiring, as
/// [`Camera::watch_acquisition_state`] sends it (PICam manual 7.3.7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcquisitionStateUpdate {
    /// The state the camera reached.
    pub state: AcquisitionState,
    /// How many times the camera has started reading out, among the states
    /// being watched (manual 7.2.3.2).
    pub readout_started_count: i64,
    /// How many times the camera has finished reading out, among the states
    /// being watched.
    pub readout_ended_count: i64,
    /// Whether any state changes were missed.
    pub errors: AcquisitionStateErrors,
}

/// How long ending an acquisition waits for each update, see
/// [`Acquisition::finish`].
const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// How many waits in a row may bring nothing before ending an acquisition
/// gives up, see [`Acquisition::finish`].
const DRAIN_ATTEMPTS: u32 = 10;

/// The longest ending an acquisition waits once it has asked it to stop,
/// even while data keeps arriving, see [`Acquisition::finish`].
const DRAIN_LIMIT: Duration = Duration::from_secs(10);

/// Memory the camera acquires into instead of PICam's own, from
/// [`Camera::set_acquisition_buffer`]. It's a raw allocation rather than a
/// `Box`, since PICam writes to parts of it while the crate holds views of
/// others, so nothing may claim the whole of it.
#[derive(Debug)]
pub(crate) struct AcquisitionBuffer {
    memory: NonNull<u8>,
    len: usize,
}

impl AcquisitionBuffer {
    /// The memory's alignment, enough for every pixel type, so pixels always
    /// borrow rather than copy.
    const ALIGN: usize = 8;

    /// Allocates `len` zeroed bytes, where `len` isn't 0.
    fn new(len: usize) -> Result<Self> {
        let layout = Self::layout(len)?;
        // SAFETY: `layout` isn't zero-sized, since `len` isn't 0
        let memory = unsafe { alloc::alloc_zeroed(layout) };
        let memory = NonNull::new(memory).ok_or(Error::InsufficientMemory)?;
        Ok(Self { memory, len })
    }

    /// The allocation's layout, or `InsufficientMemory` for a size no
    /// allocation can have.
    fn layout(len: usize) -> Result<alloc::Layout> {
        alloc::Layout::from_size_align(len, Self::ALIGN).map_err(|_| Error::InsufficientMemory)
    }

    /// The buffer as PICam takes it.
    fn to_raw(&self) -> sys::PicamAcquisitionBuffer {
        sys::PicamAcquisitionBuffer {
            memory: self.memory.as_ptr().cast(),
            memory_size: i64::try_from(self.len).expect("an allocation fits in an isize"),
        }
    }

    /// Reports whether the `len` bytes from `start` lie inside the buffer.
    fn contains(&self, start: *const u8, len: usize) -> bool {
        let base = self.memory.as_ptr().addr();
        let start = start.addr();
        start >= base
            && start
                .checked_add(len)
                .is_some_and(|end| end <= base + self.len)
    }
}

impl Drop for AcquisitionBuffer {
    fn drop(&mut self) {
        let layout = Self::layout(self.len).expect("checked when allocated");
        // SAFETY: `memory` was allocated in `new` with this layout, and only
        // this drop frees it
        unsafe { alloc::dealloc(self.memory.as_ptr(), layout) }
    }
}

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
        // camera, whose committed parameters `layout` was read from and whose
        // buffer, if any, is the one passed. picam keeps the data valid and
        // unchanged until the next acquisition starts or the camera closes
        // (manual 6.4.1), whether the data is in picam's memory or the
        // camera's buffer. those, and replacing the buffer, need `&mut self`
        // or ownership, and the returned `Readouts` keeps `self` mutably
        // borrowed
        unsafe { Readouts::from_raw(&available, layout, self.acquisition_buffer()) }
    }

    /// Starts acquiring in the background, stopping after
    /// [`params::READOUT_COUNT`](crate::params::READOUT_COUNT) readouts.
    ///
    /// Parameters must be committed first. Read the data with
    /// [`Acquisition::wait`] until an update says it's no longer running.
    /// The acquisition keeps the camera borrowed. Use
    /// [`Acquisition::camera`] for calls between updates.
    ///
    /// A readout count of 0 runs until stopped, and one above
    /// [`params::EXACT_READOUT_COUNT_MAXIMUM`](crate::params::EXACT_READOUT_COUNT_MAXIMUM)
    /// takes as many readouts as asked without the camera counting them
    /// exactly. Both need a buffer from [`Camera::set_acquisition_buffer`]
    /// (manual chapter 6).
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

    /// Gives the camera `len` bytes of memory to acquire into instead of
    /// PICam's own (PICam manual 7.4.14.2), or with `len` 0 goes back to
    /// PICam's, like [`Camera::clear_acquisition_buffer`].
    ///
    /// Acquiring until stopped (a readout count of 0) or more readouts than
    /// [`params::EXACT_READOUT_COUNT_MAXIMUM`](crate::params::EXACT_READOUT_COUNT_MAXIMUM)
    /// needs one (manual chapter 6). It must hold at least one readout,
    /// [`params::READOUT_STRIDE`](crate::params::READOUT_STRIDE) bytes. PICam
    /// fills it as a circular buffer, so room for several readouts leaves time
    /// to read each update before PICam needs its space again, and
    /// [`Camera::has_acquisition_buffer_overrun`] says whether it ran out. The
    /// camera keeps the memory until it's replaced or cleared, or the camera
    /// closes. Replacing it needs the camera free of any acquisition:
    ///
    /// ```compile_fail,E0499
    /// # fn f(camera: &mut picam_rs::Camera) -> picam_rs::Result<()> {
    /// let acquisition = camera.start_acquisition()?;
    /// // picam may be writing to the buffer, so it doesn't compile
    /// camera.set_acquisition_buffer(1 << 20)?;
    /// drop(acquisition);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`crate::Error::AcquisitionInProgress`] if PICam says an acquisition
    ///   is still running, after ending one gave up
    /// - [`crate::Error::InsufficientMemory`] if the memory can't be allocated
    /// - otherwise an error if PICam is unable to use it, and the previous
    ///   buffer stays in use
    pub fn set_acquisition_buffer(&mut self, len: usize) -> Result<()> {
        if len == 0 {
            return self.clear_acquisition_buffer();
        }
        self.refuse_while_acquiring()?;
        let buffer = AcquisitionBuffer::new(len)?;
        // the camera keeps `buffer` until picam has been given another or
        // none, or the camera has closed, so picam never writes to freed
        // memory
        picam_call!(sys::PicamAdvanced_SetAcquisitionBuffer(
            self.device_handle(),
            &buffer.to_raw()
        ))?;
        // picam has let go of the previous buffer, so it's freed here
        *self.acquisition_buffer_slot() = Some(buffer);
        Ok(())
    }

    /// Goes back to acquiring into PICam's own memory, freeing the camera's
    /// buffer if it has one (PICam manual 7.4.14.2).
    ///
    /// # Errors
    ///
    /// - [`crate::Error::AcquisitionInProgress`] if PICam says an acquisition
    ///   is still running, after ending one gave up
    /// - otherwise an error if PICam is unable to clear it, and the buffer
    ///   stays in use
    pub fn clear_acquisition_buffer(&mut self) -> Result<()> {
        if self.acquisition_buffer().is_none() {
            return Ok(());
        }
        self.refuse_while_acquiring()?;
        let none = sys::PicamAcquisitionBuffer {
            memory: ptr::null_mut(),
            memory_size: 0,
        };
        picam_call!(sys::PicamAdvanced_SetAcquisitionBuffer(
            self.device_handle(),
            &none
        ))?;
        *self.acquisition_buffer_slot() = None;
        Ok(())
    }

    /// How many bytes the camera's buffer holds, as PICam reports it, or 0
    /// when acquiring into PICam's own memory (PICam manual 7.4.14.1).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to report it.
    pub fn acquisition_buffer_len(&self) -> Result<usize> {
        let mut buffer = sys::PicamAcquisitionBuffer {
            memory: ptr::null_mut(),
            memory_size: 0,
        };
        picam_call!(sys::PicamAdvanced_GetAcquisitionBuffer(
            self.device_handle(),
            &mut buffer
        ))?;
        usize::try_from(buffer.memory_size).map_err(|_| Error::InvalidAcquisitionBuffer)
    }

    /// Reports whether the camera's buffer has overrun: whether, while
    /// acquiring, PICam needed its space for new data before the old was read
    /// (PICam manual 7.4.17.1).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn has_acquisition_buffer_overrun(&self) -> Result<bool> {
        let mut overran: sys::pibln = 0;
        picam_call!(sys::PicamAdvanced_HasAcquisitionBufferOverrun(
            self.device_handle(),
            &mut overran
        ))?;
        Ok(overran != 0)
    }

    /// Reports whether [`Acquisition::clear_readout_count_online`] can work
    /// on this camera (PICam manual 7.4.17.2).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_clear_readout_count_online(&self) -> Result<bool> {
        let mut clearable: sys::pibln = 0;
        picam_call!(sys::PicamAdvanced_CanClearReadoutCountOnline(
            self.device_handle(),
            &mut clearable
        ))?;
        Ok(clearable != 0)
    }

    /// Reports whether the camera can tell when it reaches `state` while
    /// acquiring, for [`Camera::watch_acquisition_state`] (PICam manual
    /// 7.4.16.1).
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn can_watch_acquisition_state(&self, state: AcquisitionState) -> Result<bool> {
        let mut detectable: sys::pibln = 0;
        picam_call!(sys::PicamAdvanced_CanRegisterForAcquisitionStateUpdated(
            self.device_handle(),
            state.to_raw(),
            &mut detectable
        ))?;
        Ok(detectable != 0)
    }

    /// Watches for the camera reaching `state` while acquiring, sending an
    /// update each time (PICam manual 7.4.16.2). Updates arrive from PICam's
    /// own thread, one at a time. Set the watch up before starting an
    /// acquisition, or during one through [`Acquisition::camera`]. Keep it as
    /// `let (_watch, updates) = ...`, as [`Watch`] explains.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to register its callback, for
    /// example because the camera can't detect `state`, which
    /// [`Camera::can_watch_acquisition_state`] checks.
    pub fn watch_acquisition_state(
        &self,
        state: AcquisitionState,
    ) -> Result<(Watch, Receiver<AcquisitionStateUpdate>)> {
        watch::subscribe_acquisition_state(self.device_handle(), state, |event| match event {
            Event::AcquisitionState(update) => Some(*update),
            _ => None,
        })
    }

    /// Refuses to change the buffer while PICam says an acquisition is
    /// running, which only happens after ending one gave up, since PICam may
    /// still be writing to it.
    fn refuse_while_acquiring(&self) -> Result<()> {
        let mut running: sys::pibln = 0;
        picam_call!(sys::Picam_IsAcquisitionRunning(
            self.raw_handle(),
            &mut running
        ))?;
        if running != 0 {
            return Err(Error::AcquisitionInProgress);
        }
        Ok(())
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
/// after 10 waits in a row bring no data, each waiting up to 1 second, or
/// after 10 seconds in all, rather than hang on a camera that has stopped
/// responding or keeps sending data. In that case PICam may still think
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
        // acquisition started and whose buffer, if any, is the one passed.
        // picam keeps the data valid and unchanged until the next wait
        // (manual 6.4.5), whether the data is in picam's memory or the
        // camera's buffer. picam is assumed not to write over readouts it has
        // handed out before then, even when a circular buffer overruns: seen
        // on a demo camera, and listed to check on hardware. the next wait
        // needs `&mut self`, as do `stop` and `finish`, replacing the buffer
        // needs `&mut Camera`, which the acquisition holds, and the returned
        // `Update` keeps `self` mutably borrowed
        let readouts = unsafe {
            Readouts::from_raw(
                &available,
                Arc::clone(&self.layout),
                self.camera.acquisition_buffer(),
            )
        }?;
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

    /// Tries to make the acquisition run until stopped, by setting its
    /// readout count to 0 while it runs (PICam manual 7.4.17.3). Returns
    /// whether it did: it can lose a race with the acquisition reaching its
    /// readout count. An acquisition that runs until stopped needs a buffer
    /// from [`Camera::set_acquisition_buffer`]. Check
    /// [`Camera::can_clear_readout_count_online`] first.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to try.
    pub fn clear_readout_count_online(&mut self) -> Result<bool> {
        let mut cleared: sys::pibln = 0;
        picam_call!(sys::PicamAdvanced_ClearReadoutCountOnline(
            self.camera.device_handle(),
            &mut cleared
        ))?;
        Ok(cleared != 0)
    }

    /// Stops the acquisition if it's still running, and waits for it to end.
    ///
    /// # Errors
    ///
    /// Returns the first error met while stopping or waiting, or
    /// [`crate::Error::TimeOutOccurred`] if it still hadn't ended after 10
    /// waits in a row brought no data, each waiting up to 1 second, or after
    /// 10 seconds in all.
    pub fn finish(mut self) -> Result<()> {
        self.drain()
    }

    /// Stops the acquisition and waits for it to end, see `finish`.
    fn drain(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        let started = Instant::now();
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
            // give up rather than hang on a camera that has stopped
            // responding, or one that keeps sending data after a stop failed
            if self.running && (failures >= DRAIN_ATTEMPTS || started.elapsed() >= DRAIN_LIMIT) {
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
    /// Views `available`'s readouts, which must lie inside `buffer` when the
    /// camera has one.
    ///
    /// # Safety
    ///
    /// `available` must come from a successful `Picam_Acquire` or
    /// `Picam_WaitForAcquisitionUpdate` on a camera whose committed
    /// parameters `layout` was read from, and whose buffer, if any, is
    /// `buffer`. Its data must stay valid and unchanged for `'a`.
    unsafe fn from_raw(
        available: &sys::PicamAvailableData,
        layout: Arc<Layout>,
        buffer: Option<&AcquisitionBuffer>,
    ) -> Result<Self> {
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
        let start = available.initial_readout.cast::<u8>().cast_const();
        // picam reports readouts that sit one after another (manual 6.3.1).
        // checking they lie inside the camera's buffer turns any
        // misunderstanding of how it wraps into an error rather than a read
        // out of bounds
        if buffer.is_some_and(|buffer| !buffer.contains(start, len)) {
            return Err(Error::InconsistentDataLayout);
        }
        // SAFETY: picam puts `count` readouts at `initial_readout`, each
        // `readout_stride` bytes after the last and laid out as `layout`
        // describes (manual 6.1 and 6.3.1), so the `len` bytes from there are
        // its data. the caller guarantees they stay valid and unchanged for
        // `'a`
        let bytes = unsafe { slice::from_raw_parts(start, len) };
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
    fn from_raw_views_picam_data() {
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        fill(&mut storage[start..start + 72]);
        let data = |initial_readout: *mut u8, readout_count| sys::PicamAvailableData {
            initial_readout: initial_readout.cast(),
            readout_count,
        };
        let pointer = storage[start..].as_mut_ptr();

        // SAFETY: `pointer` starts 72 bytes laid out as `test_layout`
        // describes two readouts, and `storage` outlives `readouts`
        let readouts =
            unsafe { Readouts::from_raw(&data(pointer, 2), Arc::new(test_layout()), None) }
                .expect("two readouts should be viewed");
        // two readouts, without padding after the last frame
        assert_eq!(readouts.bytes().len(), 72);
        assert_eq!(readouts.len(), 2);
        assert_eq!(readouts.bytes().as_ptr(), pointer.cast_const());

        // SAFETY: neither pointer is read, since a null pointer or no
        // readouts give an empty view
        let empty = [
            unsafe { Readouts::from_raw(&data(ptr::null_mut(), 2), Arc::new(test_layout()), None) },
            unsafe { Readouts::from_raw(&data(pointer, 0), Arc::new(test_layout()), None) },
        ];
        for readouts in empty {
            let readouts = readouts.expect("no data should be an empty view");
            assert!(readouts.is_empty());
            assert!(readouts.bytes().is_empty());
        }

        // SAFETY: a negative count is refused before the pointer is used
        let negative =
            unsafe { Readouts::from_raw(&data(pointer, -1), Arc::new(test_layout()), None) };
        assert!(matches!(negative, Err(Error::InconsistentDataLayout)));
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
            // a region whose size overflows
            Layout::new(
                i32::MAX,
                1,
                i32::MAX,
                i32::MAX,
                mono16,
                i32::MAX - 7,
                vec![roi(i32::MAX, i32::MAX, 1, 1)],
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

    #[test]
    fn acquisition_state_round_trips() {
        use crate::convert::assert_round_trips;
        assert_round_trips!(AcquisitionState, sys::PicamAcquisitionState, 2);
    }

    #[test]
    fn acquisition_state_errors_keep_unknown_bits() {
        let errors = |raw: sys::PicamAcquisitionStateErrorsMask| {
            AcquisitionStateErrors::from_bits_retain(raw.0)
        };
        assert!(
            errors(sys::PicamAcquisitionStateErrorsMask::PicamAcquisitionStateErrorsMask_None)
                .is_empty()
        );
        assert_eq!(
            errors(sys::PicamAcquisitionStateErrorsMask::PicamAcquisitionStateErrorsMask_LostCount),
            AcquisitionStateErrors::LOST_COUNT
        );
        let unknown = 0x100;
        assert_eq!(
            errors(sys::PicamAcquisitionStateErrorsMask(unknown)).bits(),
            unknown
        );
    }

    #[test]
    fn buffer_holds_only_its_own_bytes() {
        let buffer = AcquisitionBuffer::new(64).expect("64 bytes should be allocated");
        let base = buffer.memory.as_ptr().cast_const();
        assert!(base.cast::<u64>().is_aligned());
        assert!(buffer.contains(base, 64));
        assert!(!buffer.contains(base, 65));
        assert!(!buffer.contains(base.wrapping_add(1), 64));
        assert!(!buffer.contains(base.wrapping_sub(1), 1));
        assert_eq!(buffer.to_raw().memory_size, 64);

        // SAFETY: the buffer's 64 bytes were allocated zeroed, and nothing
        // else writes to them while this view lives
        let bytes = unsafe { slice::from_raw_parts(base, 64) };
        assert!(bytes.iter().all(|&byte| byte == 0));

        assert!(matches!(
            AcquisitionBuffer::new(usize::MAX),
            Err(Error::InsufficientMemory)
        ));
    }

    #[test]
    fn readouts_outside_the_buffer_are_refused() {
        // two readouts in `test_layout`'s shape take 72 bytes
        let available = |buffer: &AcquisitionBuffer| sys::PicamAvailableData {
            initial_readout: buffer.memory.as_ptr().cast(),
            readout_count: 2,
        };
        let short = AcquisitionBuffer::new(60).expect("60 bytes should be allocated");
        // SAFETY: the readouts don't fit in `short`, so they're refused
        // before the pointer is used
        let refused = unsafe {
            Readouts::from_raw(&available(&short), Arc::new(test_layout()), Some(&short))
        };
        assert!(matches!(refused, Err(Error::InconsistentDataLayout)));

        let exact = AcquisitionBuffer::new(72).expect("72 bytes should be allocated");
        // SAFETY: `exact` holds 72 zeroed bytes, enough for two readouts, and
        // outlives `readouts`
        let readouts = unsafe {
            Readouts::from_raw(&available(&exact), Arc::new(test_layout()), Some(&exact))
        }
        .expect("two readouts should fit");
        assert_eq!(readouts.len(), 2);
        assert_eq!(readouts.bytes().len(), 72);
    }
}
