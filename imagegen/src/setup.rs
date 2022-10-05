use std::{sync::{Barrier, RwLock, Arc}, collections::VecDeque, num::NonZeroUsize};

use bitmap::BitMap;
use getopt::{Opt, GetoptItem};
use rand::{RngCore, SeedableRng};

use crate::{pnmdata::PnmData, CommonData, color::Color, CommonLockedData};

pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        Opt::short_long('x', "x", getopt::HasArgument::Yes),
        Opt::short_long('y', "y", getopt::HasArgument::Yes),
        Opt::short_long('s', "size", getopt::HasArgument::Yes),
        Opt::long("maxval", getopt::HasArgument::Yes),
        Opt::short_long('S', "seed", getopt::HasArgument::Yes),
    ]
}

pub fn handle_opts(opts: &[GetoptItem]) -> (Arc<CommonData>, impl RngCore + Send) {
    let mut size = (None, None);
    let mut maxval = None;
    let mut seed = None;

    macro_rules! set {
        ($arg:expr => $e:expr => $field:literal) => {
            match &mut $e {
                Some(_) => panic!("multiple {} values specified", $field),
                None => match $arg.parse() {
                    Ok(value) => $e = Some(value),
                    Err(_) => panic!("invalid {} value: {:?}", $field, $arg),
                }
            }
        };
    }

    for opt in opts {
        match opt {
            GetoptItem::Opt { opt, arg: Some(width) } if opt.long.as_deref() == Some("x") => {
                set!(width => size.0 => "width");
            },
            GetoptItem::Opt { opt, arg: Some(height) } if opt.long.as_deref() == Some("y") => {
                set!(height => size.1 => "height");
            },
            GetoptItem::Opt { opt, arg: Some(size_str) } if opt.long.as_deref() == Some("size") => {
                let (width, height) = size_str.split_once(',').or_else(|| size_str.split_once('x')).expect("invalid size");
                set!(width => size.0 => "width");
                set!(height => size.1 => "height");
            },
            GetoptItem::Opt { opt, arg: Some(maxval_str) } if opt.long.as_deref() == Some("maxval") => {
                set!(maxval_str => maxval => "maxval");
            },
            GetoptItem::Opt { opt, arg: Some(seed_str) } if opt.long.as_deref() == Some("seed") => {
                set!(seed_str => seed => "seed");
            },
            _ => {},
        }
    }

    const DEFAULT_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(256) };

    let (dimx, dimy) = (size.0.unwrap_or(DEFAULT_SIZE), size.1.unwrap_or(DEFAULT_SIZE));
    let maxval = maxval.unwrap_or(255);
    let size = NonZeroUsize::new(dimx.get().checked_mul(dimy.get()).unwrap()).unwrap();

    let image = PnmData {
        dimx: dimx.get() as u32,
        dimy: dimy.get() as u32,
        maxval,
        depth: 3,
        comments: vec![],
        rawdata: vec![Color::default(); size.get()],
    };

    let seed = seed.unwrap_or_else(|| rand::thread_rng().next_u64());

    let locked = CommonLockedData {
        image,
        placed_pixels: BitMap::new(dimy.get(), dimx.get()).unwrap(),
        edges: VecDeque::with_capacity(std::cmp::max(dimx, dimy).get() * 4),
    };

    let data = Arc::new(CommonData {
        locked: RwLock::new(locked),
        dimy,
        dimx,
        size: dimy.checked_mul(dimx).unwrap(),
        progress_barrier: Barrier::new(2),
        finished: false.into(),
        pixels_placed: 0.into(),
        pixels_generated: 0.into(),
        rng_seed: seed,
    });

    let rng = rand_chacha::ChaCha12Rng::seed_from_u64(seed);

    (data, rng)
}
