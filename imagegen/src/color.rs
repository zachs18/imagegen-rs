use getopt::{GetoptItem, Opt};
use rand::{Rng, RngCore};
use std::{borrow::Cow, num::NonZeroUsize, simd::Simd};

#[cfg(feature = "f32")]
pub type Channel = f32;

#[cfg(not(feature = "f32"))]
pub type Channel = f64;

pub type Color = Simd<Channel, 4>;

pub const fn from_3(r: Channel, g: Channel, b: Channel) -> Color {
    Color::from_array([r, g, b, 0.0])
}

const ONE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(1) };

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VectorSetKind {
    Full,
    Triangular,
    SumOne,
}

impl Default for VectorSetKind {
    fn default() -> Self {
        VectorSetKind::Full
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct VectorSet {
    start: Color,
    vectors: Cow<'static, [Color]>,
    chance: NonZeroUsize,
    kind: VectorSetKind,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct VectorSetGroup {
    // Must never be empty
    vectorsets: Cow<'static, [VectorSet]>,
    // Must equal self.vectorsets.map(.chance).sum()
    total_chance: NonZeroUsize,
}

impl VectorSetGroup {
    pub fn new(
        vectorsets: Cow<'static, [VectorSet]>,
    ) -> Result<Self, Cow<'static, [VectorSet]>> {
        let total_chance = vectorsets.iter().map(|vs| vs.chance.get()).sum();
        if let Some(total_chance) = NonZeroUsize::new(total_chance) {
            Ok(Self { vectorsets, total_chance })
        } else {
            Err(vectorsets)
        }
    }
}

pub trait ColorGenerator: std::fmt::Debug {
    /// Using rng, generate a new color in this colorspace.
    fn new_color(&self, rng: &mut dyn RngCore) -> Color;

    #[doc(hidden)]
    #[cfg(test)]
    fn as_vectorset(&self) -> Option<&VectorSet> {
        None
    }

    #[doc(hidden)]
    #[cfg(test)]
    fn as_vectorsetgroup(&self) -> Option<&VectorSetGroup> {
        None
    }
}

impl<'a, G: ColorGenerator + ?Sized> ColorGenerator for &'a G {
    fn new_color(&self, rng: &mut dyn RngCore) -> Color {
        (**self).new_color(rng)
    }

    #[doc(hidden)]
    #[cfg(test)]
    fn as_vectorset(&self) -> Option<&VectorSet> {
        (**self).as_vectorset()
    }

    #[doc(hidden)]
    #[cfg(test)]
    fn as_vectorsetgroup(&self) -> Option<&VectorSetGroup> {
        (**self).as_vectorsetgroup()
    }
}

static BASIC_COLOR: VectorSet = VectorSet {
    start: from_3(0.0, 0.0, 0.0),
    vectors: Cow::Borrowed(&[
        from_3(1.0, 0.0, 0.0),
        from_3(0.0, 1.0, 0.0),
        from_3(0.0, 0.0, 1.0),
    ]),
    chance: ONE,
    kind: VectorSetKind::Full,
};

static FULL_INTENSITY_HUES: &'static [VectorSet] = &[
    VectorSet {
        start: from_3(1.0, 0.0, 0.0),
        vectors: Cow::Borrowed(&[from_3(0.0, 1.0, 0.0)]),
        chance: ONE,
        kind: VectorSetKind::Full,
    },
    VectorSet {
        start: from_3(0.0, 1.0, 0.0),
        vectors: Cow::Borrowed(&[from_3(1.0, 0.0, 0.0)]),
        chance: ONE,
        kind: VectorSetKind::Full,
    },
    VectorSet {
        start: from_3(0.0, 1.0, 0.0),
        vectors: Cow::Borrowed(&[from_3(0.0, 0.0, 1.0)]),
        chance: ONE,
        kind: VectorSetKind::Full,
    },
    VectorSet {
        start: from_3(0.0, 0.0, 1.0),
        vectors: Cow::Borrowed(&[from_3(0.0, 1.0, 0.0)]),
        chance: ONE,
        kind: VectorSetKind::Full,
    },
    VectorSet {
        start: from_3(0.0, 0.0, 1.0),
        vectors: Cow::Borrowed(&[from_3(1.0, 0.0, 0.0)]),
        chance: ONE,
        kind: VectorSetKind::Full,
    },
    VectorSet {
        start: from_3(1.0, 0.0, 0.0),
        vectors: Cow::Borrowed(&[from_3(0.0, 0.0, 1.0)]),
        chance: ONE,
        kind: VectorSetKind::Full,
    },
];

impl ColorGenerator for VectorSet {
    fn new_color(&self, rng: &mut dyn RngCore) -> Color {
        let mut c = self.start;
        match self.kind {
            // Each vector multiplier is generated independently.
            VectorSetKind::Full => {
                for vector in &*self.vectors {
                    let multplier: Channel = rng.gen_range(0.0..=1.0);
                    c += *vector * Color::splat(multplier);
                }
                log::trace!("generated {c:?}");
                c
            }
            // The sum of the vector multipliers is not more than one.
            VectorSetKind::Triangular => todo!(),
            // The sum of the vector multipliers is one.
            VectorSetKind::SumOne => todo!(),
        }
    }

