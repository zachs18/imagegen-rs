use std::{
    io::{BufWriter, Write},
    pin::Pin,
    sync::{atomic::Ordering, Arc, Mutex},
};

use super::{ProgressData, ProgressSupervisorData, Progressor};

pub struct FileProgressor<W: Write> {
    /// TODO: use tokio AsyncWrite
    writer: Arc<Mutex<BufWriter<W>>>,
}

impl<W: Write> FileProgressor<W> {
    pub fn new(writer: W) -> Self {
        FileProgressor {
            writer: Arc::new(Mutex::new(BufWriter::new(writer))),
        }
    }
}

impl<W: Write + Send + 'static> Progressor for FileProgressor<W> {
    fn make_supervised_progressor(
        &self,
    ) -> Box<
        dyn Send
            + for<'a> FnOnce(
                super::ProgressData,
                &'a super::ProgressSupervisorData<'a>,
            ) -> Pin<Box<dyn std::future::Future<Output = ()> + 'a>>,
    > {
        let writer = self.writer.clone();

        Box::new(move |progress_data, common_data| {
            Box::pin(async move {
                let ProgressData {
                    progress_interval,
                    progress_count,
                } = progress_data;
                let ProgressSupervisorData {
                    locked,
                    ref progress_barrier,
                    finished,
                    ..
                } = *common_data;
                let mut writer = writer.lock().unwrap();
                let mut step_count = 0;
                loop {
                    log::trace!(target: "barriers", "before progress barrier a");
                    progress_barrier.wait().await;
                    log::trace!(target: "barriers", "after progress barrier a");

                    if step_count >= progress_interval {
                        step_count = 0;
                        let locked = locked.read().unwrap();
                        locked.image.write_to(&mut *writer).unwrap();
                        writer.flush().unwrap();
                    } else {
                        step_count += 1;
                    }

                    if finished.load(Ordering::SeqCst) {
                        break;
                    }
                    log::trace!(target: "barriers", "before progress barrier b");
                    progress_barrier.wait().await;
                    log::trace!(target: "barriers", "after progress barrier b");
                }
                let locked = locked.read().unwrap();
                locked.image.write_to(&mut *writer).unwrap();
                writer.flush().unwrap();
                let mut data = vec![];
                locked.image.write_to(&mut data).unwrap();
                for _ in 0..progress_count {
                    writer.write_all(&data).unwrap();
                }
            })
        })
    }
}
