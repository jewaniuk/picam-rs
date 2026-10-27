//! Acquiring data: [`Camera::acquire`], [`Camera::start_acquisition`] and
//! [`Camera::start_acquisition_stream`], into PICam's memory or a buffer the
//! camera keeps ([`Camera::set_acquisition_buffer`]), and watching
//! acquisition states (`picam_advanced.h`, PICam manual chapter 6 and 7.4.14
//! to 7.4.17).

use std::borrow::Cow;
use std::fmt;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
};

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
/// [`Camera::set_acquisition_buffer`].
///
/// PICam writes to it circularly, and once it runs out of room it writes over
/// data not yet read, possibly while the crate is copying that data (seen on
/// a demo camera with SDK 5.15). So the OS allocates it, not Rust's
/// allocator, and the crate never makes a reference to it or reads it except
/// with volatile reads through raw pointers ([`AcquisitionBuffer::copy_out`]).
/// That keeps it outside every Rust allocation, where the `read_volatile`
/// docs leave such reads to the hardware: one racing PICam's writes returns
/// wrong bytes, not undefined behaviour, and the overrun check catches them.
pub(crate) struct AcquisitionBuffer {
    memory: NonNull<u8>,
    len: usize,
    // where data is copied out of `memory`, reused across copies and grown as
    // needed. `u64`s keep it 8-byte aligned, so pixels borrow rather than copy
    copy: Vec<u64>,
}

impl AcquisitionBuffer {
    /// Allocates `len` zeroed bytes from the OS. Panics if `len` is 0, which
    /// callers clear the buffer for instead.
    fn new(len: usize) -> Result<Self> {
        assert!(len != 0, "an acquisition buffer can't be empty");
        // SAFETY: a null address lets the os choose where, and `len` isn't 0.
        // the os returns null or `len` committed, zeroed, page-aligned bytes
        let memory =
            unsafe { VirtualAlloc(ptr::null(), len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        let memory = NonNull::new(memory.cast()).ok_or(Error::InsufficientMemory)?;
        Ok(Self {
            memory,
            len,
            copy: Vec::new(),
        })
    }

    /// The buffer as PICam takes it.
    fn to_raw(&self) -> sys::PicamAcquisitionBuffer {
        sys::PicamAcquisitionBuffer {
            memory: self.memory.as_ptr().cast(),
            memory_size: i64::try_from(self.len).expect("an allocation fits in an isize"),
        }
    }

    /// Where the buffer lies.
    fn bounds(&self) -> BufferBounds {
        BufferBounds {
            base: self.memory.as_ptr().addr(),
            len: self.len,
        }
    }

    /// Reports whether the `len` bytes from `start` lie inside the buffer.
    fn contains(&self, start: *const u8, len: usize) -> bool {
        self.bounds().contains(start, len)
    }

    /// Copies the `len` bytes from `start` out of the buffer with volatile
    /// reads, refusing any that don't lie inside it.
    ///
    /// PICam may be writing to them, if it has overrun. A racing write gives
    /// wrong bytes rather than undefined behaviour (see [`AcquisitionBuffer`]),
    /// so check the overrun flag before trusting the copy.
    fn copy_out(&mut self, start: *const u8, len: usize) -> Result<&[u8]> {
        // picam reports readouts that sit one after another (manual 6.3.1).
        // checking they lie inside the buffer turns any misunderstanding of
        // how it wraps into an error rather than a read out of bounds
        if !self.contains(start, len) {
            return Err(Error::InconsistentDataLayout);
        }
        let words = len.div_ceil(8);
        if self.copy.len() < words {
            self.copy.resize(words, 0);
        }
        let copy = as_bytes_mut(&mut self.copy, len);
        // SAFETY: the bytes lie inside the buffer (checked above), which the
        // os allocated in `new` and keeps allocated while `self` is borrowed,
        // and which nothing references or reads but volatilely
        unsafe { volatile_copy(start, copy) };
        Ok(copy)
    }
}

/// Where the camera's buffer lies, as addresses, so a streaming
/// acquisition's callback can check copies against it from PICam's thread.
#[derive(Debug, Clone, Copy)]
struct BufferBounds {
    base: usize,
    len: usize,
}

impl BufferBounds {
    /// Reports whether the `len` bytes from `start` lie inside the buffer.
    fn contains(&self, start: *const u8, len: usize) -> bool {
        let start = start.addr();
        start >= self.base
            && start
                .checked_add(len)
                .is_some_and(|end| end <= self.base + self.len)
    }
}

impl fmt::Debug for AcquisitionBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the copy's length is enough, its contents can run to the buffer's
        // size
        f.debug_struct("AcquisitionBuffer")
            .field("memory", &self.memory)
            .field("len", &self.len)
            .field("copy_len", &(self.copy.len() * 8))
            .finish()
    }
}

