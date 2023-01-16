use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::Ordering, Arc},
};

use super::{ProgressData, ProgressSupervisorData, Progressor};

pub struct TextProgressor<F: for<'a> FnMut(std::fmt::Arguments<'a>) + ?Sized> {
    callback: Arc<F>,
}

impl<F: for<'a> Fn(std::fmt::Arguments<'a>)> TextProgressor<F> {
    pub fn new(callback: F) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }
}

impl<F: for<'a> Fn(std::fmt::Arguments<'a>) + Sync + Send + ?Sized + 'static> Progressor
    for TextProgressor<F>
{
    fn make_supervised_progressor(
        &self,
    ) -> Box<
        dyn Send
            + for<'a> FnOnce(
                ProgressData,
                &'a ProgressSupervisorData<'a>,
            ) -> Pin<Box<dyn Future<Output = ()> + 'a>>,
    > {
        Box::new({
            let callback = self.callback.clone();
            move |progress_data, common_data| {
                Box::pin(async move {
                    let ProgressData {
                        progress_interval, ..
                    } = progress_data;
                    let ProgressSupervisorData {
                        locked,
                        ref progress_barrier,
                        finished,
                        pixels_placed,
                        pixels_generated,
                        size,
                        ..
                    } = *common_data;
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
                            let percent_done = 100.0 * pixels_placed as f64 / size.get() as f64;
                            callback(format_args!(
                                "Approximately {percent_done:4.1}% done ({progress_interval}, {prev_edge_count} edges, {pixels_placed} pixels placed, {pixels_generated} pixels generated)",
                            ));
                        } else {
                            step_count += 1;
                        }
                        progress_barrier.wait().await;
                    }
                })
            }
        })
    }
}
