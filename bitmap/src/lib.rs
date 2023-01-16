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
        if row >= self.height || col >= self.width {
            panic!("index out of range");
        }
        let byte_idx = row * self.stride + col / 8;
        let bit_idx = col % 8;
        (self.data[byte_idx] & (1 << bit_idx)) != 0
    }

    pub fn set(&mut self, (row, col): (usize, usize), value: bool) {
        if row >= self.height || col >= self.width {
            panic!("index out of range");
        }
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

    /// Calls `f` with each index whose bit is `true` (row, col)
    pub fn for_each_true(&self, mut f: impl FnMut(usize, usize)) {
        for row in 0..self.height {
            let byte_range = row * self.stride..(row + 1) * self.stride;
            for (byte_col, byte) in self.data[byte_range].iter().enumerate() {
                for bit_col in 0..8 {
                    let col = (byte_col << 3) | bit_col;
                    if byte & (1 << bit_col) != 0 && col < self.width {
                        f(row, col);
                    }
                }
            }
        }
    }

    /// Calls `f` with each index whose bit is `false` (row, col)
    pub fn for_each_false(&self, mut f: impl FnMut(usize, usize)) {
        'rows: for row in 0..self.height {
            let start_byte = row * self.stride;
            for (byte_col, byte) in self.data[start_byte..][..self.stride].iter().enumerate() {
                for bit_col in 0..8 {
                    let col = (byte_col << 3) | bit_col;
                    if col >= self.width {
                        continue 'rows;
                    }
                    if byte & (1 << bit_col) == 0 {
                        f(row, col);
                    }
                }
            }
        }
    }

    pub fn count(&self) -> usize {
        let mut count = 0;
        'rows: for row in 0..self.height {
            let start_byte = row * self.stride;
            for (byte_col, byte) in self.data[start_byte..][..self.stride].iter().enumerate() {
                if byte_col * 8 + 7 < self.width {
                    count += byte.count_ones() as usize;
                } else {
                    for bit_col in 0..8 {
                        let col = (byte_col << 3) | bit_col;
                        if col >= self.width {
                            continue 'rows;
                        }
                        if byte & (1 << bit_col) != 0 {
                            count += 1;
                        }
                    }
                }
            }
        }
        count
    }
}