    #[doc(hidden)]
    #[cfg(test)]
    fn as_vectorset(&self) -> Option<&VectorSet> {
        Some(self)
    }
}

impl ColorGenerator for VectorSetGroup {
    fn new_color(&self, rng: &mut dyn RngCore) -> Color {
        if self.vectorsets.len() == 0 {
            return Color::default();
        }
        if self.vectorsets.len() == 1 {
            return self.vectorsets[0].new_color(rng);
        }
        let mut chance = rng.gen_range(0..self.total_chance.get());
        for vectorset in &*self.vectorsets {
            if chance < vectorset.chance.get() {
                return vectorset.new_color(rng);
            }
            chance -= vectorset.chance.get();
        }
        unreachable!("total_chance should be the sum of all chances")
    }

    #[doc(hidden)]
    #[cfg(test)]
    fn as_vectorsetgroup(&self) -> Option<&VectorSetGroup> {
        Some(self)
    }
}

// pub fn options(cmd: clap::Command) -> clap::Command {
//     // let normal_color = arg!([normal_color] -N --normal "Default color
// generation.");     // let vector_color = ArgGroup::new("vector_color")
//     //     .arg(arg!(-n "Start a new vectorset."))
//     //     .arg(arg!(-v "Start a new vectorset."))
//     //     .arg(arg!(-b <base> "Make <base_vector> the starting color for the
// current vectorset."))     //     .arg(arg!(-t <vectorset_type> "Change the
// type of the current vectorset to <type>: full, triangular, or sum_one."))
//     //     .arg(arg!(--hues "All full-intensity hues."));

//     // // TODO
//     // cmd
//     //     .arg(normal_color)
//     //     .group(vector_color)
//     cmd
// }

pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        Opt::short_long('N', "normal", getopt::HasArgument::No),
        Opt::long("hues", getopt::HasArgument::No),
        Opt::short_long('n', "newvectorset", getopt::HasArgument::No),
        Opt::short_long('v', "vector", getopt::HasArgument::Yes),
        Opt::short_long('b', "base", getopt::HasArgument::Yes),
        Opt::short_long('t', "type", getopt::HasArgument::Yes),
    ]
}

fn parse_color(s: &str) -> Result<Color, String> {
    let mut color = [0.0; 4];
    for (s, channel) in s.split(',').zip(color.iter_mut()) {
        *channel = s
            .parse()
            .map_err(|_| format!("incorrect color string: {:?}", s))?;
    }
    Ok(Color::from_array(color))
}

