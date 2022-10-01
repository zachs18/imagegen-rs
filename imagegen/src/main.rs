#![feature(portable_simd)]

use std::{sync::{Arc, RwLock, Barrier, atomic::{AtomicBool, AtomicUsize}}, collections::VecDeque};

mod setup;
mod color;
mod generate;
mod progress;
mod pnmdata;

use bitmap::BitMap;
use color::ColorGenerator;
use generate::{Offset, Pixel};
use getopt::Getopt;
use pnmdata::PnmData;
use progress::Progressor;

use crate::{generate::GeneratorData, progress::ProgressData};

pub struct CommonLockedData {
    //geometry: Arc<dyn Geometry>,
    image: PnmData,
    placed_pixels: BitMap,
    /// Represents to-be-placed pixels
    edges: VecDeque<Pixel>,
}

pub struct CommonData {
    pub locked: RwLock<CommonLockedData>,
    pub height: usize,
    pub width: usize,
    pub size: usize,
    pub progress_barrier: Barrier,
    pub finished: AtomicBool,
    pub pixels_placed: AtomicUsize,
    pub pixels_generated: AtomicUsize,
    pub rng_seed: u64,
}

macro_rules! chain {
    ( $iter:expr $(,)? ) => {
        $iter.into_iter()
    };
    ( $iter:expr, $($iters:expr),* $(,)? ) => {
        $iter.into_iter().chain(chain!($($iters),*))
    };
}

fn main() {
    env_logger::builder().format(|f, record| {
        use std::io::Write;
        let tid = unsafe { libc::gettid() };
        let color = match record.level() {
            log::Level::Error => "31;1",
            log::Level::Warn => "33;1",
            log::Level::Info => "32;1",
            log::Level::Debug => "34;1",
            log::Level::Trace => "35;1",
        };
        writeln!(
            f,
            "\x1b[{}m{}\x1b[0m {} {} [{}:{}]: {}",
            color,
            record.level(),
            record.target(),
            tid,
            record.file().unwrap_or("<unknown>"),
            match record.line() {
                Some(line) => format!("{}", line),
                None => "<unknown>".to_owned(),
            },
            record.args(),
        )
    }).init();

    let args = std::env::args().skip(1).collect::<Vec<_>>();

    let getopt = Getopt::from_iter(
        chain!(
            setup::opts(),
            generate::opts(),
            color::opts(),
            progress::opts(),
        )
    ).unwrap();

    let opts = getopt.parse(args.iter().map(|x| &**x)).collect::<Result<Vec<_>, _>>().unwrap();

    let (mut common_data, mut rng) = setup::handle_opts(&opts);
    let mut generator = generate::handle_opts(&opts);
    let color_generator = color::handle_opts(&opts);
    log::trace!("color_generator: {:?}", color_generator);
    let (mut progressor, progress_data) = progress::handle_opts(&opts);

    let _gen_thread = std::thread::spawn({
        let common_data = common_data.clone();
        move || {
            let data = GeneratorData {
            };
            generator.generate(data, common_data, &*color_generator, &mut rng);
        }
    });

    let _prog_thread = std::thread::spawn({
        let common_data = common_data.clone();
        move || {
            progressor.run(progress_data, common_data);
        }
    });

    _gen_thread.join().unwrap();
    _prog_thread.join().unwrap();

    let locked = Arc::get_mut(&mut common_data).expect("all other threads have exited").locked.get_mut().unwrap();
    locked.image.write_to(&mut std::io::stdout().lock());

}
