use std::{sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, RwLock, Arc}, future::Future, pin::Pin, path::PathBuf};

use getopt::{Opt, GetoptItem};

use crate::{CommonData, CommonLockedData};

#[cfg(feature = "sdl2")]
mod sdl;
#[cfg(feature = "framebuffer")]
mod framebuffer;
mod text;
mod file;

#[derive(Clone)]
pub struct ProgressData {
    pub progress_interval: usize,
}

/// CommonData, but with its own progress_barrier.
/// The supervisor progressor handles the CommonData
pub struct ProgressSupervisorData<'a> {
    pub locked: &'a RwLock<CommonLockedData>,
    pub height: usize,
    pub width: usize,
    pub size: usize,
    pub progress_barrier: tokio::sync::Barrier,
    pub finished: &'a AtomicBool,
    pub pixels_placed: &'a AtomicUsize,
    pub pixels_generated: &'a AtomicUsize,
    pub rng_seed: u64,
}

pub trait Progressor {
    /// Caller should run this in a new thread
    fn run_alone(&mut self, data: ProgressData, common_data: Arc<CommonData>) {
        let supervisor_data = ProgressSupervisorData {
            locked: &common_data.locked,
            height: common_data.height,
            width: common_data.width,
            size: common_data.size,
            progress_barrier: tokio::sync::Barrier::new(2),
            finished: &common_data.finished,
            pixels_placed: &common_data.pixels_placed,
            pixels_generated: &common_data.pixels_generated,
            rng_seed: common_data.rng_seed,
        };
        std::thread::scope(|scope| {

            let fut = self.run_under_supervisor(data, &supervisor_data);
            scope.spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
                rt.block_on(fut);
            });

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            rt.block_on(async {
                loop {
                    common_data.progress_barrier.wait();
                    supervisor_data.progress_barrier.wait().await;
                    if common_data.finished.load(Ordering::SeqCst) {
                        // Only read this betwee barriers, so we know generator thread wont change it under us
                        log::trace!("supervisor breaking loop");
                        break;
                    }
                    common_data.progress_barrier.wait();
                    supervisor_data.progress_barrier.wait().await;
                }
                log::trace!("supervisor exiting");
            })
        });
    }

    /// Caller should run this on a tokio runtime in parallel with other progressors
    fn run_under_supervisor<'a>(&'a mut self, data: ProgressData, common_data: &'a ProgressSupervisorData<'a>) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

pub struct ProgressSupervisor {
    progressors: Vec<Box<dyn Progressor + Send>>,
}

impl Progressor for ProgressSupervisor {
    fn run_under_supervisor<'a>(&'a mut self, _data: ProgressData, _common_data: &'a ProgressSupervisorData<'a>) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        unreachable!("Cannot run ProgressSupervisor under another ProgressSupervisor")
    }

    fn run_alone(&mut self, data: ProgressData, common_data: Arc<CommonData>) {
        let supervisor_data = ProgressSupervisorData {
            locked: &common_data.locked,
            height: common_data.height,
            width: common_data.width,
            size: common_data.size,
            progress_barrier: tokio::sync::Barrier::new(self.progressors.len()),
            finished: &common_data.finished,
            pixels_placed: &common_data.pixels_placed,
            pixels_generated: &common_data.pixels_generated,
            rng_seed: common_data.rng_seed,
        };
        std::thread::scope(|scope| {
            for progressor in self.progressors.iter_mut() {
                let fut = progressor.run_under_supervisor(data.clone(), &supervisor_data);
                scope.spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
                    rt.block_on(fut);
                });
            }

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            rt.block_on(async {
                while !common_data.finished.load(Ordering::SeqCst) {
                    common_data.progress_barrier.wait();
                    supervisor_data.progress_barrier.wait().await;
                }
                log::trace!("supervisor exiting");
            })
        });
    }
}

pub struct NoOpProgressor;

impl Progressor for NoOpProgressor {
    fn run_under_supervisor<'a>(&'a mut self, _data: ProgressData, common_data: &'a ProgressSupervisorData<'a>) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            loop {
                common_data.progress_barrier.wait().await;
                if common_data.finished.load(Ordering::SeqCst) {
                    break;
                }
                common_data.progress_barrier.wait().await;
            }
        })
    }
}

pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        Opt::short_long('P', "progressfile", getopt::HasArgument::Yes),
        Opt::short_long('d', "defaultprogressfile", getopt::HasArgument::No),
        Opt::short_long('T', "progresstext", getopt::HasArgument::No),
        Opt::short_long('I', "progressinterval", getopt::HasArgument::Yes),
        Opt::long("SDL", getopt::HasArgument::No),
        Opt::long("wait", getopt::HasArgument::Yes),
        Opt::long("framebuffer", getopt::HasArgument::Optional),
    ]
}

pub fn handle_opts(opts: &[GetoptItem]) -> (Box<dyn Progressor + Send>, ProgressData)  {
    let mut progressors: Vec<Box<dyn Progressor + Send>> = vec![];
    let mut progress_interval = None;
    for opt in opts {
        match opt {
            GetoptItem::Opt { opt, arg: Some(filename) } if opt.long.as_deref() == Some("progressfile") => {
                todo!("open filename and make progress::file::FileProgressor")
            },
            GetoptItem::Opt { opt, arg: None } if opt.long.as_deref() == Some("defaultprogressfile") => {
                todo!("open the default filename and make progress::file::FileProgressor")
            },
            GetoptItem::Opt { opt, arg: None } if opt.long.as_deref() == Some("progresstext") => {
                todo!("make progress::text::TextProgressor with stderr")
            },
            GetoptItem::Opt { opt, arg: Some(progress_interval_str) } if opt.long.as_deref() == Some("progressinterval") => {
                progress_interval = Some(progress_interval_str.parse().unwrap());
            },
            GetoptItem::Opt { opt, arg: None } if opt.long.as_deref() == Some("SDL") => {
                todo!("figure out SDL handling")
            },
            GetoptItem::Opt { opt, arg: Some(wait_time) } if opt.long.as_deref() == Some("wait") => {
                todo!("figure out wait handling")
            },
            #[cfg(feature = "framebuffer")]
            GetoptItem::Opt { opt, arg } if opt.long.as_deref() == Some("framebuffer") => {
                let fb_path = PathBuf::from(arg.unwrap_or("/dev/fb0"));
                progressors.push(Box::new(framebuffer::FramebufferProgressor { fb_path }));
            },
            #[cfg(not(feature = "framebuffer"))]
            GetoptItem::Opt { opt, arg } if opt.long.as_deref() == Some("framebuffer") => {
                log::error!("Compiled without framebuffer support. Ignoring '--framebuffer' argument.");
            },
            _ => {},
        }
    }

    let data = ProgressData {
        progress_interval: progress_interval.unwrap_or(1024),
    };

    let progressor = if progressors.len() == 0 {
        log::trace!("no progressor requested, just doing text");
        Box::new(text::TextProgressor::new(|s| {
            eprintln!("{}", s);
        }))
    } else if progressors.len() == 1 {
        progressors.pop().unwrap()
    } else {
        todo!("progressor supervisor?")
    };

    (progressor, data)
}
