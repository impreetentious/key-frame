//! The exported WebAssembly boundary.
//!
//! There is no binding generator here and no JavaScript glue to keep in step
//! with a tool version. The module imports nothing at all, so any host that can
//! instantiate WebAssembly can drive it: a browser, a bare `WebAssembly.
//! instantiate` in Node, or a standalone runtime. That is what makes the
//! native-versus-WebAssembly equality check possible on the same artifact the
//! application ships.
//!
//! The contract, in full:
//!
//! - The host calls `kf_alloc(len)` for a buffer, writes `len` bytes into the
//!   module's memory at the returned offset, and passes that offset back.
//! - Every entry point returns a status code; zero is success. On any nonzero
//!   status `kf_message_ptr`/`kf_message_len` describe what happened in UTF-8.
//! - `kf_output_ptr`/`kf_output_len` describe the bytes the last successful
//!   call produced. They are valid until the next call that produces output.
//! - Growing the memory invalidates previously read offsets, so a host reads
//!   `kf_output_ptr` after each call rather than caching it.
//!
//! Offsets and lengths are pointer-width. On `wasm32` that is a 32-bit value,
//! so a host sees plain `i32` parameters; building natively for a test keeps
//! the same source honest instead of silently truncating a 64-bit address into
//! a 32-bit one.
//!
//! An offset that crosses this boundary is a pointer the host will hand back,
//! so every address here is exposed and recovered explicitly — `expose_provenance`
//! going out, `with_exposed_provenance` coming in. Written as `as` casts, the
//! same round trip compiles and runs identically while telling the compiler the
//! provenance ended at the boundary, which is the one thing that is not true
//! about it.
//!
//! Every `unsafe` block below states the invariant the host must uphold for it.
//! They are the only ones in the repository.

use core::cell::RefCell;

use crate::session::{PacketOutcome, Session, Status};

/// The boundary contract's version. A host checks this before anything else;
/// it changes only when the meaning of an existing entry point changes.
pub const ABI_VERSION: u32 = 1;

thread_local! {
    /// WebAssembly modules are single-threaded, and the session is owned
    /// entirely by this module: nothing hands out a reference to it, so every
    /// borrow below begins and ends inside one call.
    static SESSION: RefCell<Session> = RefCell::new(Session::new());
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_abi_version() -> u32 {
    ABI_VERSION
}

/// Reserves `len` bytes and returns their offset in the module's memory.
///
/// Returns zero when the length cannot be represented, which a host must treat
/// as a failed allocation. Offset zero is never a valid buffer.
#[unsafe(no_mangle)]
pub extern "C" fn kf_alloc(len: usize) -> usize {
    // Fallible on purpose. `vec![0; len]` would call `handle_alloc_error` when
    // the module cannot grow its memory, which on this target is an
    // `unreachable` trap: the instance dies and every later call fails, so the
    // host's "could not allocate room for the stream" path was unreachable and
    // a large stream took the whole session down instead of returning an error.
    let mut buffer: Vec<u8> = Vec::new();
    if buffer.try_reserve_exact(len).is_err() {
        return 0;
    }
    buffer.resize(len, 0);
    // `kf_free` rebuilds this allocation from the offset and length alone, so
    // the capacity has to equal the length exactly. `try_reserve_exact` gives
    // exactly `len` for a byte vector, and the conversion below is then a
    // no-op that makes the requirement explicit rather than assumed.
    debug_assert_eq!(buffer.capacity(), len);
    let boxed = buffer.into_boxed_slice();
    // The address is handed to the host, which hands it back to `kf_free`. That
    // round trip is what `expose_provenance` states: an `as usize` here and a
    // `as *mut u8` there would make the same journey while telling the compiler
    // the pointer's provenance ends at this line, which is not what happens.
    Box::into_raw(boxed).cast::<u8>().expose_provenance()
}

/// Releases a buffer previously returned by [`kf_alloc`].
#[unsafe(no_mangle)]
pub extern "C" fn kf_free(offset: usize, len: usize) {
    if offset == 0 {
        return;
    }
    // SAFETY: the host contract is that `offset` and `len` are exactly the
    // offset returned by a previous `kf_alloc` and the length passed to it, and
    // that the buffer has not already been freed. Reconstructing the `Vec` with
    // the same length and capacity is the inverse of the `mem::forget` there.
    unsafe {
        drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(
            core::ptr::with_exposed_provenance_mut::<u8>(offset),
            len,
        )));
    }
}

