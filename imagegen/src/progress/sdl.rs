use std::{pin::Pin, sync::atomic::Ordering, marker::PhantomData, ops::{Index, IndexMut}};

use crate::{progress::NoOpProgressor, color::Color};

use super::Progressor;

pub struct Sdl2Progressor {
}

struct SdlSurfacePixelsMut<'a> {
    data: *mut u8,
    byte_stride: usize,
    width: usize,
    height: usize,
    _phantom: PhantomData<&'a mut [u8]>,
}

impl<'a> SdlSurfacePixelsMut<'a> {
    pub unsafe fn new_unchecked(data: &'a mut [u8], byte_stride: usize, width: usize, height: usize) -> Self {
        Self {
            data: data.as_mut_ptr(),
            byte_stride,
            width,
            height,
            _phantom: PhantomData,
        }
    }
}

impl<'a> Index<(usize, usize)> for SdlSurfacePixelsMut<'a> {
    type Output = [u8; 4];

    fn index(&self, (row, col): (usize, usize)) -> &Self::Output {
        if row >= self.height || col >= self.width { panic!("index out of bounds"); }
        let byte_idx = col * 4 + row * self.byte_stride;
        unsafe {
            &*(self.data.wrapping_add(byte_idx).cast() as *const [u8; 4])
        }
    }
}

impl<'a> IndexMut<(usize, usize)> for SdlSurfacePixelsMut<'a> {
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut Self::Output {
        if row >= self.height || col >= self.width { panic!("index out of bounds"); }
        let byte_idx = col * 4 + row * self.byte_stride;
        unsafe {
            &mut *self.data.wrapping_add(byte_idx).cast()
        }
    }
}


