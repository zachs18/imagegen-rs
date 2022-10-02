use std::{sync::atomic::Ordering, future::Future, pin::Pin};

use super::{Progressor, ProgressData, ProgressSupervisorData};

pub struct TextProgressor<F: for<'a> FnMut(std::fmt::Arguments<'a>) + ?Sized> {
    callback: F,
}

impl<F: for<'a> FnMut(std::fmt::Arguments<'a>)> TextProgressor<F> {
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F: for<'a> FnMut(std::fmt::Arguments<'a>) + Sync + Send + ?Sized> Progressor for TextProgressor<F> {
    fn run_under_supervisor<'a>(&'a mut self, data: ProgressData, common_data: &'a ProgressSupervisorData<'a>) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let ProgressData { progress_interval, } = data;
            let ProgressSupervisorData {
                locked,
                progress_barrier,
                finished,
                pixels_placed,
                pixels_generated,
                size,
                ..
            } = &*common_data;
            let mut step_count = 0;
            let mut prev_edge_count = 0;
            loop {
                progress_barrier.wait().await;
                if finished.load(Ordering::SeqCst) {
                    // Only read this betwee barriers, so we know generator thread wont change it under us
                    break;
                }
                if step_count >= progress_interval {
                    step_count = 0;
                    if let Ok(guard) = locked.try_read() {
                        prev_edge_count = guard.edges.len();
                    }
                    let pixels_placed = pixels_placed.load(Ordering::SeqCst);
                    let pixels_generated = pixels_generated.load(Ordering::SeqCst);
                    let percent_done = 100.0 * pixels_placed as f64 / *size as f64;
                    (self.callback)(format_args!(
                        "Approximately {percent_done:4.1}% done ({progress_interval}, {prev_edge_count} edges, {pixels_placed} pixels placed, {pixels_generated} pixels generated)",
                    ));
                } else {
                    step_count += 1;
                }
                progress_barrier.wait().await;
            }
        })
    }
}
