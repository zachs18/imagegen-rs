use std::{num::NonZeroUsize, sync::{Arc, atomic::Ordering}, ops::Range, collections::VecDeque};

use bitmap::BitMap;
use getopt::{Opt, GetoptItem};
use rand::{RngCore, Rng, seq::SliceRandom};

use crate::{color::{ColorGenerator, Color, Channel}, pnmdata::PnmData, CommonData, CommonLockedData};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pixel {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Offset {
    pub dx: i32,
    pub dy: i32,
}

impl std::ops::Add<Offset> for Pixel {
    type Output = Pixel;

    fn add(mut self, rhs: Offset) -> Self::Output {
        self.x += rhs.dx;
        self.y += rhs.dy;
        self
    }
}

// TODO: somehow make fitness function configurable

#[derive(Clone)]
pub struct GeneratorData {
}
pub trait Generator: std::fmt::Debug {
    /// Caller should run this in a separate thread.
    fn generate(&mut self, data: GeneratorData, common_data: Arc<CommonData>, color_generator: &dyn ColorGenerator, rng: &mut dyn RngCore);

    #[cfg(test)]
    #[doc(hidden)]
    fn offsets(&self) -> &[Offset];
}

fn place_seeds_common(count: usize, dimx: NonZeroUsize, dimy: NonZeroUsize, data: &mut CommonLockedData, color_generator: &dyn ColorGenerator, rng: &mut dyn RngCore) -> Vec<Pixel> {
    log::trace!("placing {count} seeds");
    let mut placed = Vec::with_capacity(count);
    let mut failures = 0usize;
    let mut successes = 0usize;
    'outer: for _ in 0..count {
        'retry: loop {
            let y = rng.gen_range(0..dimy.get());
            let x = rng.gen_range(0..dimx.get());
            if data.placed_pixels.get((y, x)) {
                failures += 1;
                if failures >= 4 {
                    log::trace!("Failed to place seed 4 times");
                    break 'outer;
                }
                continue 'retry;
            }

            log::trace!("placing seed at ({x},{y})");

            data.image[(y, x)] = color_generator.new_color(rng);
            data.placed_pixels.set((y, x), true);
            placed.push(Pixel { x: x as _, y: y as _ });

            successes += 1;
            break 'retry;
        }
    }
    if successes < count {
        log::trace!("naive seeding failed {failures} times (it placed {successes} / {count}); using slower always-successful version");
        let mut all_empty = Vec::with_capacity(dimx.get());
        log::trace!("{} placed pixels according to bitmap", data.placed_pixels.count());
        data.placed_pixels.for_each_false(|row, col| {
            debug_assert!(!data.placed_pixels.get((row, col)));
            all_empty.push((row, col));
        });
        for &(y, x) in all_empty.choose_multiple(rng, count - successes) {
            log::trace!("placing seed at ({x},{y})");

            data.image[(y, x)] = color_generator.new_color(rng);
            data.placed_pixels.set((y, x), true);
            placed.push(Pixel { x: x as _, y: y as _ });

            successes += 1;
        }
    }
    placed
}

/// For inner generation, only one neighbor is considered for fitness.
/// Edges for inner generators are the actual placed pixels; when an edge is
/// found to be the "best" for a color, that color is placed adjacent to the edge
/// (and becomes an edge itself)
#[derive(Debug, Clone)]
struct InnerGenerator {
    seeds: NonZeroUsize,
    offsets: Vec<Offset>,
    workers: NonZeroUsize,
    colorcount: NonZeroUsize,
    maxfitness: Option<Channel>,
}