impl Drop for AcquisitionBuffer {
    fn drop(&mut self) {
        // SAFETY: `memory` came from `VirtualAlloc` in `new`, and only this
        // drop frees it. releasing takes a size of 0
        if unsafe { VirtualFree(self.memory.as_ptr().cast(), 0, MEM_RELEASE) } == 0 {
            tracing::error!("failed to free an acquisition buffer");
        }
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
    /// keep it longer. With a buffer from [`Camera::set_acquisition_buffer`],
    /// the readouts are a copy of it. PICam leaves
    /// [`params::READOUT_COUNT`](crate::params::READOUT_COUNT) at
    /// `readout_count` afterwards, so set it again before
    /// [`Camera::start_acquisition`] (seen with SDK 5.15).
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
    ///   which case any data is dropped, or with
    ///   [`AcquisitionErrors::DATA_LOST`] if PICam wrote over the camera's
    ///   buffer while it was copied
    /// - [`crate::Error::InconsistentDataLayout`] if the camera's layout
    ///   parameters don't fit together, or the data doesn't lie inside the
    ///   camera's buffer
    /// - [`crate::Error::InvalidAcquisitionBuffer`] if the camera's buffer
    ///   can't hold `readout_count` readouts (seen with SDK 5.15)
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
        let device = self.device_handle();
        match self.acquisition_buffer_slot() {
            Some(buffer) => copy_from_buffer(buffer, device, &available, layout),
            // SAFETY: `available` came from a successful `Picam_Acquire` on
            // this camera, whose committed parameters `layout` was read from,
            // into picam's own memory, since the camera has no buffer. it
            // acquires exactly `readout_count` readouts, even after the
            // readout count was cleared online (seen on a demo camera with
            // sdk 5.15), so that memory doesn't wrap. picam keeps the data
            // valid and unchanged until the next acquisition starts or the
            // camera closes (manual 6.4.1). those need `&mut
            // self` or ownership, and the returned `Readouts` keeps `self`
            // mutably borrowed
            None => unsafe { Readouts::from_raw(&available, layout) },
        }
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
    /// - [`crate::Error::InvalidAcquisitionBuffer`] if the readout count needs
    ///   a buffer and the camera has none, or the camera's buffer is too
    ///   small (seen with SDK 5.15)
    /// - [`crate::Error::AcquisitionUpdatedHandlerRegistered`] if an
    ///   [`AcquisitionStream`] on this camera was leaked with
    ///   [`std::mem::forget`]
    /// - otherwise an error if PICam is unable to start
    pub fn start_acquisition(&mut self) -> Result<Acquisition<'_>> {
        self.refuse_while_streaming()?;
        let layout = Arc::new(Layout::read(self)?);
        picam_call!(sys::Picam_StartAcquisition(self.raw_handle()))?;
        Ok(Acquisition {
            camera: self,
            layout,
            running: true,
        })
    }

    /// Starts acquiring in the background, with PICam sending readouts
    /// through a callback as they arrive, rather than through
    /// [`Acquisition::wait`] (PICam manual 7.4.15).
    ///
    /// Returns the acquisition and a receiver of its updates. Each update is
    /// a copy, so the receiver is `Send`: the data can be processed on
    /// another thread while this one holds the acquisition, to stop or
    /// [`finish`](AcquisitionStream::finish) it. PICam sends one update per
    /// readout (seen with SDK 5.15), and a last one once the acquisition has
    /// ended: with no readouts, or an `Err` if it ended with errors. The
    /// receiver then disconnects, so iterating over it ends with the
    /// acquisition. Updates nobody reads stay in memory.
    ///
    /// Parameters, the readout count and buffers work as for
    /// [`Camera::start_acquisition`]. Data from the camera's buffer is copied
    /// with the same check for PICam writing over it.
    ///
    /// ```no_run
    /// # fn f(camera: &mut picam_rs::Camera) -> picam_rs::Result<()> {
    /// use std::thread;
    ///
    /// let (stream, updates) = camera.start_acquisition_stream()?;
    /// let worker = thread::spawn(move || -> picam_rs::Result<usize> {
    ///     let mut readouts = 0;
    ///     for update in updates {
    ///         readouts += update?.readouts().len();
    ///     }
    ///     Ok(readouts)
    /// });
    /// // this thread could stop the acquisition early instead
    /// stream.finish()?;
    /// let readouts = worker.join().expect("the worker shouldn't panic")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The stream keeps the camera borrowed:
    ///
    /// ```compile_fail,E0499
    /// # fn f(camera: &mut picam_rs::Camera) -> picam_rs::Result<()> {
    /// let (stream, updates) = camera.start_acquisition_stream()?;
    /// // picam would refuse to acquire while the stream runs, so it doesn't
    /// // compile
    /// let readouts = camera.acquire(1, None)?;
    /// drop(stream);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - [`crate::Error::ParametersNotCommitted`] if parameters have changed
    ///   since the last commit
    /// - [`crate::Error::InconsistentDataLayout`] if the camera's layout
    ///   parameters don't fit together
    /// - [`crate::Error::InvalidAcquisitionBuffer`] if the readout count
    ///   needs a buffer and the camera has none, or the camera's buffer is
    ///   too small (seen with SDK 5.15)
    /// - [`crate::Error::AcquisitionUpdatedHandlerRegistered`] if an
    ///   [`AcquisitionStream`] on this camera was leaked with
    ///   [`std::mem::forget`]
    /// - otherwise an error if PICam is unable to start
    ///
    /// An update is an `Err` with
    /// - [`crate::Error::AcquisitionFailed`] if PICam reported errors, which
    ///   end the acquisition, or with [`AcquisitionErrors::DATA_LOST`] if
    ///   PICam wrote over the camera's buffer while it was copied
    /// - [`crate::Error::InconsistentDataLayout`] if the data doesn't lie
    ///   inside the camera's buffer
    /// - an error if PICam was unable to report whether the buffer overran
    pub fn start_acquisition_stream(
        &mut self,
    ) -> Result<(AcquisitionStream<'_>, Receiver<Result<StreamedUpdate>>)> {
        self.refuse_while_streaming()?;
        let layout = Arc::new(Layout::read(self)?);
        let (updates_sender, updates) = mpsc::channel();
        let (ended_sender, ended) = mpsc::channel();
        let sink = Mutex::new(StreamSink {
            layout,
            buffer: self.acquisition_buffer().map(AcquisitionBuffer::bounds),
            senders: Some((updates_sender, ended_sender)),
            pending: None,
        });
        // registered before starting, so no readout is missed, on the device
        // handle, the only one picam takes (seen with sdk 5.15)
        let watch = watch::subscribe_acquisition_updated(self.device_handle(), move |event| {
            sink.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .handle(event);
        })?;
        // if starting fails, dropping `watch` unregisters the callback
        picam_call!(sys::Picam_StartAcquisition(self.raw_handle()))?;
        Ok((
            AcquisitionStream {
                camera: self,
                ended,
                running: true,
                _watch: watch,
            },
            updates,
        ))
    }

