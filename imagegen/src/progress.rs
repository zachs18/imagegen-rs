use std::{
    future::Future,
    num::NonZeroUsize,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, RwLock,
    },
};

use getopt::{GetoptItem, Opt};

use crate::{CommonData, CommonLockedData};

use self::file::FileProgressor;

mod file;
#[cfg(feature = "framebuffer")]
mod framebuffer;
#[cfg(feature = "sdl2")]
mod sdl;
mod text;

#[derive(Clone)]
pub struct ProgressData {
    pub progress_interval: usize,
    pub progress_count: usize,
}

/// CommonData, but with its own progress_barrier.
/// The supervisor progressor handles the CommonData
pub struct ProgressSupervisorData<'a> {
    pub locked: &'a RwLock<CommonLockedData>,
    pub dimy: NonZeroUsize,
    pub dimx: NonZeroUsize,
    pub size: NonZeroUsize,
    pub progress_barrier: Arc<tokio::sync::Barrier>,
    pub finished: &'a AtomicBool,
    pub pixels_placed: &'a AtomicUsize,
    pub pixels_generated: &'a AtomicUsize,
    pub rng_seed: u64,
}

pub trait Progressor: Send {
    /// Caller should run this in a new thread
    fn run_alone(&self, data: ProgressData, common_data: Arc<CommonData>) {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let progress_barrier = Arc::new(tokio::sync::Barrier::new(2));
        let fut = {
            let common_data = common_data.clone();
            let progress_barrier = progress_barrier.clone();
            let func = self.make_supervised_progressor();
            async move {
                let supervisor_data = ProgressSupervisorData {
                    locked: &common_data.locked,
                    dimy: common_data.dimy,
                    dimx: common_data.dimx,
                    size: common_data.size,
                    progress_barrier,
                    finished: &common_data.finished,
                    pixels_placed: &common_data.pixels_placed,
                    pixels_generated: &common_data.pixels_generated,
                    rng_seed: common_data.rng_seed,
                };
                func(data, &supervisor_data).await;
            }
        };

        rt.block_on(async {
            let local = tokio::task::LocalSet::new();
            local
                .run_until(async {
                    let task = tokio::task::spawn_local(fut);
                    loop {
                        // Wait at the generator barrier first to ensure the generator is done writing
                        log::trace!(target: "barriers", "before progress barrier a");
                        common_data.progress_barrier.wait();
                        log::trace!(target: "barriers", "mid progress barrier a");
                        progress_barrier.wait().await;
                        log::trace!(target: "barriers", "after progress barrier a");

                        if common_data.finished.load(Ordering::SeqCst) {
                            // Only read this betwee barriers, so we know generator thread wont change it under us
                            log::trace!("supervisor breaking loop");
                            break;
                        }

                        // Wait at the progressor barrier first to ensure the progressors are done reading
                        // TODO: need to have all threads on one barrier, so that
                        // e.g. SDL can request a quit after barrier b, and then all threads will
                        // see it when they check between the barriers.
                        // OR: use a separate AtomicBool, and have the supervisor propagate the value.
                        log::trace!(target: "barriers", "before progress barrier b");
                        progress_barrier.wait().await;
                        log::trace!(target: "barriers", "mid progress barrier b");
                        common_data.progress_barrier.wait();
                        log::trace!(target: "barriers", "after progress barrier b");
                    }
                    log::trace!("joining task");
                    task.await.expect("task failed");
                    log::trace!("supervisor exiting");
                })
                .await;
        });
    }

    /// Caller should call this function in another thread, and keep its result
    /// on that thread
    fn make_supervised_progressor(
        &self,
    ) -> Box<
        dyn Send
            + for<'a> FnOnce(
                ProgressData,
                &'a ProgressSupervisorData<'a>,
            )
                -> Pin<Box<dyn Future<Output = ()> + 'a>>,
    >;
}

pub struct ProgressSupervisor {
    progressors: Vec<Box<dyn Progressor + Send>>,
}

impl Progressor for ProgressSupervisor {
    fn make_supervised_progressor(
        &self,
    ) -> Box<
        dyn Send
            + for<'a> FnOnce(
                ProgressData,
                &'a ProgressSupervisorData<'a>,
            )
                -> Pin<Box<dyn Future<Output = ()> + 'a>>,
    > {
        unreachable!(
            "Cannot run ProgressSupervisor under another ProgressSupervisor"
        )
    }

    fn run_alone(&self, data: ProgressData, common_data: Arc<CommonData>) {
        let progress_barrier =
            Arc::new(tokio::sync::Barrier::new(self.progressors.len() + 1));

        std::thread::scope(|scope| {
            for progressor in &self.progressors {
                scope.spawn({
                    let common_data = common_data.clone();
                    let progress_barrier = progress_barrier.clone();
                    let data = data.clone();
                    let func = progressor.make_supervised_progressor();
                    move || {
                        let supervisor_data = ProgressSupervisorData {
                            locked: &common_data.locked,
                            dimy: common_data.dimy,
                            dimx: common_data.dimx,
                            size: common_data.size,
                            progress_barrier,
                            finished: &common_data.finished,
                            pixels_placed: &common_data.pixels_placed,
                            pixels_generated: &common_data.pixels_generated,
                            rng_seed: common_data.rng_seed,
                        };
                        let fut = func(data, &supervisor_data);
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .build()
                            .unwrap();
                        rt.block_on(fut);
                    }
                });
            }

            let rt =
                tokio::runtime::Builder::new_current_thread().build().unwrap();
            rt.block_on(async {
                loop {
                    // Wait at the generator barrier first to ensure the generator is done writing
                    log::trace!(target: "barriers", "before progress barrier a");
                    common_data.progress_barrier.wait();
                    log::trace!(target: "barriers", "mid progress barrier a");
                    progress_barrier.wait().await;
                    log::trace!(target: "barriers", "after progress barrier a");
                    if common_data.finished.load(Ordering::SeqCst) {
                        break;
                    }

                    // Wait at the progressor barrier first to ensure the progressors are done reading
                    log::trace!(target: "barriers", "before progress barrier b");
                    progress_barrier.wait().await;
                    log::trace!(target: "barriers", "mid progress barrier b");
                    common_data.progress_barrier.wait();
                    log::trace!(target: "barriers", "after progress barrier b");
                }
                log::trace!("supervisor exiting");
            })
        });
    }
}