fn validate_inner_edges(dimy: NonZeroUsize, dimx: NonZeroUsize, edges: &mut VecDeque<Pixel>, placed_pixels: &BitMap, offsets: &[Offset]) {
    edges.retain(|pixel| {
        placed_pixels.get((pixel.y as usize, pixel.x as usize)) && {
            let mut any_neighbor_open = false;
            'offsets: for offset in offsets {
                // if let Some(canonical) = geometry.canonicalize(pixel + offset) {...}
                let y = pixel.y + offset.dy;
                if y < 0 || y as usize >= dimy.get() { continue 'offsets; }
                let x = pixel.x + offset.dx;
                if x < 0 || x as usize >= dimx.get() { continue 'offsets; }
                if !placed_pixels.get((y as usize, x as usize)) {
                    any_neighbor_open = true;
                    break 'offsets;
                }
            }
            any_neighbor_open
        }
    });
}

/// Chooses a neighbor to `pixel`, places `color` in the data at that location, sets it as placed in the bitmap, and adds it as an edge.
fn place_pixel_inner(dimy: NonZeroUsize, dimx: NonZeroUsize, pixel: Pixel, color: Color, image: &mut PnmData, edges: &mut VecDeque<Pixel>, placed_pixels: &mut BitMap, offsets: &[Offset]) -> Result<Pixel, ()> {
    for offset in offsets {
        let y = pixel.y + offset.dy;
        if y < 0 || y as usize >= dimy.get() { continue; }
        let x = pixel.x + offset.dx;
        if x < 0 || x as usize >= dimx.get() { continue; }
        let location = Pixel { y, x };
        let y = y as usize;
        let x = x as usize;
        if placed_pixels.get((y, x)) { continue; }
        placed_pixels.set((y, x), true);
        image[(y, x)] = color;
        edges.push_back(location);
        return Ok(location);
    }
    Err(())
}

impl Generator for InnerGenerator {
    fn generate(&mut self, data: GeneratorData, common_data: Arc<CommonData>, color_generator: &dyn ColorGenerator, rng: &mut dyn RngCore) {
        // Place seeds
        {
            let mut locked = common_data.locked.write().unwrap();
            let seed_locations = place_seeds_common(self.seeds.get(), common_data.dimx, common_data.dimy, &mut locked, color_generator, rng);
            common_data.pixels_generated.fetch_add(seed_locations.len(), Ordering::SeqCst);
            common_data.pixels_placed.fetch_add(seed_locations.len(), Ordering::SeqCst);
            locked.edges.extend(seed_locations);
        }

        let generate_colors = |color_generator: &dyn ColorGenerator, rng: &mut dyn RngCore| -> Arc<[Color]> {
            Arc::from_iter((0..self.colorcount.get()).map(|_| color_generator.new_color(rng)))
        };

        // Main loop
        if self.workers.get() == 1 {
            // Just do the calculation on this thread between the barriers.
            // Removes the need for channels/tokio.
            // log::error!("single-thread generator not yet implemented. Run with '-w 2' or above.");
            // todo!("single-thread generator main loop");

            loop {
                let mut best_places = vec![None; self.colorcount.get()];
                {
                    let mut locked = common_data.locked.write().unwrap();

                    // If there are no edges left, seed again
                    if locked.edges.len() == 0 {
                        log::trace!("re-seeding");
                        let seed_locations = place_seeds_common(1, common_data.dimx, common_data.dimy, &mut locked, color_generator, rng);
                        common_data.pixels_generated.fetch_add(seed_locations.len(), Ordering::SeqCst);
                        common_data.pixels_placed.fetch_add(seed_locations.len(), Ordering::SeqCst);
                        locked.edges.extend(seed_locations);
                    }
                }

                log::trace!(target: "barriers", "before progress barrier a");
                common_data.progress_barrier.wait();
                log::trace!(target: "barriers", "after progress barrier a");
                if common_data.finished.load(Ordering::SeqCst) {
                    break;
                }

                let colors = generate_colors(color_generator, rng);
                common_data.pixels_generated.fetch_add(colors.len(), Ordering::SeqCst);
                {
                    let CommonLockedData { image, placed_pixels, edges } = &*common_data.locked.read().unwrap();

                    for edge in 0..edges.len() {
                        let pixel@Pixel { x, y } = edges[edge];
                        // TODO: geometry
                        let x = x as usize;
                        let y = y as usize;

                        let color = image[(y, x)];
                        for (current_best, new_color) in best_places.iter_mut().zip(&*colors) {
                            // let fitness = fitness(*color, &image)
                            // TODO: configurable fitness function
                            let diff = color - new_color;
                            let sq_diff = diff * diff;
                            let fitness: Channel = sq_diff.as_array().iter().sum();
                            match current_best {
                                Some((_, current_fitness)) if *current_fitness < fitness => {},
                                _ => *current_best = Some((pixel, fitness)),
                            }
                        }
                    }
                }

                log::trace!(target: "barriers", "before progress barrier b");
                common_data.progress_barrier.wait();
                log::trace!(target: "barriers", "afterprogress barrier b");

                // Apply best_places
                let mut locked = common_data.locked.write().unwrap();
                let locked = &mut *locked;
                self.offsets.shuffle(rng);
                for (color, (pixel, _)) in colors.iter().zip(best_places).filter_map(|(color, best)| Some((color, best?))) {
                    // let Pixel { x, y } = pixel;
                    // // TODO: geometry
                    // let x = x as usize;
                    // let y = y as usize;

                    // locked.image[(y, x)] = *color;
                    // locked.placed_pixels.set((y, x), true);
                    if let Ok(_) = place_pixel_inner(common_data.dimy, common_data.dimx, pixel, *color, &mut locked.image, &mut locked.edges, &mut locked.placed_pixels, &self.offsets) {
                        common_data.pixels_placed.fetch_add(1, Ordering::SeqCst);
                    } else {
                        log::warn!("failed to place pixel at {pixel:?}");
                    }
                }
                if common_data.pixels_placed.load(Ordering::SeqCst) == common_data.size.get() {
                    common_data.finished.store(true, Ordering::SeqCst);
                    log::trace!("generator finished");
                } else {
                    validate_inner_edges(common_data.dimy, common_data.dimx, &mut locked.edges, &mut locked.placed_pixels, &self.offsets);
                }
            }
        } else {
            // Supervisor sends the colors to the worker, the worker calculates the best places,
            // the worker sends back the best places this worker saw with their fitness.
            struct WorkerData {
                colors_rx: tokio::sync::broadcast::Receiver<Arc<[Color]>>,
                edges_rx: tokio::sync::mpsc::Receiver<Range<usize>>,
                best_places_tx: tokio::sync::mpsc::Sender<Vec<Option<(Pixel, Channel)>>>,
                data: GeneratorData,
                common_data: Arc<CommonData>,
                generator: InnerGenerator, // TODO: something better than cloning self
            }
            let mut handles = Vec::with_capacity(self.workers.get());
            let mut edges_txs = Vec::with_capacity(self.workers.get());

            let (colors_tx, _) = tokio::sync::broadcast::channel(1);
            let (best_places_tx, mut best_places_rx) = tokio::sync::mpsc::channel(self.workers.get());

            for _ in 0..self.workers.get() {
                let (edges_tx, edges_rx) = tokio::sync::mpsc::channel(1);
                edges_txs.push(edges_tx);
                let data = WorkerData {
                    edges_rx,
                    colors_rx: colors_tx.subscribe(),
                    best_places_tx: best_places_tx.clone(),
                    data: data.clone(),
                    common_data: common_data.clone(),
                    generator: self.clone(),
                };
                handles.push(std::thread::spawn(move || {
                    let mut data = data;
                    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
                    rt.block_on(async move {
                        while !data.common_data.finished.load(Ordering::SeqCst) {
                            log::warn!("TODO: handle RecvError::Closed as supervisor thread exiting");
                            let colors = match data.colors_rx.recv().await {
                                Ok(colors) => colors,
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                    log::warn!("Worker exiting because supervisor closed colors channel");
                                    break;
                                },
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(..)) => {
                                    log::error!("Worker error: colors channel lagged");
                                    unreachable!("colors channel lagged");
                                }
                            };
                            // Calculate best places for each color in this worker's edge chunk
                            let mut best_places = vec![None; data.generator.colorcount.get()];
                            {
                                let locked = data.common_data.locked.read().unwrap();
                                let CommonLockedData { image, placed_pixels, edges } = &*locked;

                                log::trace!("recv'ing edge range");
                                let my_edges = data.edges_rx.recv().await.expect("supervisor thread exited?");
                                log::trace!("recv'd edge range: {my_edges:?}");

                                for edge in my_edges {
                                    let pixel@Pixel { x, y } = edges[edge];
                                    // TODO: geometry
                                    let x = x as usize;
                                    let y = y as usize;

                                    let color = image[(y, x)];
                                    for (current_best, new_color) in best_places.iter_mut().zip(&*colors) {
                                        // let fitness = fitness(*color, &image)
                                        // TODO: configurable fitness function
                                        let diff = color - new_color;
                                        let sq_diff = diff * diff;
                                        let fitness: Channel = sq_diff.as_array().iter().sum();
                                        match current_best {
                                            Some((_, current_fitness)) if *current_fitness < fitness => {},
                                            _ => *current_best = Some((pixel, fitness)),
                                        }
                                    }
                                }
                            }
                            data.best_places_tx.send(best_places).await.expect("supervisor thread exited?");
                        }
                    });
                }));
            }

            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();

            rt.block_on(async {
                loop {
                    let mut best_places = vec![None; self.colorcount.get()];
                    {
                        let mut locked = common_data.locked.write().unwrap();

                        // If there are no edges left, seed again
                        if locked.edges.len() == 0 {
                            log::trace!("re-seeding");
                            let seed_locations = place_seeds_common(1, common_data.dimx, common_data.dimy, &mut locked, color_generator, rng);
                            common_data.pixels_generated.fetch_add(seed_locations.len(), Ordering::SeqCst);
                            common_data.pixels_placed.fetch_add(seed_locations.len(), Ordering::SeqCst);
                            locked.edges.extend(seed_locations);
                        }
                    }
                    {
                        let locked = common_data.locked.read().unwrap();

                        log::trace!(target: "barriers", "before progress barrier a");
                        common_data.progress_barrier.wait();
                        log::trace!(target: "barriers", "afterprogress barrier a");
                        if common_data.finished.load(Ordering::SeqCst) {
                            break;
                        }

                        let edgecount = locked.edges.len();
                        let step = edgecount / edges_txs.len();
                        log::trace!("sending edge ranges: {} (slices of {:?}", edges_txs.len(), 0..edgecount);
                        for (w, tx) in edges_txs.iter_mut().enumerate() {
                            let range = if w == self.workers.get() - 1 {
                                w * step .. edgecount
                            } else {
                                w * step .. (w+1) * step
                            };
                            log::trace!("sending edge range {w}: {range:?}");
                            tx.send(range).await.expect("worker exited?");
                        }
                    }
                    let colors = generate_colors(color_generator, rng);
                    common_data.pixels_generated.fetch_add(colors.len(), Ordering::SeqCst);
                    log::trace!("sending colors");
                    colors_tx.send(colors.clone()).expect("Worker threads should be running");

                    // Ensure all progressors have read what they need to
                    log::trace!(target: "barriers", "before progress barrier b");
                    common_data.progress_barrier.wait();
                    log::trace!(target: "barriers", "afterprogress barrier b");

                    // Wait for workers (happens at best_places_rx.recv())
                    // Coalesce worker results into best_places
                    for _ in 0..self.workers.get() {
                        let best_places_recvd = best_places_rx.recv().await.expect("worker thread exited early?");
                        debug_assert!(best_places_recvd.len() == best_places.len(), "worker returned wrong length?");
                        for (best, worker) in best_places.iter_mut().zip(best_places_recvd) {
                            match (&*best, &worker, self.maxfitness) {
                                (_, None, _) => { /* do nothing */},
                                (None, Some(_), None) => *best = worker,
                                (None, Some((_, fitness)), Some(maxfitness)) => if *fitness < maxfitness {
                                    *best = worker
                                },
                                (Some((_, bfitness)), Some((_, wfitness)), _) => {
                                    // Don't need to check maxfitness, since the best fitness already satisfies that
                                    if wfitness < bfitness {
                                        *best = worker;
                                    }
                                },
                            }
                        }
                    }

                    log::trace!("best_places = {best_places:?}");

                    // Apply best_places
                    let mut locked = common_data.locked.write().unwrap();
                    let locked = &mut *locked;
                    self.offsets.shuffle(rng);
                    for (color, (pixel, _)) in colors.iter().zip(best_places).filter_map(|(color, best)| Some((color, best?))) {
                        // let Pixel { x, y } = pixel;
                        // // TODO: geometry
                        // let x = x as usize;
                        // let y = y as usize;

                        // locked.image[(y, x)] = *color;
                        // locked.placed_pixels.set((y, x), true);
                        if let Ok(_) = place_pixel_inner(common_data.dimy, common_data.dimx, pixel, *color, &mut locked.image, &mut locked.edges, &mut locked.placed_pixels, &self.offsets) {
                            common_data.pixels_placed.fetch_add(1, Ordering::SeqCst);
                        } else {
                            log::warn!("failed to place pixel at {pixel:?}");
                        }
                    }
                    if common_data.pixels_placed.load(Ordering::SeqCst) == common_data.size.get() {
                        common_data.finished.store(true, Ordering::SeqCst);
                        log::trace!("generator finished");
                    } else {
                        validate_inner_edges(common_data.dimy, common_data.dimx, &mut locked.edges, &mut locked.placed_pixels, &self.offsets);
                    }
                }
            });
            drop(colors_tx);
            for handle in handles {
                handle.join().unwrap_or_else(|err| log::error!("Worker panicked: {err:?}"));
            }
        }
    }

    #[cfg(test)]
    #[doc(hidden)]
    fn offsets(&self) -> &[Offset] {
        &self.offsets
    }
}