    /// Refuses to start an acquisition, or to change the buffer, while an
    /// acquisition-updated callback is registered on the camera, which only
    /// a leaked [`AcquisitionStream`] leaves behind. It would take the next
    /// acquisition's data, copying it with a layout that may no longer hold,
    /// and its last callbacks may still read the buffer after PICam says the
    /// acquisition has ended.
    fn refuse_while_streaming(&self) -> Result<()> {
        if watch::is_acquisition_updated_registered(self.device_handle()) {
            return Err(Error::AcquisitionUpdatedHandlerRegistered);
        }
        Ok(())
    }

    /// Gives the camera `len` bytes of memory from the OS to acquire into
    /// instead of PICam's own (PICam manual 7.4.14.2), or with `len` 0 goes
    /// back to PICam's, like [`Camera::clear_acquisition_buffer`].
    ///
    /// Acquiring until stopped (a readout count of 0) or more readouts than
    /// [`params::EXACT_READOUT_COUNT_MAXIMUM`](crate::params::EXACT_READOUT_COUNT_MAXIMUM)
    /// needs one (manual chapter 6). `acquire(n)` needs room for `n` readouts,
    /// and an acquisition that runs until stopped needs room for at least 2,
    /// or they fail with [`crate::Error::InvalidAcquisitionBuffer`] (seen with
    /// SDK 5.15). Read [`params::READOUT_STRIDE`](crate::params::READOUT_STRIDE)
    /// after setting the other parameters, since metadata such as time stamps
    /// lengthens it. Data from the buffer, by [`Camera::acquire`],
    /// [`Acquisition::wait`] or [`Camera::start_acquisition_stream`], is
    /// always a copy, so the crate never views
    /// memory PICam may be writing to. The camera keeps that copy, up to
    /// `len` bytes, until the buffer is replaced or cleared. PICam fills the
    /// buffer circularly, so room for several readouts leaves time to read
    /// each update before PICam needs its space again, and
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
    ///   is still running, after ending one gave up or an [`Acquisition`] or
    ///   [`AcquisitionStream`] was leaked with [`std::mem::forget`]
    /// - [`crate::Error::AcquisitionUpdatedHandlerRegistered`] if an
    ///   [`AcquisitionStream`] on this camera was leaked, whose callback may
    ///   still read the buffer until the camera closes
    /// - [`crate::Error::InsufficientMemory`] if the memory can't be allocated
    /// - otherwise an error if PICam is unable to use it, and the previous
    ///   buffer stays in use
    pub fn set_acquisition_buffer(&mut self, len: usize) -> Result<()> {
        if len == 0 {
            return self.clear_acquisition_buffer();
        }
        self.refuse_while_acquiring()?;
        self.refuse_while_streaming()?;
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
    /// - [`crate::Error::AcquisitionUpdatedHandlerRegistered`] if an
    ///   [`AcquisitionStream`] on this camera was leaked with
    ///   [`std::mem::forget`], whose callback may still read the buffer until
    ///   the camera closes
    /// - otherwise an error if PICam is unable to clear it, and the buffer
    ///   stays in use
    pub fn clear_acquisition_buffer(&mut self) -> Result<()> {
        if self.acquisition_buffer().is_none() {
            return Ok(());
        }
        self.refuse_while_acquiring()?;
        self.refuse_while_streaming()?;
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
    /// (PICam manual 7.4.17.1). The flag clears when the next acquisition
    /// starts.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to check this.
    pub fn has_acquisition_buffer_overrun(&self) -> Result<bool> {
        has_overrun(self.device_handle())
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
    /// Returns an error if PICam is unable to register its callback. PICam
    /// accepts a watch even for a state the camera says it can't detect (seen
    /// with SDK 5.15), and what it sends then isn't documented, so check
    /// [`Camera::can_watch_acquisition_state`] first.
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
    /// When the camera has a buffer, each update is a copy, since PICam writes
    /// over readouts not yet read once it runs out of room. If it did, `wait`
    /// returns [`crate::Error::AcquisitionFailed`] with
    /// [`AcquisitionErrors::DATA_LOST`] rather than data that may be torn.
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
    ///   halt the acquisition, or with [`AcquisitionErrors::DATA_LOST`] if
    ///   PICam wrote over the camera's buffer before it was read. Keep
    ///   waiting, or call [`finish`](Self::finish), until it has ended.
    /// - [`crate::Error::InconsistentDataLayout`] if the data doesn't fit the
    ///   layout read when the acquisition started, or doesn't lie inside the
    ///   camera's buffer
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
        let device = self.camera.device_handle();
        let layout = Arc::clone(&self.layout);
        let readouts = match self.camera.acquisition_buffer_slot() {
            Some(buffer) => copy_from_buffer(buffer, device, &available, layout)?,
            // SAFETY: `available` came from a successful wait on this camera,
            // whose committed parameters `layout` was read from when the
            // acquisition started, into picam's own memory, since the camera
            // has no buffer. picam keeps the data valid and unchanged until
            // the next wait (manual 6.4.5). that memory never becomes
            // circular: clearing the readout count online needs a buffer, and
            // once one has cleared it, picam refuses to start without a
            // buffer until parameters are committed again (seen on a demo
            // camera with sdk 5.15). the next wait needs `&mut self`, as do
            // `stop` and `finish`, and the returned `Update` keeps `self`
            // mutably borrowed
            None => unsafe { Readouts::from_raw(&available, layout) }?,
        };
        Ok(Some(Update {
            readouts,
            running: status.running != 0,
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
    /// readout count. It needs a buffer from
    /// [`Camera::set_acquisition_buffer`], which PICam then fills
    /// circularly. The camera keeps the cleared count for later acquisitions
    /// too, even though [`params::READOUT_COUNT`](crate::params::READOUT_COUNT)
    /// still reads the old count and the parameters read as committed, until
    /// [`Camera::commit_parameters`] runs again (seen with SDK 5.15). Check
    /// [`Camera::can_clear_readout_count_online`] first.
    ///
    /// # Errors
    ///
    /// - [`crate::Error::InvalidAcquisitionBuffer`] if the camera has no
    ///   buffer
    /// - otherwise an error if PICam is unable to try
    pub fn clear_readout_count_online(&mut self) -> Result<bool> {
        // without a buffer, picam reuses its own memory circularly (seen on a
        // demo camera with sdk 5.15), which `wait` views in place
        if self.camera.acquisition_buffer().is_none() {
            return Err(Error::InvalidAcquisitionBuffer);
        }
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

/// An acquisition whose readouts PICam sends through a callback rather than
/// [`Acquisition::wait`], from [`Camera::start_acquisition_stream`] (PICam
/// manual 7.4.15).
///
/// It keeps the camera borrowed, like [`Acquisition`]. Use
/// [`AcquisitionStream::camera`] for calls while it runs.
/// [`finish`](Self::finish) stops it and waits for it to end. Dropping it does
/// the same, but can only log errors. Either one then unregisters the
/// callback, so the receiver disconnects. They give up 10 seconds after
/// asking it to stop, rather than hang on a camera that has stopped
/// responding. In that case PICam may still think the acquisition is
/// running, and the next acquisition or buffer change fails with
/// [`crate::Error::AcquisitionInProgress`].
#[derive(Debug)]
pub struct AcquisitionStream<'a> {
    camera: &'a mut Camera,
    // disconnects once picam has reported the acquisition ended, since the
    // callback's sink then drops its senders. nothing is ever sent
    ended: Receiver<()>,
    // whether the end hasn't been seen yet
    running: bool,
    // unregisters the callback, dropped after `Drop` has ended the
    // acquisition
    _watch: Watch,
}

impl AcquisitionStream<'_> {
    /// Asks PICam to stop the acquisition, which happens some time later. The
    /// receiver disconnects once it has.
    ///
    /// # Errors
    ///
    /// Returns an error if PICam is unable to stop it.
    pub fn stop(&mut self) -> Result<()> {
        picam_call!(sys::Picam_StopAcquisition(self.camera.raw_handle()))
    }

    /// Reports whether PICam says the acquisition is running.
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

    /// The camera, for calls while the acquisition runs such as
    /// [`Camera::read`] or [`Camera::set_online`].
    pub fn camera(&self) -> &Camera {
        self.camera
    }

    /// Stops the acquisition if it's still running, and waits for it to end.
    ///
    /// # Errors
    ///
    /// Returns the error met while stopping, or
    /// [`crate::Error::TimeOutOccurred`] if it still hadn't ended 10 seconds
    /// after being asked to stop.
    pub fn finish(mut self) -> Result<()> {
        self.end()
    }

    /// Stops the acquisition and waits for it to end, see `finish`.
    fn end(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        self.running = false;
        // ended already, by itself or after an error
        if let Err(TryRecvError::Disconnected) = self.ended.try_recv() {
            return Ok(());
        }
        let stopped = match self.stop() {
            // it ended between the check and the stop
            Err(Error::AcquisitionNotInProgress) => Ok(()),
            result => result,
        };
        // give up rather than hang on a camera that has stopped responding
        if let Err(RecvTimeoutError::Timeout) = self.ended.recv_timeout(DRAIN_LIMIT) {
            return Err(stopped.err().unwrap_or(Error::TimeOutOccurred));
        }
        stopped
    }
}

impl Drop for AcquisitionStream<'_> {
    fn drop(&mut self) {
        // nowhere to hand a `Result` back to from `Drop`, so just report it
        if let Err(e) = self.end() {
            tracing::error!("failed to end acquisition stream: {e}");
        }
    }
}

/// One update from an [`AcquisitionStream`]: a copy of the readouts that
/// arrived, and the acquisition's status.
///
/// It owns its data, so it can be kept, or sent to another thread.
pub struct StreamedUpdate {
    // the copy, 8-byte aligned so pixels borrow rather than copy
    words: Vec<u64>,
    // how many bytes of `words` hold readouts
    len: usize,
    count: usize,
    layout: Arc<Layout>,
    running: bool,
    readout_rate: f64,
}

impl StreamedUpdate {
    /// The readouts that arrived. There are none in the last update, which
    /// only says the acquisition has ended.
    pub fn readouts(&self) -> Readouts<'_> {
        Readouts::from_copy(
            as_bytes(&self.words, self.len),
            self.count,
            Arc::clone(&self.layout),
        )
    }

    /// Whether the acquisition was still running. The last update says it
    /// isn't, unless the acquisition ended with errors and the last update
    /// is an `Err`, and the receiver disconnects after it.
    pub fn running(&self) -> bool {
        self.running
    }

    /// How fast readouts are arriving, in readouts per second, when more than
    /// one is acquired (manual 6.3.2).
    pub fn readout_rate(&self) -> f64 {
        self.readout_rate
    }
}

impl fmt::Debug for StreamedUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the length is enough, the copy itself can be megabytes
        f.debug_struct("StreamedUpdate")
            .field("len", &self.len)
            .field("count", &self.count)
            .field("running", &self.running)
            .field("readout_rate", &self.readout_rate)
            .finish_non_exhaustive()
    }
}

/// What a streaming acquisition's callback keeps between PICam's calls.
struct StreamSink {
    layout: Arc<Layout>,
    buffer: Option<BufferBounds>,
    // the user's updates and the guard's end signal, dropped once picam
    // reports the acquisition ended, so both receivers disconnect
    senders: Option<(Sender<Result<StreamedUpdate>>, Sender<()>)>,
    // an update copied but not yet checked for overrun, with whether the
    // acquisition was still running
    pending: Option<(Result<StreamedUpdate>, bool)>,
}

impl StreamSink {
    /// Handles one event from the acquisition-updated trampoline. Only the
    /// trampoline, or a test with live memory, may build the events, since
    /// copying relies on `available` pointing at live readouts.
    fn handle(&mut self, event: &Event) {
        match event {
            Event::AcquisitionUpdated { available, status } => {
                let running = status.running != 0;
                // SAFETY: the trampoline dispatches this event inside picam's
                // callback, which passed `available`, for this camera's
                // acquisition, whose layout and buffer the sink holds
                let update = unsafe { self.copy(available.as_ref(), status) };
                match available {
                    // checked for overrun next, outside the sink lock
                    Some(_) => self.pending = Some((update, running)),
                    None => self.send(update, running),
                }
            }
            Event::AcquisitionChecked(overran) => {
                let Some((update, running)) = self.pending.take() else {
                    return;
                };
                // a copy picam may have written over midway is never sent,
                // and an update that's already an error keeps its error
                let update = match (update, overran) {
                    (Err(e), _) => Err(e),
                    (Ok(update), Ok(false)) => Ok(update),
                    (Ok(_), Ok(true)) => {
                        Err(Error::AcquisitionFailed(AcquisitionErrors::DATA_LOST))
                    }
                    (Ok(_), Err(e)) => Err(*e),
                };
                self.send(update, running);
            }
            _ => {}
        }
    }

    /// Sends `update`, then drops the senders if the acquisition has ended.
    fn send(&mut self, update: Result<StreamedUpdate>, running: bool) {
        if let Some((updates, _)) = &self.senders {
            // a dropped receiver only means nobody is listening
            let _ = updates.send(update);
        }
        if !running {
            self.senders = None;
        }
    }

    /// Copies `available`'s readouts, from the camera's buffer or PICam's own
    /// memory.
    ///
    /// # Safety
    ///
    /// Must be called inside the acquisition-updated callback that passed
    /// `available`, for an acquisition on a camera whose committed parameters
    /// `self.layout` was read from and whose buffer, if any, `self.buffer`
    /// describes.
    unsafe fn copy(
        &self,
        available: Option<&sys::PicamAvailableData>,
        status: &sys::PicamAcquisitionStatus,
    ) -> Result<StreamedUpdate> {
        check_errors(status.errors)?;
        let extent = match available {
            Some(available) => Readouts::extent(available, &self.layout)?,
            None => None,
        };
        let (words, len, count) = match extent {
            None => (Vec::new(), 0, 0),
            Some((start, len, count)) => {
                // checked before allocating, so a count picam got wrong is
                // refused rather than allocated for
                if self
                    .buffer
                    .is_some_and(|buffer| !buffer.contains(start, len))
                {
                    return Err(Error::InconsistentDataLayout);
                }
                let mut words = vec![0u64; len.div_ceil(8)];
                let bytes = as_bytes_mut(&mut words, len);
                match self.buffer {
                    // SAFETY: the bytes lie inside the camera's buffer
                    // (checked above), which the os allocated, which no rust
                    // code references or reads but volatilely, and which stays
                    // allocated while the sink is registered: replacing or
                    // freeing it needs `&mut Camera`, which the stream holds
                    // until the sink is removed, and a leaked stream's sink
                    // makes both refuse until the camera closes, which removes
                    // it first
                    Some(_) => unsafe { volatile_copy(start, bytes) },
                    // SAFETY: picam passed these readouts in its own memory to
                    // this callback, laid out as `layout` describes (manual 6.1
                    // and 6.3.1), and they stay valid while it runs (seen past
                    // the callback with sdk 5.15). that memory isn't circular,
                    // since clearing the readout count online needs a buffer,
                    // so picam isn't writing to them. `bytes` is a fresh copy
                    // of `len` bytes
                    None => unsafe { ptr::copy_nonoverlapping(start, bytes.as_mut_ptr(), len) },
                }
                (words, len, count)
            }
        };
        Ok(StreamedUpdate {
            words,
            len,
            count,
            layout: Arc::clone(&self.layout),
            running: status.running != 0,
            readout_rate: status.readout_rate,
        })
    }
}

/// Acquired readouts, borrowed from the memory PICam acquired them into, or
/// from a copy of it (see [`Camera::set_acquisition_buffer`]).
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
    /// Views `available`'s readouts in PICam's own memory.
    ///
    /// # Safety
    ///
    /// `available` must come from a successful `Picam_Acquire` or
    /// `Picam_WaitForAcquisitionUpdate` into PICam's own memory, on a camera
    /// whose committed parameters `layout` was read from. Its data must stay
    /// valid and unchanged for `'a`.
    unsafe fn from_raw(available: &sys::PicamAvailableData, layout: Arc<Layout>) -> Result<Self> {
        let Some((start, len, count)) = Self::extent(available, &layout)? else {
            return Ok(Self::empty(layout));
        };
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

    /// Where `available`'s readouts lie: their start, length and count, or
    /// `None` if there are none.
    fn extent(
        available: &sys::PicamAvailableData,
        layout: &Layout,
    ) -> Result<Option<(*const u8, usize, usize)>> {
        let count =
            usize::try_from(available.readout_count).map_err(|_| Error::InconsistentDataLayout)?;
        if count == 0 || available.initial_readout.is_null() {
            return Ok(None);
        }
        let len = (count - 1)
            .checked_mul(layout.readout_stride)
            .and_then(|start| start.checked_add(layout.readout_len))
            .ok_or(Error::InconsistentDataLayout)?;
        let start = available.initial_readout.cast::<u8>().cast_const();
        Ok(Some((start, len, count)))
    }

    /// No readouts.
    fn empty(layout: Arc<Layout>) -> Self {
        Self {
            bytes: &[],
            count: 0,
            layout,
        }
    }

    /// Views `count` readouts copied into `bytes`, whose length `extent`
    /// gave, so every readout lies inside it.
    fn from_copy(bytes: &'a [u8], count: usize, layout: Arc<Layout>) -> Self {
        Self {
            bytes,
            count,
            layout,
        }
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

    // `index` must be below `count`, and `extent` checked the bytes cover it
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
    /// They're borrowed from the readouts' memory when it's aligned for the
    /// pixel type, and copied when it isn't, since PICam doesn't document
    /// its alignment.
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

/// Copies `available`'s readouts out of the camera's `buffer`, then checks
/// PICam didn't write over them meanwhile.
fn copy_from_buffer<'a>(
    buffer: &'a mut AcquisitionBuffer,
    device: sys::PicamHandle,
    available: &sys::PicamAvailableData,
    layout: Arc<Layout>,
) -> Result<Readouts<'a>> {
    let Some((start, len, count)) = Readouts::extent(available, &layout)? else {
        return Ok(Readouts::empty(layout));
    };
    let bytes = buffer.copy_out(start, len)?;
    // picam writes over readouts not yet read once it runs out of room, then
    // sets the overrun flag (seen on a demo camera with sdk 5.15). a copy it
    // may have written over midway is never handed out
    if has_overrun(device)? {
        return Err(Error::AcquisitionFailed(AcquisitionErrors::DATA_LOST));
    }
    Ok(Readouts::from_copy(bytes, count, layout))
}

/// Reports whether `device`'s buffer has overrun, see
/// [`Camera::has_acquisition_buffer_overrun`].
pub(crate) fn has_overrun(device: sys::PicamHandle) -> Result<bool> {
    let mut overran: sys::pibln = 0;
    picam_call!(sys::PicamAdvanced_HasAcquisitionBufferOverrun(
        device,
        &mut overran
    ))?;
    Ok(overran != 0)
}

/// Copies `into.len()` bytes from `start` with volatile reads: byte by byte
/// up to the first 8-byte boundary, then 8 bytes at a time, then the rest.
/// `wrapping_add` rather than `add`, whose rules are about rust allocations.
///
/// # Safety
///
/// The bytes from `start` must lie inside memory outside every Rust
/// allocation that no Rust code references or reads but volatilely, such as
/// the camera's buffer (see [`AcquisitionBuffer`]), and must stay allocated
/// for the call. Reading them must not trap.
unsafe fn volatile_copy(start: *const u8, into: &mut [u8]) {
    let head = ((8 - start.addr() % 8) % 8).min(into.len());
    let (head_bytes, rest) = into.split_at_mut(head);
    let mut words = rest.chunks_exact_mut(8);
    let mut offset = 0;
    for byte in head_bytes {
        // SAFETY: inside the memory the caller guarantees, which
        // `read_volatile` may read however picam writes to it
        *byte = unsafe { start.wrapping_add(offset).read_volatile() };
        offset += 1;
    }
    for word in &mut words {
        // SAFETY: as above, for 8 bytes, which `head` aligned to 8
        let value = unsafe { start.wrapping_add(offset).cast::<u64>().read_volatile() };
        word.copy_from_slice(&value.to_ne_bytes());
        offset += 8;
    }
    for byte in words.into_remainder() {
        // SAFETY: as above
        *byte = unsafe { start.wrapping_add(offset).read_volatile() };
        offset += 1;
    }
}

/// The first `len` bytes of `words`.
fn as_bytes(words: &[u64], len: usize) -> &[u8] {
    assert!(
        len <= words.len() * 8,
        "`words` holds fewer than `len` bytes"
    );
    // SAFETY: `words` holds at least `len` initialized bytes, any of which is
    // a valid `u8`, and the view borrows `words`
    unsafe { slice::from_raw_parts(words.as_ptr().cast(), len) }
}

/// The first `len` bytes of `words`, to write.
fn as_bytes_mut(words: &mut [u64], len: usize) -> &mut [u8] {
    assert!(
        len <= words.len() * 8,
        "`words` holds fewer than `len` bytes"
    );
    // SAFETY: as in `as_bytes`, borrowing `words` mutably
    unsafe { slice::from_raw_parts_mut(words.as_mut_ptr().cast(), len) }
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
        let readouts = unsafe { Readouts::from_raw(&data(pointer, 2), Arc::new(test_layout())) }
            .expect("two readouts should be viewed");
        // two readouts, without padding after the last frame
        assert_eq!(readouts.bytes().len(), 72);
        assert_eq!(readouts.len(), 2);
        assert_eq!(readouts.bytes().as_ptr(), pointer.cast_const());

        // SAFETY: neither pointer is read, since a null pointer or no
        // readouts give an empty view
        let empty = [
            unsafe { Readouts::from_raw(&data(ptr::null_mut(), 2), Arc::new(test_layout())) },
            unsafe { Readouts::from_raw(&data(pointer, 0), Arc::new(test_layout())) },
        ];
        for readouts in empty {
            let readouts = readouts.expect("no data should be an empty view");
            assert!(readouts.is_empty());
            assert!(readouts.bytes().is_empty());
        }

        // SAFETY: a negative count is refused before the pointer is used
        let negative = unsafe { Readouts::from_raw(&data(pointer, -1), Arc::new(test_layout())) };
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
        let mut buffer = AcquisitionBuffer::new(64).expect("64 bytes should be allocated");
        let base = buffer.memory.as_ptr().cast_const();
        assert!(base.cast::<u64>().is_aligned());
        assert!(buffer.contains(base, 64));
        assert!(!buffer.contains(base, 65));
        assert!(!buffer.contains(base.wrapping_add(1), 64));
        assert!(!buffer.contains(base.wrapping_sub(1), 1));
        assert_eq!(buffer.to_raw().memory_size, 64);

        // read through a copy, since a view would make the buffer rust memory
        let bytes = buffer
            .copy_out(base, 64)
            .expect("the buffer should be copied");
        assert!(bytes.iter().all(|&byte| byte == 0));

        assert!(matches!(
            AcquisitionBuffer::new(usize::MAX),
            Err(Error::InsufficientMemory)
        ));
    }

    #[test]
    fn copies_out_every_byte_at_any_alignment() {
        let mut buffer = AcquisitionBuffer::new(64).expect("64 bytes should be allocated");
        let base = buffer.memory.as_ptr();
        for i in 0..64u8 {
            // SAFETY: the byte lies inside the buffer, which nothing else
            // writes to, and like reads, writes to it are volatile only
            unsafe { base.wrapping_add(usize::from(i)).write_volatile(i) };
        }
        let base = base.cast_const();
        for offset in 0..8 {
            for len in [0, 1, 7, 8, 9, 15, 16, 17, 64 - offset] {
                let copy = buffer
                    .copy_out(base.wrapping_add(offset), len)
                    .expect("bytes inside the buffer should be copied");
                assert!(copy.as_ptr().cast::<u64>().is_aligned());
                let expected: Vec<u8> = (offset..offset + len).map(|i| i as u8).collect();
                assert_eq!(copy, expected, "offset {offset}, length {len}");
            }
        }
    }

    #[test]
    fn copies_outside_the_buffer_are_refused() {
        let mut buffer = AcquisitionBuffer::new(60).expect("60 bytes should be allocated");
        let base = buffer.memory.as_ptr().cast_const();
        assert!(matches!(
            buffer.copy_out(base, 61),
            Err(Error::InconsistentDataLayout)
        ));
        assert!(matches!(
            buffer.copy_out(base.wrapping_sub(1), 1),
            Err(Error::InconsistentDataLayout)
        ));

        // two readouts in `test_layout`'s shape take 72 bytes, so they're
        // refused before the handle, which isn't a camera's, is used
        let available = sys::PicamAvailableData {
            initial_readout: buffer.memory.as_ptr().cast(),
            readout_count: 2,
        };
        let refused = copy_from_buffer(
            &mut buffer,
            ptr::null_mut(),
            &available,
            Arc::new(test_layout()),
        );
        assert!(matches!(refused, Err(Error::InconsistentDataLayout)));
    }

    /// A streaming sink over `test_layout`, with the receivers of its updates
    /// and its end.
    fn stream_sink(
        buffer: Option<BufferBounds>,
    ) -> (StreamSink, Receiver<Result<StreamedUpdate>>, Receiver<()>) {
        let (updates_sender, updates) = mpsc::channel();
        let (ended_sender, ended) = mpsc::channel();
        let sink = StreamSink {
            layout: Arc::new(test_layout()),
            buffer,
            senders: Some((updates_sender, ended_sender)),
            pending: None,
        };
        (sink, updates, ended)
    }

    /// An acquisition status with no readout rate.
    fn status(
        running: bool,
        errors: sys::PicamAcquisitionErrorsMask,
    ) -> sys::PicamAcquisitionStatus {
        sys::PicamAcquisitionStatus {
            running: running.into(),
            errors,
            readout_rate: 0.0,
        }
    }

    #[test]
    fn stream_sink_sends_checked_copies() {
        let none = sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_None;
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        fill(&mut storage[start..start + 72]);
        // two readouts laid out as `test_layout` describes, which outlive
        // every event below, as picam's data outlives its callback
        let available = Some(sys::PicamAvailableData {
            initial_readout: storage[start..].as_mut_ptr().cast(),
            readout_count: 2,
        });
        let (mut sink, updates, ended) = stream_sink(None);

        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(true, none),
        });
        // held until the overrun check
        assert!(matches!(updates.try_recv(), Err(TryRecvError::Empty)));
        sink.handle(&Event::AcquisitionChecked(Ok(false)));
        let update = updates
            .try_recv()
            .expect("the update should be sent")
            .expect("the update should be a copy");
        assert!(update.running());
        let readouts = update.readouts();
        assert_eq!(readouts.len(), 2);
        assert_eq!(readouts.bytes(), &storage[start..start + 72]);
        assert!(readouts.bytes().as_ptr().cast::<u64>().is_aligned());

        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(true, none),
        });
        sink.handle(&Event::AcquisitionChecked(Ok(true)));
        assert!(matches!(
            updates.try_recv(),
            Ok(Err(Error::AcquisitionFailed(errors))) if errors == AcquisitionErrors::DATA_LOST
        ));

        sink.handle(&Event::AcquisitionUpdated {
            available: None,
            status: status(false, none),
        });
        let last = updates
            .try_recv()
            .expect("the last update should be sent")
            .expect("the last update should be empty");
        assert!(!last.running());
        assert!(last.readouts().is_empty());
        assert!(matches!(
            updates.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        assert!(matches!(ended.try_recv(), Err(TryRecvError::Disconnected)));
    }

    #[test]
    fn stream_sink_refuses_data_outside_the_buffer() {
        let none = sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_None;
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        let base = storage[start..].as_ptr().addr();
        // the two readouts take 72 bytes, more than the buffer's 60, so
        // they're refused before being read
        let available = Some(sys::PicamAvailableData {
            initial_readout: storage[start..].as_mut_ptr().cast(),
            readout_count: 2,
        });
        let (mut sink, updates, _ended) = stream_sink(Some(BufferBounds { base, len: 60 }));
        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(true, none),
        });
        sink.handle(&Event::AcquisitionChecked(Ok(false)));
        assert!(matches!(
            updates.try_recv(),
            Ok(Err(Error::InconsistentDataLayout))
        ));

        // an overrun found afterwards doesn't hide the first error
        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(true, none),
        });
        sink.handle(&Event::AcquisitionChecked(Ok(true)));
        assert!(matches!(
            updates.try_recv(),
            Ok(Err(Error::InconsistentDataLayout))
        ));
    }

    #[test]
    fn stream_sink_reports_failed_overrun_checks() {
        let none = sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_None;
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        // two readouts' worth of live memory, which outlives the events
        let available = Some(sys::PicamAvailableData {
            initial_readout: storage[start..].as_mut_ptr().cast(),
            readout_count: 2,
        });
        let (mut sink, updates, _ended) = stream_sink(None);
        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(true, none),
        });
        sink.handle(&Event::AcquisitionChecked(Err(Error::InvalidHandle)));
        assert!(matches!(updates.try_recv(), Ok(Err(Error::InvalidHandle))));
    }

    #[test]
    fn stream_sink_ends_after_its_last_update() {
        let none = sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_None;
        let data_lost = sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_DataLost;
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        // two readouts' worth of live memory, which outlives the events
        let available = Some(sys::PicamAvailableData {
            initial_readout: storage[start..].as_mut_ptr().cast(),
            readout_count: 2,
        });

        // ending with errors: the last update is the error
        let (mut sink, updates, ended) = stream_sink(None);
        sink.handle(&Event::AcquisitionUpdated {
            available: None,
            status: status(false, data_lost),
        });
        assert!(matches!(
            updates.try_recv(),
            Ok(Err(Error::AcquisitionFailed(errors))) if errors == AcquisitionErrors::DATA_LOST
        ));
        assert!(matches!(
            updates.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        assert!(matches!(ended.try_recv(), Err(TryRecvError::Disconnected)));

        // ending with data: the senders go once it has been checked
        let (mut sink, updates, ended) = stream_sink(None);
        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(false, none),
        });
        assert!(matches!(ended.try_recv(), Err(TryRecvError::Empty)));
        sink.handle(&Event::AcquisitionChecked(Ok(false)));
        let last = updates
            .try_recv()
            .expect("the last update should be sent")
            .expect("the last update should be a copy");
        assert!(!last.running());
        assert_eq!(last.readouts().len(), 2);
        assert!(matches!(
            updates.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        assert!(matches!(ended.try_recv(), Err(TryRecvError::Disconnected)));
    }

    #[test]
    fn stream_sink_reports_acquisition_errors() {
        let data_lost = sys::PicamAcquisitionErrorsMask::PicamAcquisitionErrorsMask_DataLost;
        let mut storage = [0u8; 72 + 8];
        let start = storage.as_ptr().align_offset(align_of::<u32>());
        let available = Some(sys::PicamAvailableData {
            initial_readout: storage[start..].as_mut_ptr().cast(),
            readout_count: 2,
        });
        let (mut sink, updates, ended) = stream_sink(None);
        sink.handle(&Event::AcquisitionUpdated {
            available,
            status: status(true, data_lost),
        });
        sink.handle(&Event::AcquisitionChecked(Ok(false)));
        assert!(matches!(
            updates.try_recv(),
            Ok(Err(Error::AcquisitionFailed(errors))) if errors == AcquisitionErrors::DATA_LOST
        ));
        // still running, so nothing has disconnected
        assert!(matches!(ended.try_recv(), Err(TryRecvError::Empty)));
    }
}
