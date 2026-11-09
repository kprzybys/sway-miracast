//! GPU buffers for zero-copy capture: the compositor copies the screen into a GBM buffer object on
//! the GPU (screencopy into a linux-dmabuf wl_buffer) and VA imports the same buffer — no CPU copy.
//!
//! The buffers are LINEAR and the raw format comes from the dmabuf fourcc (see `wl::drm_gst_fmt`): the appsrc
//! caps are plain video/x-raw, a Y-tiled buffer behind them came out shredded on the TV (seen 2026-11-09).

use gstreamer as gst;
use gstreamer_allocators as gst_alloc;
use gstreamer_allocators::prelude::*;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use wayland_client::protocol::wl_buffer;
use wayland_protocols::wp::linux_dmabuf::zv1::client::zwp_linux_buffer_params_v1 as dbparams;

use crate::opts::Opts;
use crate::shm::RING;
use crate::wl::{CaptureFmt, Wl};

#[repr(C)]
struct GbmDevice {
    _p: [u8; 0],
}
#[repr(C)]
struct GbmBo {
    _p: [u8; 0],
}

#[link(name = "gbm")]
extern "C" {
    fn gbm_create_device(fd: i32) -> *mut GbmDevice;
    fn gbm_bo_create_with_modifiers(
        dev: *mut GbmDevice,
        width: u32,
        height: u32,
        format: u32,
        modifiers: *const u64,
        count: u32,
    ) -> *mut GbmBo;
    fn gbm_bo_get_fd(bo: *mut GbmBo) -> i32;
    fn gbm_bo_get_stride(bo: *mut GbmBo) -> u32;
    fn gbm_bo_get_offset(bo: *mut GbmBo, plane: i32) -> u32;
    fn gbm_bo_get_modifier(bo: *mut GbmBo) -> u64;
    fn gbm_bo_get_plane_count(bo: *mut GbmBo) -> i32;
}

/// One exported buffer object (single plane).
pub struct Bo {
    pub fd: OwnedFd,
    pub stride: u32,
    pub offset: u32,
    pub modifier: u64,
    pub size: usize,
}

pub struct Gbm {
    _node: std::fs::File, // the device must outlive the BOs
    dev: *mut GbmDevice,
}

impl Gbm {
    pub fn open(node: &str) -> Result<Self, String> {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(node).map_err(|e| format!("{node}: {e}"))?;
        // SAFETY: fd is an open render node that outlives the device (`_node` is kept in the same struct).
        let dev = unsafe { gbm_create_device(f.as_raw_fd()) };
        if dev.is_null() {
            return Err("gbm_create_device failed".into());
        }
        Ok(Gbm { _node: f, dev })
    }

    /// Allocate a single-plane buffer with exactly `modifier` (renderable by the compositor, importable by VA).
    pub fn alloc(&self, w: u32, h: u32, fourcc: u32, modifier: u64) -> Result<Bo, String> {
        // SAFETY: `self.dev` is a valid gbm device; `modifier` points to one u64 for the call, count = 1.
        let bo = unsafe { gbm_bo_create_with_modifiers(self.dev, w, h, fourcc, &modifier, 1) };
        if bo.is_null() {
            return Err(format!("gbm_bo_create_with_modifiers(0x{modifier:x}) failed"));
        }
        // BOs are intentionally leaked: they live as long as the process (ring of a few buffers).
        // SAFETY: `bo` was checked non-null above; the gbm getters only read it (BO intentionally never freed, see above).
        unsafe {
            if gbm_bo_get_plane_count(bo) != 1 {
                return Err("multi-plane buffer (e.g. compression) not supported".into());
            }
            let fd = gbm_bo_get_fd(bo);
            if fd < 0 {
                return Err("gbm_bo_get_fd failed".into());
            }
            let fd = OwnedFd::from_raw_fd(fd);
            let size = libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_END) as usize;
            Ok(Bo {
                fd,
                stride: gbm_bo_get_stride(bo),
                offset: gbm_bo_get_offset(bo, 0),
                modifier: gbm_bo_get_modifier(bo),
                size,
            })
        }
    }
}

/// DRM_FORMAT_MOD_LINEAR: rows one after another, what plain video/x-raw caps mean. (Intel Y-tiling,
/// 0x0100000000000002, was shredded behind those caps.)
pub const LINEAR: u64 = 0;

/// Preferred render node (what was hardcoded before --render-node): tried first when present.
const DEFAULT_RENDER_NODE: &str = "/dev/dri/renderD128";

/// Render nodes to try, in order: DEFAULT_RENDER_NODE first, then the other `renderD*` names sorted.
fn render_candidates(mut names: Vec<String>) -> Vec<String> {
    names.retain(|n| n.starts_with("renderD"));
    names.sort();
    let mut paths: Vec<String> = names.into_iter().map(|n| format!("/dev/dri/{n}")).collect();
    if let Some(i) = paths.iter().position(|p| p == DEFAULT_RENDER_NODE) {
        let d = paths.remove(i);
        paths.insert(0, d);
    }
    paths
}

