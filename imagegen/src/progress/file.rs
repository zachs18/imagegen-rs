use std::{io::{Write, BufWriter}, sync::{Arc, Mutex, atomic::Ordering}, pin::Pin};

use super::Progressor;

pub struct FileProgressor<W: Write> {
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
    fn make_supervised_progressor(&self) -> Box<dyn Send + for<'a> FnOnce(super::ProgressData, &'a super::ProgressSupervisorData<'a>) -> Pin<Box<dyn std::future::Future<Output = ()> + 'a>>> {
        let writer = self.writer.clone();

        Box::new(move |progress_data, common_data| {
            Box::pin(async move {
                let mut writer = writer.lock().unwrap();
                loop {
                    log::trace!(target: "barriers", "before progress barrier a");
                    common_data.progress_barrier.wait().await;
                    log::trace!(target: "barriers", "after progress barrier a");

                    let locked = common_data.locked.read().unwrap();
                    locked.image.write_to(&mut *writer).unwrap();
                    writer.flush().unwrap();

                    if common_data.finished.load(Ordering::SeqCst) {
                        break;
                    }
                    log::trace!(target: "barriers", "before progress barrier b");
                    common_data.progress_barrier.wait().await;
                    log::trace!(target: "barriers", "after progress barrier b");
                }
            })
        })
    }
}