#[derive(Default)]
pub struct GeneratorSettings {
    // Generator settings
    seeds: Option<NonZeroUsize>,
    offsets: Option<Vec<Offset>>,
    workers: Option<NonZeroUsize>,
    colorcount: Option<NonZeroUsize>,
    maxfitness: Option<Channel>,
    outer: Option<bool>,
}

const NORMAL_OFFSETS: &[Offset] = &[
    Offset { dx: -1, dy: -1 },
    Offset { dx: -1, dy: 0 },
    Offset { dx: -1, dy: 1 },
    Offset { dx: 0, dy: -1 },
    Offset { dx: 0, dy: 1 },
    Offset { dx: 1, dy: -1 },
    Offset { dx: 1, dy: 0 },
    Offset { dx: 1, dy: 1 },
];

const ORTHOGONAL_OFFSETS: &[Offset] = &[
    Offset { dx: -1, dy: 0 },
    Offset { dx: 0, dy: -1 },
    Offset { dx: 0, dy: 1 },
    Offset { dx: 1, dy: 0 },
];

const DIAGONAL_OFFSETS: &[Offset] = &[
    Offset { dx: -1, dy: -1 },
    Offset { dx: -1, dy: 1 },
    Offset { dx: 1, dy: -1 },
    Offset { dx: 1, dy: 1 },
];