pub struct NoOpProgressor;

impl Progressor for NoOpProgressor {
    fn make_supervised_progressor(
        &self,
    ) -> Box<
        dyn Send
            + for<'a> FnOnce(
                ProgressData,
                &'a ProgressSupervisorData<'a>,
            )
                -> Pin<Box<dyn Future<Output = ()> + 'a>>,
    > {
        Box::new(|_progress_data, common_data| {
            Box::pin(async move {
                loop {
                    common_data.progress_barrier.wait().await;
                    if common_data.finished.load(Ordering::SeqCst) {
                        break;
                    }
                    common_data.progress_barrier.wait().await;
                }
            })
        })
    }
}

pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        Opt::short_long('P', "progressfile", getopt::HasArgument::Yes),
        Opt::short_long('d', "defaultprogressfile", getopt::HasArgument::No),
        Opt::short_long('T', "progresstext", getopt::HasArgument::No),
        Opt::short_long('I', "progressinterval", getopt::HasArgument::Yes),
        Opt::short_long('M', "progresscount", getopt::HasArgument::Yes),
        #[cfg(feature = "sdl2")]
        Opt::long("SDL", getopt::HasArgument::No),
        Opt::long("wait", getopt::HasArgument::Yes),
        #[cfg(feature = "framebuffer")]
        Opt::long("framebuffer", getopt::HasArgument::Optional),
    ]
}

pub fn handle_opts(
    opts: &[GetoptItem<'_>],
) -> (Box<dyn Progressor + Send>, ProgressData) {
    let mut progressors: Vec<Box<dyn Progressor + Send>> = vec![];
    let mut progress_interval = None;
    let mut progress_count = None;
    for opt in opts {
        match opt {
            GetoptItem::Opt { opt, arg: Some(filename) }
                if opt.is_long("progressfile") =>
            {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(*filename)
                    .unwrap();
                progressors.push(Box::new(FileProgressor::new(file)));
            }
            GetoptItem::Opt { opt, arg: None }
                if opt.is_long("defaultprogressfile") =>
            {
                todo!(
                    "open the default filename and make progress::file::FileProgressor"
                )
            }
            GetoptItem::Opt { opt, arg: None }
                if opt.is_long("progresstext") =>
            {
                progressors.push(Box::new(text::TextProgressor::new(|s| {
                    eprintln!("{}", s);
                })));
            }
            GetoptItem::Opt { opt, arg: Some(progress_interval_str) }
                if opt.is_long("progressinterval") =>
            {
                progress_interval =
                    Some(progress_interval_str.parse().unwrap());
            }
            GetoptItem::Opt { opt, arg: Some(progress_count_str) }
                if opt.is_long("progresscount") =>
            {
                progress_count = Some(progress_count_str.parse().unwrap());
            }
            #[cfg(feature = "sdl2")]
            GetoptItem::Opt { opt, arg: None } if opt.is_long("SDL") => {
                progressors.push(Box::new(sdl::Sdl2Progressor {}));
            }
            #[cfg(not(feature = "sdl2"))]
            GetoptItem::Opt { opt, arg: None } if opt.is_long("SDL") => {
                log::error!(
                    "Compiled without sdl2 support. Ignoring '--SDL' argument."
                );
            }
            GetoptItem::Opt { opt, arg: Some(_wait_time_str) }
                if opt.is_long("wait") =>
            {
                todo!("figure out wait handling")
            }
            #[cfg(feature = "framebuffer")]
            GetoptItem::Opt { opt, arg } if opt.is_long("framebuffer") => {
                let fb_path = PathBuf::from(arg.unwrap_or("/dev/fb0"));
                progressors.push(Box::new(
                    framebuffer::FramebufferProgressor { fb_path },
                ));
            }
            #[cfg(not(feature = "framebuffer"))]
            GetoptItem::Opt { opt, .. } if opt.is_long("framebuffer") => {
                log::error!(
                    "Compiled without framebuffer support. Ignoring '--framebuffer' argument."
                );
            }
            _ => {}
        }
    }

    let data = ProgressData {
        progress_interval: progress_interval.unwrap_or(1024),
        progress_count: progress_count.unwrap_or(1),
    };

    let progressor = if progressors.len() == 0 {
        log::trace!("no progressor requested, just doing text");
        Box::new(text::TextProgressor::new(|s| {
            eprintln!("{}", s);
        }))
    } else if progressors.len() == 1 {
        progressors.pop().unwrap()
    } else {
        Box::new(ProgressSupervisor { progressors })
    };

    (progressor, data)
}
