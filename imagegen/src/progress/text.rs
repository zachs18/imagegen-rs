use std::{fmt::Write, sync::{atomic::Ordering, Arc}};

use crate::CommonData;

use super::{Progressor, ProgressData};

pub struct TextProgressor<F: for<'a> FnMut(std::fmt::Arguments<'a>) + ?Sized> {
    callback: F,
}

impl<F: for<'a> FnMut(std::fmt::Arguments<'a>)> TextProgressor<F> {
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F: for<'a> FnMut(std::fmt::Arguments<'a>) + ?Sized> Progressor for TextProgressor<F> {
    fn run(&mut self, data: ProgressData, common_data: Arc<CommonData>) -> () {
        let ProgressData { progress_interval, } = data;
        let CommonData { locked, progress_barrier, finished, pixels_placed, pixels_generated, height, width, rng_seed, size } = &*common_data;
        let mut step_count = 0;
        let mut prev_edge_count = 0;
        while !finished.load(Ordering::SeqCst) {
            // TODO: barrier before load finished to prevent inconsistent data
            progress_barrier.wait();
            if step_count >= progress_interval {
                step_count = 0;
                if let Ok(guard) = locked.try_read() {
                    prev_edge_count = guard.edges.len();
                }
                let pixels_placed = pixels_placed.load(Ordering::Relaxed);
                let pixels_generated = pixels_generated.load(Ordering::Relaxed);
                let percent_done = 100.0 * pixels_placed as f64 / *size as f64;
                (self.callback)(format_args!(
                    "Approximately {percent_done:4.1}% done ({progress_interval}, {prev_edge_count} edges, {pixels_placed} pixels placed, {pixels_generated} pixels generated)",
                ));
            } else {
                step_count += 1;
            }
        }
    }
}