/// Opens a stream that may be damaged, walking it to the end.
///
/// The strict `kf_open` refuses anything a linear decode would refuse, which is
/// the right answer for a stream that should be intact. This is the other
/// question: what do the corruption rules do to this file? Afterwards
/// `kf_recovery_len` and `kf_recovery_at` report how each structurally accepted
/// packet was classified, and the frames the host can ask for are the display
/// timeline the normative document defines.
#[unsafe(no_mangle)]
pub extern "C" fn kf_open_tolerant(offset: usize, len: usize) -> u32 {
    if offset == 0 && len != 0 {
        return Status::BadStream.code();
    }
    // SAFETY: the host contract is that `offset` and `len` describe a buffer it
    // obtained from `kf_alloc` and filled, so the range is inside this module's
    // memory, initialized, and not aliased for writing while this call runs.
    let bytes = unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<u8>(offset), len)
    }
    .to_vec();
    SESSION.with_borrow_mut(|session| session.open_tolerant(bytes).code())
}

/// How many packets the last tolerant open classified. Zero after a strict one.
#[unsafe(no_mangle)]
pub extern "C" fn kf_recovery_len() -> usize {
    SESSION.with_borrow(|session| session.recovery().len())
}

/// One packet's outcome, by position. Out of range answers `u32::MAX`, which is
/// not one of the five codes.
#[unsafe(no_mangle)]
pub extern "C" fn kf_recovery_at(position: usize) -> u32 {
    SESSION.with_borrow(|session| {
        session
            .recovery()
            .get(position)
            .map_or(u32::MAX, |status| PacketOutcome::of(*status).code())
    })
}

/// Opens a stream previously written into the module's memory.
#[unsafe(no_mangle)]
pub extern "C" fn kf_open(offset: usize, len: usize) -> u32 {
    if offset == 0 && len != 0 {
        return Status::BadStream.code();
    }
    // SAFETY: the host contract is that `offset` and `len` describe a buffer it
    // obtained from `kf_alloc` and filled, so the range is inside this module's
    // memory, initialized, and not aliased for writing while this call runs.
    // The bytes are copied into the session immediately; nothing retains the
    // borrow past this statement.
    let bytes = unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<u8>(offset), len)
    }
    .to_vec();
    SESSION.with_borrow_mut(|session| session.open(bytes).code())
}

/// Decodes one frame by random access.
#[unsafe(no_mangle)]
pub extern "C" fn kf_decode_frame(index: u32) -> u32 {
    SESSION.with_borrow_mut(|session| session.decode_frame(index).code())
}

/// Reports one frame's syntax as UTF-8 JSON in the output buffer.
#[unsafe(no_mangle)]
pub extern "C" fn kf_probe_frame(index: u32) -> u32 {
    SESSION.with_borrow_mut(|session| session.probe(index).code())
}

