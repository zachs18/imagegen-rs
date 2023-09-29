#![deny(unsafe_op_in_unsafe_fn)]
use std::{
    marker::PhantomData,
    ops::{Range, RangeBounds},
    ptr::NonNull,
    sync::atomic::AtomicU8,
};

use aliasing::{Aliasing, UnaliasedAliasing, UnaliasedInnerBytesAliasing};
use copy_range::CopyRange;
use either::Either;
use mutability::{ConstMutability, MutMutability, Mutability};

pub use aliasing::{
    Aliased, AliasedEdgesOnly, AliasedNoEdges, JustAnEdge, Unaliased,
    UnaliasedNoEdges,
};
pub use mutability::{ConstSync, ConstUnsync, MutableSync, MutableUnsync};

macro_rules! transmute {
    ($self:ident as BitMapView) => {
        BitMapView {
            data: $self.data,
            stride: $self.stride,
            columns: $self.columns.clone(),
            rows: $self.rows.clone(),
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    };
    ($self:ident as BaseBitSlice) => {
        BaseBitSlice {
            data: $self.data,
            bits: $self.bits.clone(),
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    };
}

pub mod aliasing;
pub mod mutability;

/// A 2-D bitmap
pub struct BitMap {
    /// Packed 8-bits to a byte, with each row starting on a new byte
    data: Vec<u8>,
    /// Size of a row in bytes. Must be `>= self.width.div_ceil(8)`
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
        Some(Self { data: vec![0u8; byte_size], stride, width, height })
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
            for (byte_col, byte) in
                self.data[start_byte..][..self.stride].iter().enumerate()
            {
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

    pub fn count_ones(&self) -> usize {
        let mut count_ones = 0;
        'rows: for row in 0..self.height {
            let start_byte = row * self.stride;
            for (byte_col, byte) in
                self.data[start_byte..][..self.stride].iter().enumerate()
            {
                if byte_col * 8 + 7 < self.width {
                    count_ones += byte.count_ones() as usize;
                } else {
                    for bit_col in 0..8 {
                        let col = (byte_col << 3) | bit_col;
                        if col >= self.width {
                            continue 'rows;
                        }
                        if byte & (1 << bit_col) != 0 {
                            count_ones += 1;
                        }
                    }
                }
            }
        }
        count_ones
    }

    pub fn as_view_ref<M: ConstMutability>(
        &self,
    ) -> BitMapView<'_, M, Unaliased> {
        BitMapView {
            data: NonNull::from(&self.data[..]).cast(),
            stride: self.stride,
            columns: CopyRange::from(0..self.width),
            rows: CopyRange::from(0..self.height),
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }

    pub fn as_view_mut<M: MutMutability>(
        &mut self,
    ) -> BitMapView<'_, M, Unaliased> {
        BitMapView {
            data: NonNull::from(&mut self.data[..]).cast(),
            stride: self.stride,
            columns: CopyRange::from(0..self.width),
            rows: CopyRange::from(0..self.height),
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ByteBitRange {
    pub start: u8,
    pub end: u8,
}

impl ByteBitRange {
    pub const ALL: Self = Self { start: 0, end: 8 };

    fn empty() -> ByteBitRange {
        ByteBitRange { start: 0, end: 0 }
    }
}

impl From<Range<u8>> for ByteBitRange {
    fn from(value: Range<u8>) -> Self {
        Self { start: value.start, end: value.end }
    }
}

impl ByteBitRange {
    pub const fn is_empty(&self) -> bool {
        self.mask() == 0
    }

    pub const fn len(&self) -> usize {
        self.mask().count_ones() as usize
    }

    pub fn pop_first(&mut self) -> Option<u8> {
        if !self.is_empty() {
            let value = self.start;
            self.start += 1;
            Some(value)
        } else {
            None
        }
    }

    pub fn pop_last(&mut self) -> Option<u8> {
        if !self.is_empty() {
            self.end -= 1;
            Some(self.end)
        } else {
            None
        }
    }

    pub const fn mask(&self) -> u8 {
        if self.start >= self.end || self.start >= 8 {
            0
        } else if self.end >= 8 {
            0b11111111 << self.start
        } else {
            let ByteBitRange { start, end } = *self;

            ((255u8 << start) << (8 - end)) >> (8 - end)
        }
    }

    pub fn from_mask(mask: u8) -> Option<Self> {
        if mask == 0 {
            return Some(Self { start: 0, end: 0 });
        }
        if mask.leading_zeros() + mask.count_ones() + mask.trailing_zeros()
            != u8::BITS
        {
            return None;
        }
        Some(Self {
            start: mask.trailing_zeros() as u8,
            end: 8 - mask.leading_zeros() as u8,
        })
    }
}

/// A reference to a slice of contiguous bits.
///
/// The `M` and `A` type parameters control how the bits can be acccessed.
#[derive(Debug, Clone, Copy)]
pub struct BaseBitSlice<'a, M: Mutability, A: Aliasing> {
    data: NonNull<u8>,
    bits: CopyRange<usize>,
    _lifetime: PhantomData<&'a ()>,
    _mutability: PhantomData<M>,
    _edge_aliasing: PhantomData<A>,
}

unsafe impl<M: Mutability + Send + Sync, A: Aliasing> Send
    for BaseBitSlice<'_, M, A>
{
}
unsafe impl<M: Mutability + Send + Sync, A: Aliasing> Sync
    for BaseBitSlice<'_, M, A>
{
}

impl<'a, M: Mutability, A: Aliasing> Default for BaseBitSlice<'a, M, A> {
    fn default() -> Self {
        Self::empty()
    }
}

struct RawBytes<'a, M: Mutability, A: Aliasing> {
    inner: BaseBitSlice<'a, M, A>,
}

impl<'a, M: Mutability, A: Aliasing> Default for RawBytes<'a, M, A> {
    fn default() -> Self {
        Self { inner: BaseBitSlice::empty() }
    }
}

impl<'a, M: Mutability, A: Aliasing> Iterator for RawBytes<'a, M, A> {
    type Item = (*mut u8, ByteBitRange);

    fn next(&mut self) -> Option<Self::Item> {
        if self.inner.bits.is_empty() {
            return None;
        }
        let start_byte_idx = self.inner.bits.start / 8;
        let start_bit_idx = (self.inner.bits.start % 8) as u8;
        let end_bit_idx = ((self.inner.bits.end - 1) % 8 + 1) as u8;

        self.inner.bits.start = (start_byte_idx + 1) * 8;

        let ptr = self.inner.data.as_ptr().wrapping_add(start_byte_idx);

        if self.inner.bits.is_empty() {
            self.inner.bits.start = self.inner.bits.end;
            Some((ptr, ByteBitRange::from(start_bit_idx..end_bit_idx)))
        } else {
            Some((ptr, ByteBitRange::from(start_bit_idx..8)))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.inner.bits.is_empty() {
            (0, Some(0))
        } else {
            let start_byte_idx = self.inner.bits.start / 8;
            let start_bit_idx = (self.inner.bits.start % 8) as u8;
            let end_byte_idx = self.inner.bits.start / 8;
            let end_bit_idx = (self.inner.bits.end % 8) as u8;
            let count = if end_bit_idx == 0 {
                end_byte_idx - start_byte_idx
            } else {
                end_byte_idx - start_byte_idx + 1
            };
            (count, Some(count))
        }
    }
}

impl<'a, M: Mutability, A: Aliasing> DoubleEndedIterator
    for RawBytes<'a, M, A>
{
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.inner.bits.is_empty() {
            return None;
        }
        let start_bit_idx = (self.inner.bits.start % 8) as u8;
        let end_byte_idx = (self.inner.bits.end - 1) / 8;
        let end_bit_idx = ((self.inner.bits.end - 1) % 8 + 1) as u8;

        self.inner.bits.end = end_byte_idx * 8;

        let ptr = self.inner.data.as_ptr().wrapping_add(end_byte_idx);

        if self.inner.bits.is_empty() {
            self.inner.bits.end = self.inner.bits.start;
            Some((ptr, ByteBitRange::from(start_bit_idx..end_bit_idx)))
        } else {
            Some((ptr, ByteBitRange::from(0..end_bit_idx)))
        }
    }
}

impl<'a, M: Mutability, A: Aliasing> BaseBitSlice<'a, M, A> {
    pub const fn empty() -> Self {
        Self {
            data: NonNull::dangling(),
            bits: CopyRange { start: 0, end: 0 },
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }

    fn into_raw_bytes(self) -> RawBytes<'a, M, A> {
        RawBytes { inner: self }
    }

    fn raw_bytes(&self) -> RawBytes<'_, M::Const, A> {
        RawBytes { inner: self.reborrow() }
    }

    pub fn len(&self) -> usize {
        self.bits.len()
    }

    pub fn into_const(self) -> BaseBitSlice<'a, M::Const, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn reborrow(&self) -> BaseBitSlice<'_, M::Const, A> {
        transmute!(self as BaseBitSlice)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned bit slice or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked<'b>(
        &self,
    ) -> BaseBitSlice<'b, M::Const, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn into_bits(self) -> Bits<'a, M, A> {
        let mut bits = Bits::default();
        bits.inner = self.into_raw_bytes();
        bits
    }

    pub fn bits(&self) -> Bits<'_, M::Const, A> {
        self.reborrow().into_bits()
    }

    /// Splits this bitslice into edges and a byte-aligned middle.
    ///
    /// Edge cases (no pun intended):
    ///
    /// * If this slice consists only of one partially-referenced byte, it will
    ///   be returned in the first edge `Option`, the middle part will be empty,
    ///   and the last edge `Option` will be `None`.
    #[doc(alias = "split_edges_mut")]
    pub fn split_edges(
        self,
    ) -> (
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
        BaseBitSlice<'a, M, A::NoEdges>,
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
    ) {
        match (self.bits.start % 8, self.bits.end % 8) {
            (0, 0) => (None, transmute!(self as BaseBitSlice), None),
            (start_bit_idx, 0) => {
                let mut first = transmute!(self as BaseBitSlice);
                let mut middle = transmute!(self as BaseBitSlice);
                middle.bits.start += 8 - start_bit_idx;
                first.bits.end = middle.bits.start;
                (Some(first), middle, None)
            }
            (0, end_bit_idx) => {
                let mut last = transmute!(self as BaseBitSlice);
                let mut middle = transmute!(self as BaseBitSlice);
                middle.bits.end -= end_bit_idx;
                last.bits.start = middle.bits.end;
                if middle.bits.is_empty() {
                    (Some(last), middle, None)
                } else {
                    (None, middle, Some(last))
                }
            }
            (start_bit_idx, end_bit_idx) => {
                if self.bits.len() == 0 {
                    (None, Default::default(), None)
                } else {
                    let first_end = self.bits.start + (8 - start_bit_idx);
                    let last_start = self.bits.end - end_bit_idx;
                    match usize::cmp(&first_end, &last_start) {
                        std::cmp::Ordering::Less => {
                            // Normal case, all three parts are nonempty.
                            let mut middle = transmute!(self as BaseBitSlice);
                            let mut first = transmute!(self as BaseBitSlice);
                            let mut last = transmute!(self as BaseBitSlice);
                            first.bits.end = first_end;
                            middle.bits.start = first_end;
                            middle.bits.end = last_start;
                            last.bits.start = last_start;
                            (Some(first), middle, Some(last))
                        }
                        std::cmp::Ordering::Equal => {
                            // Two-edge with empty middle
                            let mut first = transmute!(self as BaseBitSlice);
                            let mut last = transmute!(self as BaseBitSlice);
                            first.bits.end = first_end;
                            last.bits.start = last_start;
                            (Some(first), Default::default(), Some(last))
                        }
                        std::cmp::Ordering::Greater => {
                            // Only one partially referenced byte
                            (
                                Some(transmute!(self as BaseBitSlice)),
                                Default::default(),
                                None,
                            )
                        }
                    }
                }
            }
        }
    }
}

