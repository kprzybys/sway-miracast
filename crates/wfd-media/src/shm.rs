//! CPU capture path: a ring of shm buffers the compositor copies into, handed to GStreamer without copying.

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wayland_client::protocol::{wl_buffer, wl_shm};
use wayland_client::QueueHandle;
use wfd_util::die;

use crate::wl::State;

pub const RING: usize = 3;
/// Slot `i` is held by GStreamer (a live `ShmView`).
static SHM_BUSY: [AtomicBool; RING] = [const { AtomicBool::new(false) }; RING];

pub struct Shm {
    ptr: *mut u8,
    len: usize,
    pub buffer: wl_buffer::WlBuffer,
}

/// Read-only view of shm ring slot `.2` handed to GStreamer without copying. While a view lives its slot is
/// marked busy (`SHM_BUSY`) and the capture loop does not hand that slot to the compositor; dropping the view
/// (GStreamer is done with the buffer) frees it.
struct ShmView(*const u8, usize, usize);
// SAFETY: the pointer is into a mapping that is never unmapped (see `make_shm`), and the compositor never writes
// into a slot while its view lives (busy flag, checked before every capture request), so the bytes are stable
// for the view's lifetime on any thread.
unsafe impl Send for ShmView {}
impl AsRef<[u8]> for ShmView {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: ptr/len describe a live MAP_SHARED mapping that is never unmapped and not written while busy.
        unsafe { std::slice::from_raw_parts(self.0, self.1) }
    }
}
impl Drop for ShmView {
    fn drop(&mut self) {
        SHM_BUSY[self.2].store(false, Ordering::Release);
    }
}

/// A GStreamer buffer over ring slot `i` (just filled by the compositor); the slot stays busy until it is dropped.
pub fn shm_buffer(ring: &[Shm], i: usize) -> gstreamer::Buffer {
    SHM_BUSY[i].store(true, Ordering::Release);
    gstreamer::Buffer::from_slice(ShmView(ring[i].ptr, ring[i].len, i))
}

/// First slot from `from` on that GStreamer does not hold. All busy (VA upload stalled): wait in 1 ms steps,
/// up to 100 ms, then log and take `from` anyway rather than stop capturing (a torn frame beats a frozen one).
pub fn free_shm_slot(from: usize) -> usize {
    for _ in 0..100 {
        let busy: [bool; RING] = std::array::from_fn(|i| SHM_BUSY[i].load(Ordering::Acquire));
        if let Some(i) = first_free(from, &busy) {
            return i;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    wfd_util::event!("shm_ring_full");
    from
}

/// First slot at or after `from` (wrapping) that is not busy.
fn first_free(from: usize, busy: &[bool]) -> Option<usize> {
    (0..busy.len()).map(|k| (from + k) % busy.len()).find(|&i| !busy[i])
}

/// One ring slot: a memfd mapped for the process lifetime, shared with the compositor as a wl_buffer.
pub fn make_shm(st: &State, qh: &QueueHandle<State>, fmt: wl_shm::Format, w: u32, h: u32, stride: u32) -> Shm {
    let fail = |what: &str| -> ! { die("shm_alloc_failed", &format!("{what}: {}", std::io::Error::last_os_error())) };
    let len = (stride * h) as usize;
    // SAFETY: the name is a valid C string literal; flags are valid.
    let fd = unsafe { libc::memfd_create(c"wfd-media".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        fail("memfd_create");
    }
    // SAFETY: memfd_create returned a fresh fd (checked >= 0) that nothing else owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: fd is the valid memfd owned just above.
    if unsafe { libc::ftruncate(fd.as_raw_fd(), len as i64) } != 0 {
        fail("ftruncate");
    }
    // SAFETY: fd is a valid memfd of `len` bytes; the mapping is checked against MAP_FAILED and never unmapped.
    let ptr = unsafe {
        libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd.as_raw_fd(), 0)
    };
    if ptr == libc::MAP_FAILED {
        fail("mmap");
    }
    let Some(shm) = st.shm.as_ref() else { die("shm_alloc_failed", "compositor offers no wl_shm") };
    let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
    let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, fmt, qh, ());
    pool.destroy();
    Shm { ptr: ptr as *mut u8, len, buffer }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_free_wraps_and_skips_busy_slots() {
        assert_eq!(first_free(0, &[false, false, false]), Some(0));
        assert_eq!(first_free(1, &[false, true, false]), Some(2));
        assert_eq!(first_free(2, &[false, true, true]), Some(0));
        assert_eq!(first_free(0, &[true, true, true]), None);
    }
}