/// Decodes every frame in order into one buffer.
#[unsafe(no_mangle)]
pub extern "C" fn kf_decode_all() -> u32 {
    SESSION.with_borrow_mut(|session| session.decode_all().code())
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_width() -> u32 {
    info(|info| info.width)
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_height() -> u32 {
    info(|info| info.height)
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_fps_num() -> u32 {
    info(|info| info.fps_num)
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_fps_den() -> u32 {
    info(|info| info.fps_den)
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_frame_count() -> u32 {
    info(|info| info.frame_count)
}

/// How many keyframes the open stream carries.
#[unsafe(no_mangle)]
pub extern "C" fn kf_keyframe_count() -> u32 {
    SESSION.with_borrow(|session| u32::try_from(session.keyframes().len()).unwrap_or(u32::MAX))
}

/// The keyframe at `position` in ascending order, or `u32::MAX` past the end.
#[unsafe(no_mangle)]
pub extern "C" fn kf_keyframe_at(position: u32) -> u32 {
    SESSION.with_borrow(|session| {
        usize::try_from(position)
            .ok()
            .and_then(|position| session.keyframes().get(position).copied())
            .unwrap_or(u32::MAX)
    })
}

/// The keyframe the last decode restarted from.
#[unsafe(no_mangle)]
pub extern "C" fn kf_last_entry_keyframe() -> u32 {
    SESSION.with_borrow(|session| session.last_entry().0)
}

/// How many frames the last decode had to decode, including its keyframe.
#[unsafe(no_mangle)]
pub extern "C" fn kf_last_entry_cost() -> u32 {
    SESSION.with_borrow(|session| session.last_entry().1)
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_output_ptr() -> usize {
    SESSION.with_borrow(|session| session.output().as_ptr().expose_provenance())
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_output_len() -> usize {
    SESSION.with_borrow(|session| session.output().len())
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_message_ptr() -> usize {
    SESSION.with_borrow(|session| session.message().as_ptr().expose_provenance())
}

#[unsafe(no_mangle)]
pub extern "C" fn kf_message_len() -> usize {
    SESSION.with_borrow(|session| session.message().len())
}

fn info(read: impl Fn(crate::session::StreamInfo) -> u32) -> u32 {
    SESSION.with_borrow(|session| session.info().map_or(0, &read))
}

#[cfg(test)]
mod tests {
    use super::{
        ABI_VERSION, kf_abi_version, kf_alloc, kf_decode_frame, kf_frame_count, kf_free,
        kf_keyframe_at, kf_keyframe_count, kf_message_len, kf_open, kf_output_len, kf_width,
    };

    const ORACLE_STREAM: [u8; 54] = [
        0x4b, 0x46, 0x56, 0x31, 0x01, 0x00, 0x40, 0x00, 0x40, 0x00, 0x01, 0x08, 0x18, 0x00, 0x01,
        0x00, 0x78, 0x00, 0x10, 0x00, 0x0e, 0x72, 0xb9, 0x5d, 0x4b, 0x46, 0x50, 0x31, 0x06, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x20, 0x00, 0x00, 0x23, 0x0e, 0x00, 0x74, 0x8a,
        0x7c, 0x2a, 0x57, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    /// An allocation the module cannot satisfy has to be reported, not fatal.
    ///
    /// The host checks the returned offset for zero and raises a clean error.
    /// That path was unreachable while this function allocated infallibly: the
    /// allocator aborted instead, which on the WebAssembly target traps and
    /// kills the instance, so an oversized stream ended the session rather than
    /// failing one call.
    #[test]
    fn an_impossible_allocation_is_reported_rather_than_fatal() {
        assert_eq!(kf_alloc(usize::MAX), 0);
        assert_eq!(kf_alloc(usize::MAX / 2), 0);
        // The module stays usable afterwards, which is the whole point.
        let offset = kf_alloc(16);
        assert_ne!(offset, 0);
        kf_free(offset, 16);
    }

    /// A zero-length request is still a usable, freeable buffer.
    #[test]
    fn an_empty_allocation_round_trips() {
        let offset = kf_alloc(0);
        assert_ne!(offset, 0, "zero is reserved for failure");
        kf_free(offset, 0);
    }

    /// Copies a stream through the real allocation path, exactly as a host
    /// would, rather than reaching past the boundary being tested.    /// Copies a stream through the real allocation path, exactly as a host
    /// would, rather than reaching past the boundary being tested.
    fn open_oracle() -> (usize, usize) {
        let len = ORACLE_STREAM.len();
        let offset = kf_alloc(len);
        assert_ne!(offset, 0);
        // SAFETY: `offset` is the offset of a buffer of exactly `len` bytes
        // just returned by `kf_alloc`, and nothing else holds a reference to it.
        unsafe {
            core::ptr::copy_nonoverlapping(
                ORACLE_STREAM.as_ptr(),
                core::ptr::with_exposed_provenance_mut::<u8>(offset),
                len,
            );
        }
        (offset, len)
    }

    #[test]
    fn the_boundary_reports_its_own_version() {
        assert_eq!(kf_abi_version(), ABI_VERSION);
    }

    #[test]
    fn a_zero_length_allocation_is_still_a_usable_offset() {
        // Zero is the sentinel `kf_alloc` returns when it cannot allocate, and
        // `kf_free` treats it as a no-op — so an empty allocation that came
        // back as zero would be reported to the host as a failure and would
        // never be freed. That is exactly the property this test is named for,
        // and it used to call both functions and assert nothing at all.
        let offset = kf_alloc(0);
        assert_ne!(
            offset, 0,
            "an empty allocation must be distinguishable from a failed one"
        );
        kf_free(offset, 0);
    }

    #[test]
    fn a_stream_round_trips_through_the_allocation_contract() {
        let (offset, len) = open_oracle();
        assert_eq!(kf_open(offset, len), 0);
        kf_free(offset, len);
        assert_eq!(kf_width(), 64);
        assert_eq!(kf_frame_count(), 1);
        assert_eq!(kf_keyframe_count(), 1);
        assert_eq!(kf_keyframe_at(0), 0);
        assert_eq!(kf_keyframe_at(1), u32::MAX);
        assert_eq!(kf_decode_frame(0), 0);
        assert_eq!(kf_output_len(), 64 * 64 + 2 * 32 * 32);
        assert_eq!(kf_message_len(), 0);
    }

    #[test]
    fn a_null_offset_with_a_length_is_refused_rather_than_dereferenced() {
        assert_ne!(kf_open(0, 16), 0);
    }
}