impl<'a, M: Mutability, A: UnaliasedInnerBytesAliasing> BaseBitSlice<'a, M, A> {
    /// Returns `Ok(slice)` if this slice is byte-aligned.
    /// Returns `Err(self)` otherwise.
    #[doc(alias = "try_into_byte_aligned_mut")]
    pub fn try_into_byte_aligned(
        self,
    ) -> Result<BaseBitSlice<'a, M, UnaliasedNoEdges>, Self> {
        if self.bits.start % 8 == 0 && self.bits.end % 8 == 0 {
            Ok(transmute!(self as BaseBitSlice))
        } else {
            Err(self)
        }
    }

    #[doc(alias = "into_aliased_edges_mut")]
    pub fn into_aliased_edges(self) -> BaseBitSlice<'a, M, AliasedEdgesOnly> {
        #[cfg(debug_assertions)]
        A::assert_bit_range_valid(self.bits.into_std());
        transmute!(self as BaseBitSlice)
    }

    /// Splits this bitslice into edges and a middle of bytes.
    ///
    /// Edge cases (no pun intended):
    ///
    /// * If this slice consists only of one partially-referenced byte, it will
    ///   be returned in the first edge `Option`, the middle part will be empty,
    ///   and the last edge `Option` will be `None`.
    pub fn split_bytes(
        self,
    ) -> (
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
        &'a [u8],
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
    ) {
        let (first, middle, last) = self.split_edges();
        (first, middle.into_bytes(), last)
    }

    /// Split a bit slice into two at a bit index.
    /// This requires marking the returned slices as possibly having aliased
    /// edges, since we cannot guarantee that the split will be performed at
    /// a byte boundary.
    ///
    /// # Panic
    ///
    /// Panics if `at >= self.len()`.
    #[doc(alias = "split_at_mut")]
    pub fn split_at(
        self,
        at: usize,
    ) -> (
        BaseBitSlice<'a, M, AliasedEdgesOnly>,
        BaseBitSlice<'a, M, AliasedEdgesOnly>,
    ) {
        if at == 0 {
            (Default::default(), self.into_aliased_edges())
        } else if at == self.len() {
            (self.into_aliased_edges(), Default::default())
        } else if at > self.len() {
            todo!("panic")
        } else {
            let new_split = self.bits.start + at;
            let mut head = transmute!(self as BaseBitSlice);
            let mut tail = transmute!(self as BaseBitSlice);
            head.bits.end = new_split;
            tail.bits.start = new_split;
            (head, tail)
        }
    }

    /// Unlike [`BaseBitSlice::split_at`], this method does not require changing
    /// the aliasing type. However, it only supports splitting at byte
    /// boundaries, or at the edges of the slice.
    ///
    /// # Panic
    ///
    /// Panics if `at >= self.len()`.
    pub fn try_split_at_no_additional_aliasing(
        self,
        at: usize,
    ) -> Result<(Self, Self), Self> {
        if at == 0 {
            Ok((Default::default(), self))
        } else if at == self.len() {
            Ok((self, Default::default()))
        } else if at > self.len() {
            todo!("panic")
        } else {
            let new_split = self.bits.start + at;
            if new_split % 8 == 0 {
                let mut head = transmute!(self as BaseBitSlice);
                let mut tail = transmute!(self as BaseBitSlice);
                head.bits.end = new_split;
                tail.bits.start = new_split;
                Ok((head, tail))
            } else {
                Err(self)
            }
        }
    }
}

