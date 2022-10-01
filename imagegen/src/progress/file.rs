use std::io::{Write, BufWriter};

pub struct FileProgressor<W: Write> {
    writer: BufWriter<W>,
}