/// The GBM device on --render-node, or on the first render node that opens.
fn open_gbm(o: &Opts) -> Result<(Gbm, String), String> {
    if let Some(n) = &o.render_node {
        return Gbm::open(n).map(|g| (g, n.clone()));
    }
    let names = std::fs::read_dir("/dev/dri")
        .map_err(|e| format!("/dev/dri: {e}"))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    let mut errs = Vec::new();
    for n in render_candidates(names) {
        match Gbm::open(&n) {
            Ok(g) => return Ok((g, n)),
            Err(e) => errs.push(e),
        }
    }
    Err(format!("no usable render node ({})", errs.join("; ")))
}

/// GPU capture buffer: linear GBM BO in the compositor's dmabuf format (what plain video/x-raw caps describe).
pub struct Dma {
    pub wl: wl_buffer::WlBuffer,
    pub bo: Bo,
    /// DRM fourcc the compositor copies into
    pub fourcc: u32,
    pub w: u32,
    pub h: u32,
}

/// --dmabuf: a ring of GPU buffers, probed with one real copy. Empty = use shm.
pub fn dmabuf_ring(wl: &mut Wl, o: &Opts) -> Vec<Dma> {
    let qh = wl.qh.clone();
    let made = (|| -> Result<Vec<Dma>, String> {
        let (fcc, w, h) = wl.st.f.dmabuf.ok_or("compositor offers no dmabuf")?;
        let mgr = wl.st.dmabuf.clone().ok_or("no zwp_linux_dmabuf_v1 v3")?;
        let (gbm, node) = open_gbm(o)?;
        let gbm: &'static Gbm = Box::leak(Box::new(gbm)); // the device outlives every BO (process lifetime)
        wfd_util::event!("render_node", "path" => &node);
        (0..RING)
            .map(|_| {
                let bo = gbm.alloc(w, h, fcc, LINEAR)?;
                let params = mgr.create_params(&qh, ());
                params.add(bo.fd.as_fd(), 0, bo.offset, bo.stride, (bo.modifier >> 32) as u32, bo.modifier as u32);
                let buf = params.create_immed(w as i32, h as i32, fcc, dbparams::Flags::empty(), &qh, ());
                params.destroy();
                Ok(Dma { wl: buf, bo, fourcc: fcc, w, h })
            })
            .collect()
    })();
    let dring = match made {
        Ok(r) => r,
        Err(e) => {
            wfd_util::event!("dmabuf_unavailable", "why" => &e);
            return Vec::new();
        }
    };
    // probe: one copy into a dmabuf must succeed, else fall back to shm
    let fr = wl.capture(o.cursor);
    wl.wait_buffer_params();
    fr.copy(&dring[0].wl);
    wl.wait_copied();
    fr.destroy();
    if wl.st.f.failed {
        wfd_util::event!("dmabuf_unavailable", "why" => "compositor failed the copy");
        return Vec::new();
    }
    dring
}

/// A GStreamer buffer over a captured dmabuf (DRM format, one plane). None: the fd could not be duplicated or
/// imported (fd limit, allocator error) — the caller skips the frame.
pub fn dmabuf_buffer(alloc: &gst_alloc::DmaBufAllocator, bo: &Bo, cf: &CaptureFmt) -> Option<gst::Buffer> {
    let fd = bo.fd.try_clone().ok()?; // the allocator owns and closes its copy
                                      // SAFETY: the allocator takes ownership of a dup'ed dmabuf fd of `bo.size` bytes (the ring keeps its own copy).
    let mem = unsafe { alloc.alloc_dmabuf(fd, bo.size) }.ok()?;
    let mut b = gst::Buffer::new();
    {
        let bm = b.get_mut()?; // a new buffer is writable
        bm.append_memory(mem);
        gstreamer_video::VideoMeta::add_full(
            bm,
            gstreamer_video::VideoFrameFlags::empty(),
            gstreamer_video::VideoFormat::DmaDrm,
            cf.w,
            cf.h,
            &[bo.offset as usize],
            &[bo.stride as i32],
        )
        .ok()?;
    }
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_node_prefers_d128_then_sorted() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            render_candidates(v(&["card0", "renderD129", "by-path", "renderD128"])),
            v(&["/dev/dri/renderD128", "/dev/dri/renderD129"])
        );
        assert_eq!(
            render_candidates(v(&["renderD130", "renderD129"])),
            v(&["/dev/dri/renderD129", "/dev/dri/renderD130"])
        );
        assert!(render_candidates(v(&["card0"])).is_empty());
    }
}