impl<'a, M: MutMutability, A: Aliasing> BaseBitSlice<'a, M, A> {
    pub fn reborrow_mut(&mut self) -> BaseBitSlice<'_, M, A> {
        transmute!(self as BaseBitSlice)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned bit slice or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked_mut<'b>(
        &mut self,
    ) -> BaseBitSlice<'b, M, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn fill(&mut self, value: bool) {
        A::fill(self.reborrow_mut(), value)
    }
}

impl<'a, M: MutMutability, A: UnaliasedInnerBytesAliasing>
    BaseBitSlice<'a, M, A>
{
    /// Splits this bitslice into edges and a middle of bytes.
    ///
    /// Edge cases (no pun intended):
    ///
    /// * If this slice consists only of one partially-referenced byte, it will
    ///   be returned in the first edge `Option`, the middle part will be empty,
    ///   and the last edge `Option` will be `None`.
    pub fn split_bytes_mut(
        self,
    ) -> (
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
        &'a mut [u8],
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
    ) {
        let (first, middle, last) = self.split_edges();
        (first, middle.into_bytes_mut(), last)
    }
}

impl<'a, M: Mutability, A: UnaliasedAliasing> BaseBitSlice<'a, M, A> {
    /// This is sound because (TODO).
    /// This method would not be sound on `BitSlice` or `AliasedBitSlice`, since
    /// this slice could have partially referenced bytes.
    pub fn into_sync(self) -> BaseBitSlice<'a, M::Sync, A> {
        transmute!(self as BaseBitSlice)
    }

    /// This is sound because (TODO).
    /// This method would not be sound on `BitSlice` or `AliasedBitSlice`, since
    /// this slice could have  partially referenced bytes.
    pub fn into_unsync(self) -> BaseBitSlice<'a, M::Unsync, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn into_unaliased(self) -> BaseBitSlice<'a, M, Unaliased> {
        #[cfg(debug_assertions)]
        A::assert_bit_range_valid(self.bits.into_std());
        transmute!(self as BaseBitSlice)
    }
}

