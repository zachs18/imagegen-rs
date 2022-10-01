use crate::color::{Color, Channel};

pub struct PnmData {
    pub dimx: u32,
    pub dimy: u32,
    pub maxval: u32,
    pub depth: u32,
    pub comments: Vec<String>,
    pub rawdata: Vec<Color>,
}

impl std::ops::Index<(usize, usize)> for PnmData {
    type Output = Color;

    fn index(&self, (y, x): (usize, usize)) -> &Self::Output {
        // TODO: bounds check?
        let idx = y * (self.dimx as usize) + x;
        &self.rawdata[idx]
    }
}


impl std::ops::IndexMut<(usize, usize)> for PnmData {
    fn index_mut(&mut self, (y, x): (usize, usize)) -> &mut Self::Output {
        let idx = y * (self.dimx as usize) + x;
        &mut self.rawdata[idx]
    }
}

impl PnmData {
    pub fn write_to<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        if self.maxval > 255 {
            todo!("16-bit pnm");
        }
        if self.depth != 3 {
            todo!("non-ppm pnm");
        }
        writeln!(writer, "P6")?;
        writeln!(writer, "{} {}", self.dimx, self.dimy)?;
        write!(writer, "{}\n", self.maxval)?;

        let maxval = self.maxval as Channel;

        let to_bytes = |color: Color| {
            let a = color * Color::splat(maxval);
            a.cast::<u8>()
        };

        for &color in &self.rawdata {
            let bytes = to_bytes(color);
            writer.write_all(&bytes.as_array()[..3])?;
        }

        Ok(())
    }
}
