use std::{sync::{Barrier, atomic::{AtomicBool, AtomicUsize}, RwLock, Arc}, io::BufWriter, collections::VecDeque};

use getopt::{Opt, GetoptItem};

use crate::{pnmdata::PnmData, generate::Pixel, CommonData};

#[cfg(feature = "sdl")]
mod sdl;
#[cfg(feature = "framebuffer")]
mod framebuffer;
mod text;
mod file;

#[derive(Clone)]
pub struct ProgressData {
    pub progress_interval: usize,
}

pub trait Progressor {
    /// Caller should run this in a new thread
    fn run(&mut self, data: ProgressData, common_data: Arc<CommonData>);
}


pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        Opt::short_long('P', "progressfile", getopt::HasArgument::Yes),
        Opt::short_long('d', "defaultprogressfile", getopt::HasArgument::No),
        Opt::short_long('T', "progresstext", getopt::HasArgument::No),
        Opt::short_long('I', "progressinterval", getopt::HasArgument::Yes),
        Opt::long("SDL", getopt::HasArgument::No),
        Opt::long("wait", getopt::HasArgument::Yes),
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
            GetoptItem::Opt { opt, arg: Some(progress_interval) } if opt.long.as_deref() == Some("progressinterval") => {
                todo!("figure out progress interval handling")
            },
            GetoptItem::Opt { opt, arg: None } if opt.long.as_deref() == Some("SDL") => {
                todo!("figure out SDL handling")
            },
            GetoptItem::Opt { opt, arg: Some(wait_time) } if opt.long.as_deref() == Some("wait") => {
                todo!("figure out wait handling")
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