fn range(range: impl RangeBounds<usize>, len: usize) -> Range<usize> {
    let start = match range.start_bound() {
        std::ops::Bound::Included(&start) => start,
        std::ops::Bound::Excluded(start) => {
            start.checked_add(1).expect("index overflow")
        }
        std::ops::Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        std::ops::Bound::Included(end) => {
            end.checked_add(1).expect("index overflow")
        }
        std::ops::Bound::Excluded(&end) => end,
        std::ops::Bound::Unbounded => len,
    };
    assert!(start <= end, "index error");
    assert!(end <= len, "index error");
    start..end
}

impl<'a, M: ConstMutability, A: Aliasing> BaseBitSlice<'a, M, A> {
    /// This function is sound to be on `UnaliasedBitSlice` (as opposed to only
    /// being on `BitSlice` or `AliasedBitSlice`), because the `&[u8]` ensures
    /// that no writes can occurr to partially referenced bytes during the
    /// given lifetime.
    ///
    /// # Panics
    ///
    /// This function will panic if an out-of-bounds bit range is passed, or if
    /// the bit range is invalid for the edge aliasing type (e.g.
    /// `JustAnEdge`).
    ///
    /// # Safety
    ///
    /// It is unclear/undecided whether all/some/none atomic loads of read-only
    /// memory (e.g. non-interior-mutable statics) are defined behavior.
    /// Until that is decided, this function is unsafe, since there is no way to
    /// ensure that `bytes` does not refer to read-only memory.
    ///
    /// Concretely, conservatively this function is sound if any of:
    ///
    /// * `bytes` does not reference any read-only memory (e.g. `bytes` is
    ///   empty).
    /// * `A: UnaliasedAliasing` and no bit slice with a weaker aliasing type
    ///   will be derived from the returned slice.
    /// * `M: UnsyncMutability` and no bit slice with `M: SyncMutability` will
    ///   be derived from the returned slice.
    /// * (Maybe?) atomic loads of read-only memory are defined on the platform
    ///   being compiled for (not true on miri, e.g.).
    ///
    /// As a concrete example, it is sound to create/derive a
    /// `BitSlice<ConstSync, Unaliased>` or a `BitSlice<ConstUnsync,
    /// Aliased>` from the returned bit slice regardless of platform
    /// support, but a `BitSlice<ConstSync, Aliased>` may only be soundly
    /// created/derive from the returned bitslice if atomic reads of
    /// `AtomicU8`s in read-only memory are defined on the target platform.
    pub unsafe fn from_bytes(
        bytes: &'a [u8],
        bits: impl RangeBounds<usize>,
    ) -> Self {
        let bits = range(bits, bytes.len() * 8);
        let byte_start = div_floor_8(bits.start);
        let byte_end = div_ceil_8(bits.end);
        let _check_bounds = &bytes[byte_start..byte_end];
        A::assert_bit_range_valid(bits.clone());
        Self {
            data: NonNull::from(bytes).cast(),
            bits: bits.into(),
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }

    /// Produce an immutable [`BaseBitSlice`] referencing static memory of a
    /// value.
    ///
    /// `bits` must be a meaningful range (i.e. `start <= end`) with indices not
    /// more than `8`. It also must be suitable for the given aliasing type
    /// (e.g. `JustAnEdge`, `NoEdges`).
    pub fn from_value(value: u8, bits: ByteBitRange) -> Self {
        // These values are never modified, so it is safe to refer to any part
        // of them as `UnaliasedBitSlice<Const>` since there can never
        // be any incorrect-syncness modifications that race with reads.
        //
        // Miri does not allow atomic loads on read-only memory, so this can't
        // be a `[u8; 256]` under miri, since `UnaliasedBitSlice`s may
        // be safely converted/sliced into `BitSlice`s or `AliasedBitSlice`s.
        // I don't *think* it could actually page fault on any `std`-supported
        // arch, but just to be safe, make it `AtomicU8`s to begin with.
        //
        // No writes are ever performed, so this could also be `[UnsafeCell<u8>;
        // 256]` without data races, if that was `Sync`.
        static BYTES: [AtomicU8; 256] = {
            const ZERO: AtomicU8 = AtomicU8::new(0);
            let mut arr = [ZERO; 256];
            let mut idx = 0;
            while idx < 256 {
                arr[idx] = AtomicU8::new(idx as u8);
                idx += 1;
            }
            arr
        };

        let bits = verify_bit_range(bits).unwrap();
        let bits = CopyRange { start: bits.start.into(), end: bits.end.into() };

        A::assert_bit_range_valid(bits.into_std());

        let byte = &BYTES[value as usize];
        Self {
            data: NonNull::from(byte).cast(),
            bits,
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }
}

impl<'a, M: Mutability, A: Aliasing> BaseBitSlice<'a, M, A> {
    /// This function is sound to be on bit slices of all aliasing types (as
    /// opposed to only being on `UnaliasedBitSlice`), because the `&mut
    /// [u8]` ensures that no other accesses can occurr to partially
    /// referenced bytes during the given lifetime.
    ///
    /// Unlike [`BaseBitSlice::from_bytes`], this function is safe because it
    /// takes a mutable reference, so the issue about read-only-memory does not
    /// apply.
    ///
    /// # Panics
    ///
    /// This function will panic if an out-of-bounds bit range is passed, or if
    /// the bit range is invalid for the edge aliasing type (e.g.
    /// `JustAnEdge`).
    pub fn from_bytes_mut(
        bytes: &'a mut [u8],
        bits: impl RangeBounds<usize>,
    ) -> Self {
        let bits = range(bits, bytes.len() * 8);
        let byte_start = div_floor_8(bits.start);
        let byte_end = div_ceil_8(bits.end);
        let _check_bounds = &mut bytes[byte_start..byte_end];
        A::assert_bit_range_valid(bits.clone());
        Self {
            data: NonNull::from(bytes).cast(),
            bits: bits.into(),
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }
}

impl<'a, M: Mutability, A: UnaliasedInnerBytesAliasing> BaseBitSlice<'a, M, A> {
    /// Does not include partially-referenced bytes.
    pub fn as_whole_bytes(&self) -> &[u8] {
        let this = self.reborrow().split_edges().1;
        this.into_bytes()
    }

    /// Does not include partially-referenced bytes.
    pub fn into_whole_bytes(self) -> &'a [u8] {
        let this = self.split_edges().1;
        this.into_bytes()
    }
}

impl<'a, M: Mutability, A: UnaliasedAliasing> BaseBitSlice<'a, M, A> {
    /// Includes partially referenced bytes.
    pub fn as_bytes(&self) -> &[u8] {
        let byte_idx_start = div_floor_8(self.bits.start);
        let byte_idx_end = div_ceil_8(self.bits.end);
        let ptr = self.data.as_ptr().wrapping_add(byte_idx_start);
        unsafe {
            std::slice::from_raw_parts_mut(ptr, byte_idx_end - byte_idx_start)
        }
    }

    /// Includes partially referenced bytes.
    pub fn into_bytes(self) -> &'a [u8] {
        let byte_idx_start = div_floor_8(self.bits.start);
        let byte_idx_end = div_ceil_8(self.bits.end);
        let ptr = self.data.as_ptr().wrapping_add(byte_idx_start);
        unsafe {
            std::slice::from_raw_parts_mut(ptr, byte_idx_end - byte_idx_start)
        }
    }
}

impl<'a, M: MutMutability, A: UnaliasedInnerBytesAliasing>
    BaseBitSlice<'a, M, A>
{
    /// Does not include partially-referenced bytes.
    pub fn as_whole_bytes_mut(&mut self) -> &mut [u8] {
        let this = self.reborrow_mut().split_edges().1;
        this.into_bytes_mut()
    }

    /// Does not include partially-referenced bytes.
    pub fn into_whole_bytes_mut(self) -> &'a mut [u8] {
        let this = self.split_edges().1;
        this.into_bytes_mut()
    }
}

