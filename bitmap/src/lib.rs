/// A 2-D bitmap
pub struct BitMap {
    /// Packed 8-bits to a byte, with each row starting on a new byte
    data: Vec<u8>,
    /// Size of a row in bytes
    stride: usize,
    /// In bits
    width: usize,
    height: usize,
}

impl BitMap {
    /// Initialized to all false
    pub fn new(height: usize, width: usize) -> Option<Self> {
        let _size_check = height.checked_mul(width)?;
        let stride = width.checked_add(7)? / 8;
        let byte_size = height.checked_mul(stride)?;
        Some(Self {
            data: vec![0u8; byte_size],
            stride,
            width,
            height,
        })
    }

    pub fn get(&self, (row, col): (usize, usize)) -> bool {
        if row >= self.height || col >= self.width { panic!("index out of range"); }
        let byte_idx = row * self.stride + col / 8;
        let bit_idx = col % 8;
        (self.data[byte_idx] & (1 << bit_idx)) != 0
    }

    pub fn set(&mut self, (row, col): (usize, usize), value: bool) {
        if row >= self.height || col >= self.width { panic!("index out of range"); }
        let byte_idx = row * self.stride + col / 8;
        let bit_idx = col % 8;
        let byte = &mut self.data[byte_idx];
        if value {
            *byte |= 1 << bit_idx;
        } else {
            *byte &= !(1 << bit_idx);
        }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.height, self.width)
    }
}
