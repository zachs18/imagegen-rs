use std::{path::PathBuf, os::fd::AsRawFd, mem::MaybeUninit, pin::Pin, fs::File, sync::atomic::Ordering, io::Error, simd::simd_swizzle};

use crate::{progress::NoOpProgressor, color::Color};

use super::Progressor;

mod raw;

pub struct FramebufferProgressor {
    pub(crate) fb_path: PathBuf,
}

pub struct MmappedFramebuffer {
    ptr: *mut u8,
    len: usize,
    stride: usize,
    width: usize,
    height: usize,
}

unsafe impl Send for MmappedFramebuffer {}

impl std::ops::Index<usize> for MmappedFramebuffer {
    type Output = [[u8; 4]];

    fn index(&self, index: usize) -> &Self::Output {
        if index >= self.height { panic!("index out of bounds"); }
        unsafe {
            std::slice::from_raw_parts(
                self.ptr.cast::<u8>().add(index * self.stride).cast(),
                self.width,
            )
        }
    }
}

impl std::ops::IndexMut<usize> for MmappedFramebuffer {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if index >= self.height { panic!("index out of bounds"); }
        unsafe {
            std::slice::from_raw_parts_mut(
                self.ptr.cast::<u8>().add(index * self.stride).cast(),
                self.width,
            )
        }
    }
}

impl Drop for MmappedFramebuffer {
    fn drop(&mut self) {
        unsafe {
            if libc::munmap(self.ptr.cast(), self.len) < 0 {
                log::error!("munmap({:p}, {}) failed", self.ptr, self.len);
            }
        }
    }
}

impl Progressor for FramebufferProgressor {
    fn make_supervised_progressor(&mut self) -> Box<dyn Send + for<'a> FnOnce(super::ProgressData, &'a super::ProgressSupervisorData<'a>) -> Pin<Box<dyn std::future::Future<Output = ()> + 'a>>> {
        let mut noop_fallback = NoOpProgressor;

        let fb = match File::options().write(true).read(true).open(&self.fb_path) {
            Ok(fb) => fb,
            Err(err) => {
                log::error!("Failed to open framebuffer {:?}: {}", self.fb_path, err);
                return noop_fallback.make_supervised_progressor();
            },
        };

        let fbfd = fb.as_raw_fd();
        let mut finfo = MaybeUninit::<raw::fb_fix_screeninfo>::uninit();
        let mut vinfo = MaybeUninit::<raw::fb_var_screeninfo>::uninit();
        let (finfo, vinfo) = unsafe {
            // Get framebuffer fixed screen information
            if libc::ioctl(fbfd, raw::FBIOGET_FSCREENINFO, finfo.as_mut_ptr()) != 0 {
                let err = Error::last_os_error();
                log::error!("Failed to read framebuffer fixed information: {err}");
                return noop_fallback.make_supervised_progressor();
            }

            // Get framebuffer variable screen information
            if libc::ioctl(fbfd, raw::FBIOGET_VSCREENINFO, vinfo.as_mut_ptr()) != 0 {
                let err = Error::last_os_error();
                log::error!("Failed to read framebuffer variable information: {err}");
                return noop_fallback.make_supervised_progressor();
            }

            (finfo.assume_init(), vinfo.assume_init())
        };

        if let Err(_) = usize::try_from(u32::MAX) {
            log::error!("This framebuffer code does not support 16-bit (How are you running linux on a 16-bit platform anyway?)");
            return noop_fallback.make_supervised_progressor();
        }

        // Map framebuffer to user memory
        let screensize = finfo.smem_len as usize;

        let ptr = unsafe {
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                screensize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fbfd,
                0,
            );
            if ptr as isize == -1 {
                let err = Error::last_os_error();
                log::error!("Failed to mmap framebuffer into memory: {err}");
                return noop_fallback.make_supervised_progressor();
            }
            ptr.cast()
        };
        // POSIX (and Linux) says it's fine to close a file after you mmap it;
        // the mmap'ed region stays.
        drop(fb);

        let mut framebuffer = MmappedFramebuffer {
            ptr,
            len: screensize,
            stride: finfo.line_length as usize,
            width: vinfo.xres_virtual as usize,
            height: vinfo.yres_virtual as usize,
        };

        Box::new({
            let fb_path = self.fb_path.clone();
            move |progress_data, common_data| {
                if common_data.dimx.get() > framebuffer.width {
                    log::error!("Image too wide for framebuffer {fb_path:?} ({} > {}).", common_data.dimx, vinfo.xres_virtual);
                    return noop_fallback.make_supervised_progressor()(progress_data, common_data);
                }
                if common_data.dimy.get() > framebuffer.height {
                    log::error!("Image too tall for framebuffer {fb_path:?} ({} > {}).", common_data.dimy, vinfo.yres_virtual);
                    return noop_fallback.make_supervised_progressor()(progress_data, common_data);
                }

                Box::pin(async move {
                    use std::time::{Duration, Instant};
                    // TODO: make this configurable
                    let update_interval = Duration::from_millis(300);
                    let mut last_update = Instant::now();
                    loop {
                        log::trace!(target: "barriers", "before progress barrier a");
                        common_data.progress_barrier.wait().await;
                        log::trace!(target: "barriers", "after progress barrier a");
                        let now = Instant::now();
                        if now - last_update >= update_interval || common_data.finished.load(Ordering::SeqCst) {
                            last_update = now;
                            let locked = common_data.locked.read().unwrap();
                            for y in 0..common_data.dimy.get() {
                                for x in 0..common_data.dimx.get() {
                                    let color = locked.image[(y, x)] * Color::splat(255.0);
                                    // framebuffer[y][x] = *color.cast().as_array();
                                    framebuffer[y][x] = simd_swizzle!(color.cast(), [2,1,0,3]).to_array();
                                }
                            }
                        }
                        if common_data.finished.load(Ordering::SeqCst) {
                            break;
                        }
                        log::trace!(target: "barriers", "before progress barrier b");
                        common_data.progress_barrier.wait().await;
                        log::trace!(target: "barriers", "after progress barrier b");
                    }
                })
            }
        })
    }
}