impl<'a, M: MutMutability, A: UnaliasedAliasing> BaseBitSlice<'a, M, A> {
    /// Includes partially referenced bytes.
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        let byte_idx_start = div_floor_8(self.bits.start);
        let byte_idx_end = div_ceil_8(self.bits.end);
        let ptr = self.data.as_ptr().wrapping_add(byte_idx_start);
        unsafe {
            std::slice::from_raw_parts_mut(ptr, byte_idx_end - byte_idx_start)
        }
    }
    /// Includes partially referenced bytes.
    pub fn into_bytes_mut(self) -> &'a mut [u8] {
        let byte_idx_start = div_floor_8(self.bits.start);
        let byte_idx_end = div_ceil_8(self.bits.end);
        let ptr = self.data.as_ptr().wrapping_add(byte_idx_start);
        unsafe {
            std::slice::from_raw_parts_mut(ptr, byte_idx_end - byte_idx_start)
        }
    }
}

fn div_ceil_8(val: usize) -> usize {
    if val % 8 == 0 { val / 8 } else { (val / 8) + 1 }
}

fn div_floor_8(val: usize) -> usize {
    val / 8
}

fn verify_bit_range(range: ByteBitRange) -> Result<ByteBitRange, String> {
    let ByteBitRange { start, end } = range;
    if start > end {
        Err(format!("bit range starts at {start} but ends at {end}"))
    } else if start > 8 {
        Err(format!(
            "bit range start index {start} is out of range for 8-bit byte"
        ))
    } else if end > 8 {
        Err(format!("bit range end index {end} is out of range for 8-bit byte"))
    } else {
        Ok(range)
    }
}