pub fn handle_opts(
    opts: &[GetoptItem<'_>],
) -> Box<dyn ColorGenerator + Send + 'static> {
    let mut normal = false;
    // Invariant: This is either None, or a NON-EMPTY vec/slice
    let mut vectorsets = None;
    for opt in opts {
        match opt {
            GetoptItem::Opt { opt, arg: None } if opt.is_long("normal") => {
                normal = true
            }
            GetoptItem::Opt { opt, arg: None } if opt.is_long("hues") => {
                match vectorsets {
                    None => {
                        vectorsets = Some(Cow::Borrowed(FULL_INTENSITY_HUES))
                    }
                    Some(ref mut cow) => {
                        cow.to_mut().extend_from_slice(FULL_INTENSITY_HUES)
                    }
                }
            }
            GetoptItem::Opt { opt, arg: None }
                if opt.is_long("newvectorset") =>
            {
                match vectorsets {
                    None => {
                        vectorsets = Some(
                            vec![VectorSet {
                                start: Color::default(),
                                vectors: vec![].into(),
                                chance: ONE,
                                kind: VectorSetKind::Full,
                            }]
                            .into(),
                        )
                    }
                    Some(ref mut cow) => cow.to_mut().push(VectorSet {
                        start: Color::default(),
                        vectors: vec![].into(),
                        chance: ONE,
                        kind: VectorSetKind::Full,
                    }),
                }
            }
            GetoptItem::Opt { opt, arg: Some(vector) }
                if opt.is_long("vector") =>
            {
                let vector = parse_color(vector).expect("TODO: error handling");
                match vectorsets {
                    None => {
                        vectorsets = Some(
                            vec![VectorSet {
                                start: Color::default(),
                                vectors: vec![vector].into(),
                                chance: ONE,
                                kind: VectorSetKind::Full,
                            }]
                            .into(),
                        )
                    }
                    Some(ref mut cow) => {
                        let vectorset = cow
                            .to_mut()
                            .last_mut()
                            .expect("vectorsets should never be an empty vec");
                        vectorset.vectors.to_mut().push(vector);
                    }
                }
            }
            GetoptItem::Opt { opt, arg: Some(base) } if opt.is_long("base") => {
                let start = parse_color(base).expect("TODO: error handling");
                match vectorsets {
                    None => {
                        vectorsets = Some(
                            vec![VectorSet {
                                start,
                                vectors: Cow::Borrowed(&[]),
                                chance: ONE,
                                kind: VectorSetKind::Full,
                            }]
                            .into(),
                        )
                    }
                    Some(ref mut cow) => {
                        let vectorset = cow
                            .to_mut()
                            .last_mut()
                            .expect("vectorsets should never be an empty vec");
                        vectorset.start = start;
                    }
                }
            }
            GetoptItem::Opt { opt, arg: Some(r#type) }
                if opt.is_long("type") =>
            {
                let kind = match *r#type {
                    "full" | "f" => VectorSetKind::Full,
                    "sum_one" | "sumone" | "one" | "o" => VectorSetKind::SumOne,
                    "triangular" | "tri" | "t" => VectorSetKind::Triangular,
                    _ => panic!("TODO: error handling"),
                };
                match vectorsets {
                    None => {
                        vectorsets = Some(
                            vec![VectorSet {
                                start: Color::default(),
                                vectors: Cow::Borrowed(&[]),
                                chance: ONE,
                                kind,
                            }]
                            .into(),
                        )
                    }
                    Some(ref mut cow) => {
                        let vectorset = cow
                            .to_mut()
                            .last_mut()
                            .expect("vectorsets should never be an empty vec");
                        vectorset.kind = kind;
                    }
                }
            }
            _ => {}
        }
    }
    match (normal, vectorsets) {
        (true | false, None) => Box::new(&BASIC_COLOR), /* Default to basic */
        // if no colorspace
        // is given
        (false, Some(vectorsets)) => Box::new(
            VectorSetGroup::new(vectorsets).expect("vectorsets is not empty"),
        ),
        (true, Some(_)) => panic!("Must provide only one colorspace"),
    }
}

#[cfg(test)]
mod tests {
    use getopt::Getopt;

    use super::{
        from_3, Color, VectorSet, VectorSetGroup, VectorSetKind, BASIC_COLOR,
        FULL_INTENSITY_HUES, ONE,
    };

    #[test]
    fn basic_color_test() {
        let args_iter: [&[&str]; 3] = [&[], &["-N"], &["--normal"]];

        let getopt = Getopt::from_iter(super::opts()).unwrap();

        for args in args_iter {
            let opts = getopt
                .parse(args.iter().copied())
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            let should_be_normal = super::handle_opts(&opts);
            let should_be_normal = should_be_normal.as_vectorset().unwrap();
            assert_eq!(should_be_normal, &BASIC_COLOR);
        }
    }

    #[test]
    fn hues_test() {
        let args_iter: [&[&str]; 1] = [&["--hues"]];
        let expected = VectorSetGroup::new(FULL_INTENSITY_HUES.into()).unwrap();

        let getopt = Getopt::from_iter(super::opts()).unwrap();

        for args in args_iter {
            let opts = getopt
                .parse(args.iter().copied())
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            let should_be_hues = super::handle_opts(&opts);
            let should_be_hues = should_be_hues.as_vectorsetgroup().unwrap();
            assert_eq!(should_be_hues, &expected);
        }
    }

    #[test]
    fn vector_test() {
        let empty = VectorSetGroup::new(
            vec![VectorSet {
                start: Color::default(),
                vectors: vec![].into(),
                chance: ONE,
                kind: VectorSetKind::Full,
            }]
            .into(),
        )
        .unwrap();
        let redgreen = VectorSetGroup::new(
            vec![VectorSet {
                start: from_3(0.0, 0.0, 0.0),
                vectors: vec![from_3(1.0, 0.0, 0.0), from_3(0.0, 1.0, 0.0)]
                    .into(),
                chance: super::ONE,
                kind: super::VectorSetKind::Full,
            }]
            .into(),
        )
        .unwrap();
        #[rustfmt::skip]
        let args_iter: [(&[&str], &VectorSetGroup); 8] = [
            (&["-n"], &empty),
            (&["--newvectorset"], &empty),
            (&["-v1,0,0", "-v0,1,0"], &redgreen),
            (&["--vector=1.0,0,0.0", "-v0,1,0"], &redgreen),
            (&["--vector", "1.0,0,0.0", "-v", "0,1.000,00.0"], &redgreen),
            (&["-n", "-v1,0,0", "-v0,1,0"], &redgreen),
            (&["-n", "--vector=1.0,0,0.0", "-v0,1,0"], &redgreen),
            (&["-n", "-b0,0,0", "--vector", "1.0,0,0.0", "-v", "0,1.000,00.0"], &redgreen),
        ];

        let getopt = Getopt::from_iter(super::opts()).unwrap();

        for (args, expected) in args_iter {
            let opts = getopt
                .parse(args.iter().copied())
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            let should_be_expected = super::handle_opts(&opts);
            let should_be_expected =
                should_be_expected.as_vectorsetgroup().unwrap();
            assert_eq!(should_be_expected, expected);
        }
    }
}