impl Progressor for Sdl2Progressor {
    fn make_supervised_progressor(&mut self) -> Box<dyn Send + for<'a> FnOnce(super::ProgressData, &'a super::ProgressSupervisorData<'a>) -> Pin<Box<dyn std::future::Future<Output = ()> + 'a>>> {
        Box::new({
            move |progress_data, common_data| {
                let fut = async move {
                    let mut noop_fallback = NoOpProgressor;
                    // return noop_fallback.run_under_supervisor(data, common_data);

                    log::trace!(target: "sdl", "initializing sdl on thread {:?}", std::thread::current());
                    let sdl_context = match sdl2::init() {
                        Ok(ctx) => ctx,
                        Err(error) => {
                            log::error!("Failed to initialize SDL2: {error}");
                            return noop_fallback.make_supervised_progressor()(progress_data, common_data).await;
                        },
                    };

                    let mut events = match sdl_context.event_pump() {
                        Ok(events) => events,
                        Err(error) => {
                            log::error!("Failed to initialize SDL2 events: {error}");
                            return noop_fallback.make_supervised_progressor()(progress_data, common_data).await;
                        },
                    };

                    let video_subsystem = match sdl_context.video() {
                        Ok(subsystem) => subsystem,
                        Err(error) => {
                            log::error!("Failed to initialize SDL2 video subsystem: {error}");
                            return noop_fallback.make_supervised_progressor()(progress_data, common_data).await;
                        },
                    };

                    let window = match video_subsystem.window("imagegen-rs", common_data.dimx.get().try_into().unwrap(), common_data.dimy.get().try_into().unwrap())
                        .position_centered()
                        .build()
                    {
                        Ok(window) => window,
                        Err(error) => {
                            log::error!("Failed to initialize SDL2 window: {error}");
                            return noop_fallback.make_supervised_progressor()(progress_data, common_data).await;
                        },
                    };

                    // let mut canvas = match window.into_canvas().build() {
                    //     Ok(canvas) => canvas,
                    //     Err(error) => {
                    //         log::error!("Failed to initialize SDL2 canvas: {error}");
                    //         return noop_fallback.run_under_supervisor_for_real()(progress_data, common_data).await;
                    //     },
                    // };

                    use std::time::{Duration, Instant};
                    // TODO: make this configurable
                    let update_interval = Duration::from_millis(300);
                    let mut last_update = Instant::now();
                    let mut quit_requested = false;
                    log::trace!(target: "sdl", "starting sdl loop on thread {:?}", std::thread::current().id());
                    loop {
                        log::trace!(target: "sdl", "inside sdl loop on thread {:?}", std::thread::current().id());
                        log::trace!(target: "barriers", "sdl before barrier a");
                        common_data.progress_barrier.wait().await;
                        log::trace!(target: "barriers", "sdl after barrier a");
                        log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa 2", std::thread::current().id());

                        events.pump_events();

                        while let Some(ev) = events.poll_event() {
                            log::trace!(target: "sdl", "sdl event {:?} aaa 2", ev);
                            match ev {
                                sdl2::event::Event::Quit { timestamp }
                                | sdl2::event::Event::AppTerminating { timestamp } => {
                                    log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa 2", std::thread::current().id());
                                    quit_requested = true;
                                },
                                sdl2::event::Event::KeyDown { timestamp, window_id, keycode, scancode, keymod, repeat }
                                | sdl2::event::Event::KeyUp { timestamp, window_id, keycode, scancode, keymod, repeat } => {
                                    log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa 2", std::thread::current().id());
                                    if keycode == Some(sdl2::keyboard::Keycode::Escape) {
                                        quit_requested = true;
                                    }
                                },
                                _ => {},
                            }
                        }
                        log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa bbb", std::thread::current().id());


                        let now = Instant::now();
                        if true || now - last_update >= update_interval || common_data.finished.load(Ordering::SeqCst) {
                            log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa bbb", std::thread::current().id());
                            last_update = now;
                            let locked = common_data.locked.read().unwrap();
                            log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa bbb", std::thread::current().id());
                            let locked = &*locked;
                            log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa bbb", std::thread::current().id());
                            // for y in 0..common_data.dimy.get() {
                            //     for x in 0..common_data.dimx.get() {
                            //         let color = locked.image[(y, x)] * Color::splat(255.0);
                            //         framebuffer[y][x] = *color.cast().as_array();
                            //     }
                            // }
                            log::debug!("Writing image sdl");

                            let mut surface = match window.surface(&events) {
                                Ok(surface) => surface,
                                Err(error) => {
                                    panic!("Failed to initialize SDL2 window surface: {error}");
                                },
                            };

                            let byte_stride = surface.pitch() as usize;
                            let width = surface.width() as usize;
                            let height = surface.height() as usize;
                            surface.with_lock_mut(|data| {
                                let mut data = unsafe { SdlSurfacePixelsMut::new_unchecked(data, byte_stride, width, height) };
                                locked.placed_pixels.for_each_true(|row, col| {
                                    log::trace!("sdl placing pixel ({row},{col})");
                                    let color = locked.image[(row, col)] * Color::splat(255.0);
                                    let color = color.cast::<u8>();
                                    // let color = u32::from_ne_bytes(color.to_array());
                                    // canvas.pixel(col as _, row as _, color).unwrap();
                                    data[(row, col)] = color.to_array();
                                    log::trace!("sdl placed pixel ({row},{col})");
                                });
                            });
                            surface.finish().unwrap();
                            log::debug!("Wrote image sdl");
                        }
                        log::trace!(target: "sdl", "inside sdl loop on thread {:?} aaa bbb", std::thread::current().id());
                        if common_data.finished.load(Ordering::SeqCst) {
                            log::debug!("sdl broke out of loop");
                            break;
                        }
                        log::trace!(target: "barriers", "sdl before barrier b");
                        common_data.progress_barrier.wait().await;
                        log::trace!(target: "barriers", "sdl after barrier b");
                        if quit_requested {
                            common_data.finished.store(true, Ordering::SeqCst);
                        }
                    }
                };

                Box::pin(fut)
            }
        })
    }
}