/// A (possibly mutable) rectangular view into a [`BitMap`].
#[derive(Debug, Clone, Copy)]
pub struct BitMapView<'a, M: Mutability, A: Aliasing> {
    /// Underlying data behind this ref. Must be valid for `self.stride *
    /// self.rows.end` bytes. TODO: maybe allow short last row?
    data: NonNull<u8>,
    /// Distance between rows in bytes. Must be `>=
    /// self.columns.end.div_ciel(8)`.
    stride: usize,
    /// The columns referenced by this view, in bits.
    columns: CopyRange<usize>,
    /// The rows referenced by this view.
    rows: CopyRange<usize>,
    _lifetime: PhantomData<&'a ()>,
    _mutability: PhantomData<M>,
    _edge_aliasing: PhantomData<A>,
}

impl<'a, M: MutMutability, A: Aliasing> BitMapView<'a, M, A> {
    pub fn reborrow_mut(&mut self) -> BitMapView<'_, M, A> {
        transmute!(self as BitMapView)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned `BitMapView` or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked_mut<'b>(
        &mut self,
    ) -> BitMapView<'b, M, A> {
        transmute!(self as BitMapView)
    }

    pub fn rows_mut(&mut self) -> impl Iterator<Item = BaseBitSlice<'_, M, A>> {
        self.reborrow_mut().into_rows()
    }

    pub fn fill(&mut self, value: bool) {
        self.rows_mut().for_each(|mut row| row.fill(value));
    }
}

impl<'a, M: Mutability, A: Aliasing> BitMapView<'a, M, A> {
    pub fn into_const(self) -> BitMapView<'a, M::Const, A> {
        transmute!(self as BitMapView)
    }

    pub fn reborrow(&self) -> BitMapView<'_, M::Const, A> {
        transmute!(self as BitMapView)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned `BitMapView` or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked<'b>(&self) -> BitMapView<'b, M::Const, A> {
        transmute!(self as BitMapView)
    }

    pub fn into_rows(self) -> impl Iterator<Item = BaseBitSlice<'a, M, A>> {
        self.rows.into_iter().map(move |row| {
            let start_byte_idx = self.stride.checked_mul(row).unwrap();
            let data =
                NonNull::new(self.data.as_ptr().wrapping_add(start_byte_idx))
                    .unwrap();
            BaseBitSlice {
                data,
                bits: self.columns,
                _lifetime: PhantomData,
                _mutability: PhantomData,
                _edge_aliasing: PhantomData,
            }
        })
    }

    /// This can inherit the aliasing type, because it takes &self so TODO.
    pub fn rows(&self) -> impl Iterator<Item = BaseBitSlice<'_, M::Const, A>> {
        self.reborrow().into_rows()
    }

    #[cfg(any())]
    pub fn count_ones(&self) -> usize {
        self.chunks().map(|(_y, _x, slice)| slice.count_ones()).sum()
    }

    #[cfg(any)]
    pub fn count_ones(&self) -> usize {
        let mut count_ones = 0;
        'rows: for row in 0..self.height {
            let start_byte = row * self.stride;
            for (byte_col, byte) in
                self.data[start_byte..][..self.stride].iter().enumerate()
            {
                if byte_col * 8 + 7 < self.width {
                    count_ones += byte.count_ones() as usize;
                } else {
                    for bit_col in 0..8 {
                        let col = (byte_col << 3) | bit_col;
                        if col >= self.width {
                            continue 'rows;
                        }
                        if byte & (1 << bit_col) != 0 {
                            count_ones += 1;
                        }
                    }
                }
            }
        }
        count_ones
    }
}

