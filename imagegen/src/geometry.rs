use std::{num::NonZeroUsize, sync::Arc};

use getopt::{GetoptItem, Opt};

use crate::{generate::Pixel, CommonData};

pub struct CanonicalPixel {
    pub x: usize,
    pub y: usize,
}

pub trait Geometry {
    fn canonicalize(&self, location: Pixel) -> Option<CanonicalPixel>;
}

struct NSWrappingGeometry<const NS_WRAP: bool, const EW_WRAP: bool> {
    /// Must be <= isize::MAX
    dimx: NonZeroUsize,
    /// Must be <= isize::MAX
    dimy: NonZeroUsize,
}

struct NEWrappingGeometry<const NE_WRAP: bool, const SW_WRAP: bool> {
    /// Must be <= isize::MAX
    dim: NonZeroUsize,
}

#[cfg(target_pointer_width = "16")]
compile_error!("Geometry code assumes i32 fits in isize");

impl<const NS_WRAP: bool, const EW_WRAP: bool> Geometry
    for NSWrappingGeometry<NS_WRAP, EW_WRAP>
{
    fn canonicalize(&self, location: Pixel) -> Option<CanonicalPixel> {
        // match (isize::try_from(location.x), isize::try_from(location.y)) {
        //     (Ok(x), Ok(y)) if x < self.dimx && y < self.dimy =>
        // Some(CanonicalPixel { x, y }),     _ => None,
        // }
        let x: usize = if EW_WRAP {
            // x mod self.dimx
            (location.x as isize % self.dimx.get() as isize) as usize
        } else {
            match usize::try_from(location.x) {
                Ok(x) if x < self.dimx.get() => x,
                _ => return None,
            }
        };
        let y: usize = if NS_WRAP {
            // x mod self.dimx
            (location.y as isize % self.dimy.get() as isize) as usize
        } else {
            match usize::try_from(location.y) {
                Ok(y) if y < self.dimy.get() => y,
                _ => return None,
            }
        };
        Some(CanonicalPixel { x, y })
    }
}

type NormalGeometry = NSWrappingGeometry<false, false>;

pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        // Opt::short_long('x', "x", getopt::HasArgument::Yes),
        // Opt::short_long('y', "y", getopt::HasArgument::Yes),
        // Opt::short_long('s', "size", getopt::HasArgument::Yes),
        // Opt::long("maxval", getopt::HasArgument::Yes),
        // Opt::short_long('S', "seed", getopt::HasArgument::Yes),
    ]
}

pub fn handle_opts(
    opts: &[GetoptItem<'_>],
    common_data: &CommonData,
) -> Arc<dyn Geometry + Send + Sync> {
    #[cfg(any())]
    {
        let mut size = (None, None);
        let mut maxval = None;
        let mut seed = None;

        macro_rules! set {
            ($arg:expr => $e:expr => $field:literal) => {
                match &mut $e {
                    Some(_) => panic!("multiple {} values specified", $field),
                    None => match $arg.parse() {
                        Ok(value) => $e = Some(value),
                        Err(_) => {
                            panic!("invalid {} value: {:?}", $field, $arg)
                        }
                    },
                }
            };
        }

        for opt in opts {
            match opt {
                GetoptItem::Opt { opt, arg: Some(width) }
                    if opt.long.as_deref() == Some("x") =>
                {
                    set!(width => size.0 => "width");
                }
                GetoptItem::Opt { opt, arg: Some(height) }
                    if opt.long.as_deref() == Some("y") =>
                {
                    set!(height => size.1 => "height");
                }
                GetoptItem::Opt { opt, arg: Some(size_str) }
                    if opt.long.as_deref() == Some("size") =>
                {
                    let (width, height) = size_str
                        .split_once(',')
                        .or_else(|| size_str.split_once('x'))
                        .expect("invalid size");
                    set!(width => size.0 => "width");
                    set!(height => size.1 => "height");
                }
                GetoptItem::Opt { opt, arg: Some(maxval_str) }
                    if opt.long.as_deref() == Some("maxval") =>
                {
                    set!(maxval_str => maxval => "maxval");
                }
                GetoptItem::Opt { opt, arg: Some(seed_str) }
                    if opt.long.as_deref() == Some("seed") =>
                {
                    set!(seed_str => seed => "seed");
                }
                _ => {}
            }
        }

        let (dimx, dimy) = (size.0.unwrap_or(256), size.1.unwrap_or(256));
        let maxval = maxval.unwrap_or(255);

        let image = PnmData {
            dimx,
            dimy,
            maxval,
            depth: 3,
            comments: vec![],
            rawdata: vec![
                Color::default();
                (dimx as usize).checked_mul(dimy as usize).unwrap()
            ],
        };

        let dimx = dimx as usize;
        let dimy = dimy as usize;
        let seed = seed.unwrap_or_else(|| rand::thread_rng().next_u64());

        let locked = CommonLockedData {
            image,
            placed_pixels: BitMap::new(dimy, dimx).unwrap(),
            edges: VecDeque::new(),
        };

        let data = Arc::new(CommonData {
            locked: RwLock::new(locked),
            height: dimy,
            width: dimx,
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
    Arc::new(NormalGeometry { dimx: common_data.dimx, dimy: common_data.dimy })
}