const KNIGHT_OFFSETS: &[Offset] = &[
    Offset { dx: -2, dy: -1 },
    Offset { dx: -1, dy: -2 },
    Offset { dx: -2, dy: 1 },
    Offset { dx: -1, dy: 2 },
    Offset { dx: 2, dy: -1 },
    Offset { dx: 1, dy: -2 },
    Offset { dx: 2, dy: 1 },
    Offset { dx: 1, dy: 2 },
];

lazy_static::lazy_static!{
    static ref OFFSET_REGEX: regex::Regex = regex::Regex::new(
        r#"^(-?[0-9]+),(-?[0-9]+)$"#
    ).expect("valid regex");
}

pub fn opts() -> impl IntoIterator<Item = Opt> {
    [
        Opt::short_long('e', "seeds", getopt::HasArgument::Yes),
        Opt::short_long('O', "offsets", getopt::HasArgument::Yes),
        Opt::short_long('w', "workers", getopt::HasArgument::Yes),
        Opt::short_long('C', "colorcount", getopt::HasArgument::Yes),
        Opt::long("maxfitness", getopt::HasArgument::Yes),
        Opt::long("outer", getopt::HasArgument::No),
    ]
}

pub fn handle_opts(opts: &[GetoptItem]) -> Box<dyn Generator + Send> {
    let mut settings = GeneratorSettings::default();

    macro_rules! set {
        ($field:ident) => {
            let $field = $field.parse().expect(&format!("{:?} is not a valid {} value", $field, stringify!($field)));
            match &mut settings.$field {
                Some(_) => panic!("multiple {} values specified", stringify!($field)),
                None => settings.$field = Some($field),
            }
        };
    }

    macro_rules! add_offsets {
        ($offsets:expr) => {
            match &mut settings.offsets {
                Some(offsets) => offsets.extend_from_slice(&$offsets[..]),
                None => settings.offsets = Some(Vec::from(&$offsets[..])),
            }
        };
    }

    for opt in opts {
        match opt {
            GetoptItem::Opt { opt, arg: Some(seeds) } if opt.long.as_deref() == Some("seeds") => {
                set!(seeds);
            },
            GetoptItem::Opt { opt, arg: Some(offset) } if opt.long.as_deref() == Some("offsets") => {
                match *offset {
                    "n" => add_offsets!(NORMAL_OFFSETS),
                    "o" => add_offsets!(ORTHOGONAL_OFFSETS),
                    "d" => add_offsets!(DIAGONAL_OFFSETS),
                    "k" => add_offsets!(KNIGHT_OFFSETS),
                    _ => if let Some(captures) = OFFSET_REGEX.captures(offset) {
                        match (captures.get(1).and_then(|mtch| mtch.as_str().parse().ok()), captures.get(2).and_then(|mtch| mtch.as_str().parse().ok())) {
                            (None, None) => todo!(),
                            (None, Some(_)) => todo!(),
                            (Some(_), None) => todo!(),
                            (Some(dx), Some(dy)) => add_offsets!([Offset { dx, dy }]),
                        }
                    } else {
                        todo!("error");
                    },
                }
            },
            GetoptItem::Opt { opt, arg: Some(workers) } if opt.long.as_deref() == Some("workers") => {
                set!(workers);
            },
            GetoptItem::Opt { opt, arg: Some(colorcount) } if opt.long.as_deref() == Some("colorcount") => {
                set!(colorcount);
            },
            GetoptItem::Opt { opt, arg: Some(maxfitness) } if opt.long.as_deref() == Some("maxfitness") => {
                set!(maxfitness);
            },
            GetoptItem::Opt { opt, arg: None } if opt.long.as_deref() == Some("outer") => {
                todo!("figure out wait handling")
            },
            _ => {},
        }
    }
    match settings.outer {
        Some(true) => todo!(),
        Some(false) | None => Box::new(InnerGenerator {
            seeds: settings.seeds.unwrap_or(NonZeroUsize::new(1).unwrap()),
            offsets: settings.offsets.unwrap_or_else(|| Vec::from(NORMAL_OFFSETS)),
            workers: settings.workers.unwrap_or(NonZeroUsize::new(1).unwrap()),
            colorcount: settings.colorcount.unwrap_or(NonZeroUsize::new(1).unwrap()),
            maxfitness: settings.maxfitness,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use getopt::Getopt;

    use super::{NORMAL_OFFSETS, ORTHOGONAL_OFFSETS, DIAGONAL_OFFSETS, KNIGHT_OFFSETS, Offset};

    #[test]
    fn basic_offsets() {
        let args_iter: [(&[&str], Cow<[Offset]>); 9] = [
            (&[], NORMAL_OFFSETS.into()),
            (&["-On"], NORMAL_OFFSETS.into()),
            (&["-Oo"], ORTHOGONAL_OFFSETS.into()),
            (&["-Od"], DIAGONAL_OFFSETS.into()),
            (&["-Ok"], KNIGHT_OFFSETS.into()),
            (&["-O", "k"], KNIGHT_OFFSETS.into()),
            (&["-O3,3"], Cow::Borrowed(&[Offset { dx: 3, dy: 3 }])),
            (&["-O", "n", "-O-3,-3"], NORMAL_OFFSETS.iter().copied().chain(std::iter::once(Offset { dx: -3, dy: -3 })).collect()),
            (&["-O", "n", "-O", "-3,-3"], NORMAL_OFFSETS.iter().copied().chain(std::iter::once(Offset { dx: -3, dy: -3 })).collect()),
        ];

        let getopt = Getopt::from_iter(super::opts()).unwrap();

        for (args, expected) in args_iter {
            let opts = getopt.parse(args.iter().copied()).collect::<Result<Vec<_>,_>>().unwrap();

            let should_be_normal = super::handle_opts(&opts);
            assert_eq!(should_be_normal.offsets(), &*expected);
        }
    }
}