pub struct Bits<'a, M: Mutability, A: Aliasing> {
    /// If this is `Left`, it contains the next bits to be returned by `next`;
    /// this is usually used when `A::SEMANTICALLY_ALIASED` is `false`.
    /// If  this is `Right`, it contains a pointer to the byte to read the next
    /// bit from; this is only used when `A::SEMANTICALLY_ALIASED` is
    /// `true`, since caching could result in outdated values.
    first: Either<u8, NonNull<u8>>,
    /// Bits in `first` that are still to be yielded.
    first_bits: ByteBitRange,
    /// If this is `Left`, it contains the next bits to be returned by
    /// `next_back`; this is usually used when `A::SEMANTICALLY_ALIASED` is
    /// `false`. If  this is `Right`, it contains a pointer to the byte to
    /// read the next bit from; this is only used when
    /// `A::SEMANTICALLY_ALIASED` is `true`, since caching could result in
    /// outdated values.
    last: Either<u8, NonNull<u8>>,
    /// Bits in `last` that are still to be yielded.
    last_bits: ByteBitRange,
    /// Bytes whose bits have not yet been yielded, and that are not yet in
    /// `first` or `last`.
    inner: RawBytes<'a, M, A>,
}

impl<'a, M: Mutability, A: Aliasing> Default for Bits<'a, M, A> {
    fn default() -> Self {
        Self {
            first: Either::Left(0),
            first_bits: ByteBitRange::empty(),
            last: Either::Left(0),
            last_bits: ByteBitRange::empty(),
            inner: Default::default(),
        }
    }
}
fn byte_to_bits(byte: u8) -> std::array::IntoIter<bool, 8> {
    [
        (byte & (1 << 0)) != 0,
        (byte & (1 << 1)) != 0,
        (byte & (1 << 2)) != 0,
        (byte & (1 << 3)) != 0,
        (byte & (1 << 4)) != 0,
        (byte & (1 << 5)) != 0,
        (byte & (1 << 6)) != 0,
        (byte & (1 << 7)) != 0,
    ]
    .into_iter()
}
fn byte_to_bits_with_range(
    byte: u8,
    bitrange: ByteBitRange,
) -> std::array::IntoIter<bool, 8> {
    let mut bits = byte_to_bits(byte);
    for _ in 0..bitrange.start {
        bits.next();
    }
    for _ in bitrange.end..8 {
        bits.next_back();
    }
    bits
}

impl<'a, M: Mutability, A: Aliasing> Iterator for Bits<'a, M, A> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Read from first
            if let Some(idx) = self.first_bits.pop_first() {
                let byte = match self.first {
                    Either::Left(byte) => byte,
                    Either::Right(ptr) => unsafe {
                        A::load_byte::<M>(ptr.as_ptr(), false)
                    },
                };
                return Some((byte & (1 << idx)) != 0);
            }

            // If first exhausted, get a new first.
            let Some((ptr, new_first_bits)) = self.inner.next() else {
                break;
            };
            let new_first = if A::SEMANTICALLY_ALIASED {
                Either::Right(NonNull::new(ptr).unwrap())
            } else {
                let byte = unsafe { A::load_byte::<M>(ptr, false) };
                Either::Left(byte)
            };
            self.first = new_first;
            self.first_bits = new_first_bits;
        }

        // Read from last if everything else exhausted
        let idx = self.last_bits.pop_first()?;
        let byte = match self.last {
            Either::Left(byte) => byte,
            Either::Right(ptr) => unsafe {
                A::load_byte::<M>(ptr.as_ptr(), false)
            },
        };
        Some((byte & (1 << idx)) != 0)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.len();
        (count, Some(count))
    }

    fn fold<B, F>(mut self, init: B, mut f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        if A::SEMANTICALLY_ALIASED {
            let mut accum = init;
            while let Some(x) = self.next() {
                accum = f(accum, x);
            }
            accum
        } else {
            let mut accum = init;
            if !self.first_bits.is_empty() {
                accum = byte_to_bits_with_range(
                    self.first.left().unwrap(),
                    self.first_bits,
                )
                .fold(accum, &mut f);
                self.first_bits = ByteBitRange::empty();
            }
            for (byte, bitrange) in self.inner {
                if bitrange.len() < 8 {
                    // Last byte
                    let byte = unsafe { A::load_byte::<M>(byte, false) };
                    accum = byte_to_bits_with_range(byte, bitrange)
                        .fold(accum, &mut f);
                } else {
                    let byte = unsafe { A::load_byte::<M>(byte, true) };
                    accum = byte_to_bits(byte).fold(accum, &mut f);
                }
            }
            if !self.last_bits.is_empty() {
                accum = byte_to_bits_with_range(
                    self.last.left().unwrap(),
                    self.last_bits,
                )
                .fold(accum, &mut f);
                self.last_bits = ByteBitRange::empty();
            }
            accum
        }
    }
}

impl<'a, M: Mutability, A: Aliasing> ExactSizeIterator for Bits<'a, M, A> {
    fn len(&self) -> usize {
        self.first_bits.len()
            + self.inner.inner.bits.len()
            + self.last_bits.len()
    }
}

impl<'a, M: Mutability, A: Aliasing> DoubleEndedIterator for Bits<'a, M, A> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            // Read from last
            if let Some(idx) = self.last_bits.pop_last() {
                let byte = match self.last {
                    Either::Left(byte) => byte,
                    Either::Right(ptr) => unsafe {
                        A::load_byte::<M>(ptr.as_ptr(), false)
                    },
                };
                return Some((byte & (1 << idx)) != 0);
            }

            // If last exhausted, get a new last.
            let Some((ptr, new_last_bits)) = self.inner.next_back() else {
                break;
            };
            let new_last = if A::SEMANTICALLY_ALIASED {
                Either::Right(NonNull::new(ptr).unwrap())
            } else {
                let byte = unsafe { A::load_byte::<M>(ptr, false) };
                Either::Left(byte)
            };
            self.last = new_last;
            self.last_bits = new_last_bits;
        }

        // Read from first if everything else exhausted
        let idx = self.first_bits.pop_last()?;
        let byte = match self.first {
            Either::Left(byte) => byte,
            Either::Right(ptr) => unsafe {
                A::load_byte::<M>(ptr.as_ptr(), false)
            },
        };
        Some((byte & (1 << idx)) != 0)
    }

    fn rfold<B, F>(mut self, init: B, mut f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        if A::SEMANTICALLY_ALIASED {
            let mut accum = init;
            while let Some(x) = self.next_back() {
                accum = f(accum, x);
            }
            accum
        } else {
            let mut accum = init;
            if !self.last_bits.is_empty() {
                accum = byte_to_bits_with_range(
                    self.last.left().unwrap(),
                    self.last_bits,
                )
                .rfold(accum, &mut f);
                self.last_bits = ByteBitRange::empty();
            }
            for (byte, bitrange) in self.inner.rev() {
                if bitrange.len() < 8 {
                    // first byte
                    let byte = unsafe { A::load_byte::<M>(byte, false) };
                    accum = byte_to_bits_with_range(byte, bitrange)
                        .rfold(accum, &mut f);
                } else {
                    let byte = unsafe { A::load_byte::<M>(byte, true) };
                    accum = byte_to_bits(byte).rfold(accum, &mut f);
                }
            }
            if !self.first_bits.is_empty() {
                accum = byte_to_bits_with_range(
                    self.first.left().unwrap(),
                    self.first_bits,
                )
                .rfold(accum, &mut f);
                self.first_bits = ByteBitRange::empty();
            }
            accum
        }
    }
}

pub type UnaliasedBitSlice<'a, M> = BaseBitSlice<'a, M, Unaliased>;
pub type BitSlice<'a, M> = BaseBitSlice<'a, M, AliasedEdgesOnly>;
pub type AliasedBitSlice<'a, M> = BaseBitSlice<'a, M, Aliased>;

#[cfg(test)]
mod tests {
    use crate::{
        mutability::{ConstSync, MutableSync},
        BaseBitSlice, BitSlice, ByteBitRange, Unaliased,
    };

    #[test]
    fn from_value() {
        #[cfg(not(miri))]
        let bytes = 0..=255;
        #[cfg(miri)]
        let bytes = [0, 255, 42, 0x55, 0xf0, 0x0f, 0xa5, 0xe7, 0x7e];
        for i in bytes {
            for s in 0..8 {
                for e in s..8 {
                    let slice =
                        BaseBitSlice::<ConstSync, Unaliased>::from_value(
                            i,
                            ByteBitRange::from(s..e),
                        );
                    assert_eq!(
                        slice.bits().collect::<Vec<bool>>(),
                        (s..e)
                            .map(|bit_idx| (i & (1 << bit_idx)) != 0)
                            .collect::<Vec<bool>>(),
                        "i = {i}, s = {s}, e = {e}"
                    );
                }
            }
        }
    }

    #[test]
    fn bits() {
        let mut bytes = [0b01001001, 0b10010010, 0b00100100];
        let slice = BitSlice::<MutableSync>::from_bytes_mut(&mut bytes, ..);
        let bits = std::iter::repeat([true, false, false])
            .take(8)
            .flatten()
            .collect::<Vec<bool>>();
        assert_eq!(slice.bits().collect::<Vec<_>>(), bits);
    }
}
