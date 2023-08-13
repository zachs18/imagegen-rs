#![deny(unsafe_op_in_unsafe_fn)]
use std::{
    cell::Cell,
    marker::PhantomData,
    mem::transmute,
    ops::{Range, RangeBounds},
    ptr::NonNull,
    sync::atomic::{AtomicU8, Ordering},
};

use az::Az;
use radium::Radium;

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
    ($self:ident as AliasedBitSlice) => {
        AliasedBitSlice {
            data: $self.data,
            bits: $self.bits.clone(),
            _lifetime: PhantomData,
            _mutability: PhantomData,
        }
    };
}

/// The mutability of a [`BitMapView`] or [`BaseBitSlice`].
///
/// The types implementing this trait can be either represent mutable
/// ([`MutMutability`]) or immutable ([`ConstMutability`]) bit collections, and
/// can orthogonally either represent thread-safe ([`SyncMutability`]) or
/// non-thread-safe ([`UnsyncMutability`]) bit collections.
///
/// Note that the thread-safety of a bit collection type only applies to the
/// "edges" of the bit collection, i.e. bytes to which the bit collection does
/// not wholly refer, and whose other bits may have a different mutability under
/// a different reference. The thread-safety of a bit collection can be changed
/// if the bit collection only refers to entire bytes, or (in the case of
/// [`UnaliasedBitSlice`]) as a static guarantee that the other bits of the
/// "edges" are not referred to by anything that could violate aliasing rules by
/// using non-atomic loads/stores.
pub unsafe trait Mutability: std::fmt::Debug {
    type Mut: MutMutability;
    type Const: ConstMutability;
    type Sync: SyncMutability;
    type Unsync: UnsyncMutability;

    /// Must be either `Cell<u8>` or `AtomicU8`.
    type Edge: Radium<Item = u8>;
}

pub unsafe trait MutMutability: Mutability<Mut = Self> {}
unsafe impl<M: Mutability<Mut = M>> MutMutability for M {}

pub unsafe trait ConstMutability:
    Mutability<Const = Self> + Copy
{
}
unsafe impl<M: Mutability<Const = M> + Copy> ConstMutability for M {}

pub unsafe trait SyncMutability:
    Mutability<Edge = AtomicU8, Sync = Self>
{
}
unsafe impl<M: Mutability<Edge = AtomicU8, Sync = Self>> SyncMutability for M {}

pub unsafe trait UnsyncMutability:
    Mutability<Edge = Cell<u8>, Unsync = Self>
{
}
unsafe impl<M: Mutability<Edge = Cell<u8>, Unsync = Self>> UnsyncMutability
    for M
{
}

/// This mutability marker type indicates that a given bit slice references is
/// mutable and thread-safe.
///
/// Bit slices references with this mutability are not `Copy`.
///
/// Note that bit slices with unaliased edges can always be converted between
/// thread-safe and non-thread-safe versions.
///
/// ```rust
/// # use bitmap::{AliasedBitSlice, MutableSync, ConstUnsync, ByteBitRange};
/// let mut bytes = [42];
/// let slice = AliasedBitSlice::<MutableSync>::from_bytes_mut(&mut bytes, 0..6);
/// std::thread::scope(|scope| {
///     scope.spawn(|| {
///         assert_eq!(slice.bits().collect::<Vec<bool>>(), [false, true, false, true, false, true]);
///     });
/// });
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct MutableSync;

/// This mutability marker type indicates that a given bit slice references is
/// immutable and thread-safe.
///
/// Bit slices references with this mutability are `Copy`.
///
/// Note that bit slices with unaliased edges can always be converted between
/// thread-safe and non-thread-safe versions.
///
/// ```rust
/// # use bitmap::{AliasedBitSlice, ConstSync, ConstUnsync, ByteBitRange};
/// let slice = AliasedBitSlice::<ConstSync>::from_value(42, ByteBitRange::from(0..6));
/// std::thread::spawn(move || {
///     assert_eq!(slice.bits().collect::<Vec<bool>>(), [false, true, false, true, false, true]);
/// });
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ConstSync;

unsafe impl Mutability for MutableSync {
    type Mut = Self;
    type Const = ConstSync;
    type Sync = Self;
    type Unsync = MutableUnsync;

    type Edge = AtomicU8;
}
unsafe impl Mutability for ConstSync {
    type Mut = MutableSync;
    type Const = Self;
    type Sync = Self;
    type Unsync = ConstUnsync;

    type Edge = AtomicU8;
}

#[non_exhaustive]
#[derive(Debug)]
pub struct MutableUnsync(PhantomData<*const ()>);

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ConstUnsync(PhantomData<*const ()>);

unsafe impl Mutability for MutableUnsync {
    type Mut = Self;
    type Const = ConstUnsync;
    type Sync = MutableSync;
    type Unsync = Self;

    type Edge = Cell<u8>;
}
unsafe impl Mutability for ConstUnsync {
    type Mut = MutableUnsync;
    type Const = Self;
    type Sync = ConstSync;
    type Unsync = Self;

    type Edge = Cell<u8>;
}

pub unsafe trait EdgeAliasing: Sized + std::fmt::Debug {
    /// `assert!` that the given `bits` are valid for this aliasing type.
    /// Callers should `#[cfg(debug_assertions)] A::assert_bits_valid(bits);` if
    /// they want a debug assertion, or call unconditionally to check in
    /// release as well.
    #[inline]
    fn assert_bit_range_valid(bits: Range<usize>) {
        assert!(bits.start <= bits.end, "invalid range {bits:?}");
    }

    fn fill<'a, M: MutMutability>(
        slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    );

    /// # Safety:
    ///
    /// `slice` must contain `bit_idx`
    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8;
}
pub unsafe trait EdgeAliasingUnaliased: EdgeAliasing {}

/// This bitslice must use interior mutability to access its
/// partially-referenced bytes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AliasedEdges;

/// This bitslice can access its partially-referenced bytes normally.
///
/// The main difference between this and [`AliasedEdges`] is that bit slices
/// with this aliasing type uphold the "aliasing XOR mutability" rule with
/// respect to all of their underlying bytes, not just the "inner" bytes, so
/// interior-mutable accesses are not needed at the edges.
///
/// The bits on the edges of this slice are not important and may be freely
/// overwritten if this slice is mutable.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct UnaliasedEdges;

/// This bitslice does not have any partially-referenced bytes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct NoEdges;

/// This bitslice consists of only one partially-referenced byte.
/// The aliasing of that byte is given by the `A` type parameter.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct JustAnEdge<A: EdgeAliasing>(PhantomData<A>);

unsafe impl EdgeAliasing for AliasedEdges {
    fn fill<'a, M: MutMutability>(
        slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    ) {
        let (first, mut middle, last) = slice.split_edges();

        middle.fill(value);

        [first, last].into_iter().flatten().for_each(|mut slice| {
            slice.fill(value);
        });
    }

    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8 {
        debug_assert!(slice.bits.into_range().contains(&bit_idx));
        let byte_idx = bit_idx / 8;
        let full_byte_range =
            div_ceil_8(slice.bits.start)..div_floor_8(slice.bits.end);
        if full_byte_range.contains(&byte_idx) {
            unsafe { *slice.data.as_ptr().cast_const().add(byte_idx) }
        } else {
            let byte_ptr =
                unsafe { slice.data.as_ptr().cast_const().add(byte_idx) };
            let byte: &<M as Mutability>::Edge = unsafe { &*byte_ptr.cast() };
            byte.load(Ordering::Relaxed)
        }
    }
}
unsafe impl EdgeAliasing for UnaliasedEdges {
    fn fill<'a, M: MutMutability>(
        mut slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    ) {
        slice.as_bytes_mut().fill(if value { !0 } else { 0 });
    }

    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8 {
        debug_assert!(slice.bits.into_range().contains(&bit_idx));
        unsafe { *slice.data.as_ptr().cast_const().add(bit_idx / 8) }
    }
}
unsafe impl EdgeAliasing for NoEdges {
    #[inline]
    fn assert_bit_range_valid(bits: Range<usize>) {
        assert!(bits.start <= bits.end, "invalid range {bits:?}");
        assert_eq!(bits.start % 8, 0, "invalid range {bits:?} for NoEdges");
        assert_eq!(bits.end % 8, 0, "invalid range {bits:?} for NoEdges");
    }

    fn fill<'a, M: MutMutability>(
        mut slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    ) {
        slice.as_bytes_mut().fill(if value { !0 } else { 0 });
    }

    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8 {
        debug_assert!(slice.bits.into_range().contains(&bit_idx));
        unsafe { *slice.data.as_ptr().cast_const().add(bit_idx / 8) }
    }
}
unsafe impl<A: EdgeAliasing> EdgeAliasing for JustAnEdge<A> {
    #[inline]
    fn assert_bit_range_valid(bits: Range<usize>) {
        if bits.end % 8 != 0 {
            assert_eq!(
                div_floor_8(bits.start),
                div_floor_8(bits.end),
                "JustAnEdge requires that the range {bits:?} reference only bits in one byte"
            );
        } else {
            assert_eq!(
                div_ceil_8(bits.start),
                div_floor_8(bits.end),
                "JustAnEdge requires that the range {bits:?} reference only bits in one byte, and not all the bits in one byte"
            );
        }
        A::assert_bit_range_valid(bits.clone());
    }

    fn fill<'a, M: MutMutability>(
        slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    ) {
        let byte_idx = div_floor_8(slice.bits.start);
        let byte_start_bit_idx = byte_idx * 8;
        let bit_range = ByteBitRange::from(
            (slice.bits.start - byte_start_bit_idx).az::<u8>()
                ..(slice.bits.end - byte_start_bit_idx).az::<u8>(),
        );
        let ptr = slice.data.as_ptr().wrapping_add(byte_idx);
        let mask = bit_range.mask();
        let byte: &<M as Mutability>::Edge =
            unsafe { &*ptr.cast_const().cast() };
        if value {
            byte.fetch_or(mask, Ordering::Relaxed);
        } else {
            byte.fetch_and(!mask, Ordering::Relaxed);
        }
    }

    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8 {
        #[cfg(debug_assertions)]
        Self::assert_bit_range_valid(slice.bits.into_range());
        let forget_just_edge: BaseBitSlice<'a, M, A> =
            transmute!(slice as BaseBitSlice);
        unsafe { A::load_byte_containing(forget_just_edge, bit_idx) }
    }
}

unsafe impl EdgeAliasingUnaliased for UnaliasedEdges {}
unsafe impl EdgeAliasingUnaliased for NoEdges {}
unsafe impl<A: EdgeAliasingUnaliased> EdgeAliasingUnaliased for JustAnEdge<A> {}

#[derive(Debug, Clone, Copy)]
struct CopyRange<T> {
    start: T,
    end: T,
}

impl<T> CopyRange<T> {
    fn into_range(self) -> Range<T> {
        self.start..self.end
    }
}

impl<T> From<Range<T>> for CopyRange<T> {
    fn from(value: Range<T>) -> Self {
        Self { start: value.start, end: value.end }
    }
}

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
    ) -> BitMapView<'_, M, UnaliasedEdges> {
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
    ) -> BitMapView<'_, M, UnaliasedEdges> {
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
}

impl From<Range<u8>> for ByteBitRange {
    fn from(value: Range<u8>) -> Self {
        Self { start: value.start, end: value.end }
    }
}

impl ByteBitRange {
    pub fn mask(&self) -> u8 {
        if self.start >= self.end || self.start >= 8 {
            0
        } else if self.end >= 8 {
            0b11111111 << self.start
        } else {
            let ByteBitRange { start, end } = *self;

            ((255u8 << start) << (8 - end)) >> (8 - end)
        }
    }
}

/// A reference to a slice of contiguous bits.
///
/// The `M` and `A` type parameters control how the bits can be acccessed.
#[derive(Debug, Clone, Copy)]
pub struct BaseBitSlice<'a, M: Mutability, A: EdgeAliasing> {
    data: NonNull<u8>,
    bits: CopyRange<usize>,
    _lifetime: PhantomData<&'a ()>,
    _mutability: PhantomData<M>,
    _edge_aliasing: PhantomData<A>,
}

unsafe impl<M: Mutability + Send + Sync, A: EdgeAliasing> Send
    for BaseBitSlice<'_, M, A>
{
}
unsafe impl<M: Mutability + Send + Sync, A: EdgeAliasing> Sync
    for BaseBitSlice<'_, M, A>
{
}

impl<'a, M: Mutability, A: EdgeAliasing> Default for BaseBitSlice<'a, M, A> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<'a, M: Mutability, A: EdgeAliasing> BaseBitSlice<'a, M, A> {
    pub const fn empty() -> Self {
        Self {
            data: NonNull::dangling(),
            bits: CopyRange { start: 0, end: 0 },
            _lifetime: PhantomData,
            _mutability: PhantomData,
            _edge_aliasing: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.bits.into_range().len()
    }

    pub fn into_const(self) -> BaseBitSlice<'a, M::Const, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn reborrow(&self) -> BaseBitSlice<'_, M::Const, A> {
        transmute!(self as BaseBitSlice)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned `AliasedBitSlice` or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked<'b>(
        &self,
    ) -> BaseBitSlice<'b, M::Const, A> {
        transmute!(self as BaseBitSlice)
    }

    /// Returns `Ok(slice)` if this slice is byte-aligned.
    /// Returns `Err(self)` otherwise.
    pub fn try_into_byte_aligned(
        self,
    ) -> Result<BaseBitSlice<'a, M, NoEdges>, Self> {
        if self.bits.start % 8 == 0 && self.bits.end % 8 == 0 {
            Ok(transmute!(self as BaseBitSlice))
        } else {
            Err(self)
        }
    }

    pub fn into_aliased(self) -> BaseBitSlice<'a, M, AliasedEdges> {
        #[cfg(debug_assertions)]
        A::assert_bit_range_valid(self.bits.into_range());
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

    /// Splits this bitslice into edges and a byte-aligned middle.
    ///
    /// Edge cases (no pun intended):
    ///
    /// * If this slice consists only of one partially-referenced byte, it will
    ///   be returned in the first edge `Option`, the middle part will be empty,
    ///   and the last edge `Option` will be `None`.
    pub fn split_edges(
        self,
    ) -> (
        Option<BaseBitSlice<'a, M, JustAnEdge<A>>>,
        BaseBitSlice<'a, M, NoEdges>,
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
                if middle.bits.into_range().is_empty() {
                    (Some(last), middle, None)
                } else {
                    (None, middle, Some(last))
                }
            }
            (start_bit_idx, end_bit_idx) => {
                if self.bits.into_range().len() == 0 {
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

    pub fn into_bits(self) -> Bits<'a, M, A> {
        if self.len() == 0 {
            Bits { cached_first: 0, cached_last: 0, inner: self }
        } else {
            let (first, middle, last) = self.reborrow().split_bytes();
            let cached_first = first
                .map(|first| unsafe {
                    let idx = first.bits.start;
                    JustAnEdge::load_byte_containing(first, idx)
                })
                .unwrap_or_else(|| middle[0]);
            let cached_last = last
                .map(|last| unsafe {
                    let idx = last.bits.end - 1;
                    JustAnEdge::load_byte_containing(last, idx)
                })
                .or(middle.get(0).copied())
                .unwrap_or(cached_first);
            Bits { cached_first, cached_last, inner: self }
        }
    }

    pub fn bits(&self) -> Bits<'_, M::Const, A> {
        self.reborrow().into_bits()
    }

    /// Split a bit slice into two at a bit index.
    /// This requires marking the returned slices as possibly having aliased
    /// edges, since we cannot guarantee that the split will be performed at
    /// a byte boundary.
    pub fn split_at(
        self,
        at: usize,
    ) -> (BaseBitSlice<'a, M, AliasedEdges>, BaseBitSlice<'a, M, AliasedEdges>)
    {
        if at == 0 {
            (Default::default(), self.into_aliased())
        } else if at == self.len() {
            (self.into_aliased(), Default::default())
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

impl<'a, M: MutMutability, A: EdgeAliasing> BaseBitSlice<'a, M, A> {
    pub fn reborrow_mut(&mut self) -> BaseBitSlice<'_, M, A> {
        transmute!(self as BaseBitSlice)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned `AliasedBitSlice` or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked_mut<'b>(
        &mut self,
    ) -> BaseBitSlice<'b, M, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn fill(&mut self, value: bool) {
        A::fill(self.reborrow_mut(), value)
    }

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

impl<'a, M: Mutability, A: EdgeAliasingUnaliased> BaseBitSlice<'a, M, A> {
    /// This is sound because (TODO).
    /// This method would not be sound on `AliasedBitSlice`, since this slice
    /// could have partially referenced bytes.
    pub fn into_sync(self) -> BaseBitSlice<'a, M::Sync, A> {
        transmute!(self as BaseBitSlice)
    }

    /// This is sound because (TODO).
    /// This method would not be sound on `AliasedBitSlice`, since this slice
    /// could have  partially referenced bytes.
    pub fn into_unsync(self) -> BaseBitSlice<'a, M::Unsync, A> {
        transmute!(self as BaseBitSlice)
    }

    pub fn into_unaliased(self) -> BaseBitSlice<'a, M, UnaliasedEdges> {
        #[cfg(debug_assertions)]
        A::assert_bit_range_valid(self.bits.into_range());
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

impl<'a, M: ConstMutability, A: EdgeAliasing> BaseBitSlice<'a, M, A> {
    /// This function is sound to be on `UnaliasedBitSlice` (as opposed to only
    /// being on `AliasedBitSlice`), because the `&[u8]` ensures that no
    /// writes can occurr to partially referenced bytes during the given
    /// lifetime.
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
    /// * The returned slice is `UnaliasedBitSlice` and no `AliasedBitSlice`
    ///   will be derived from the returned slice.
    /// * `M: UnsyncMutability` and no bit slice with `M: SyncMutability` will
    ///   be derived from the returned slice.
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
        // be safely converted/sliced into `AliasedBitSlice`s.
        // I don't *think* it could actually page fault on any `std`-supported
        // arch, but just to be safe, make it `AtomicU8`s to begin with.
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

        A::assert_bit_range_valid(bits.into_range());

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

impl<'a, M: Mutability, A: EdgeAliasing> BaseBitSlice<'a, M, A> {
    /// This function is sound to be on `UnaliasedBitSlice` (as opposed to only
    /// being on `AliasedBitSlice`), because the `&mut [u8]` ensures that no
    /// other accesses can occurr to partially referenced bytes during the given
    /// lifetime.
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

    pub fn into_whole_bytes(self) -> &'a [u8] {
        let this = self.split_edges().1;
        this.into_bytes()
    }
}

impl<'a, M: Mutability, A: EdgeAliasingUnaliased> BaseBitSlice<'a, M, A> {
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

impl<'a, M: MutMutability, A: EdgeAliasingUnaliased> BaseBitSlice<'a, M, A> {
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
pub struct BitMapView<'a, M: Mutability, A: EdgeAliasing> {
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

impl<'a, M: MutMutability, A: EdgeAliasing> BitMapView<'a, M, A> {
    pub fn reborrow_mut(&mut self) -> BitMapView<'_, M, A> {
        transmute!(self as BitMapView)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned `AliasedBitSlice` or
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

impl<'a, M: Mutability, A: EdgeAliasing> BitMapView<'a, M, A> {
    pub fn into_const(self) -> BitMapView<'a, M::Const, A> {
        transmute!(self as BitMapView)
    }

    pub fn reborrow(&self) -> BitMapView<'_, M::Const, A> {
        transmute!(self as BitMapView)
    }

    /// SAFETY: (TODO) The user must ensure that the underlying bytes are not
    /// accessed in a UB way while the returned `AliasedBitSlice` or
    /// anything derived from it is accessible.
    pub unsafe fn reborrow_unchecked<'b>(&self) -> BitMapView<'b, M::Const, A> {
        transmute!(self as BitMapView)
    }

    pub fn into_rows(self) -> impl Iterator<Item = BaseBitSlice<'a, M, A>> {
        self.rows.into_range().map(move |row| {
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

    /// This can be Unaliased, because it takes &self so nothing can modify
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

pub struct Bits<'a, M: Mutability, A: EdgeAliasing> {
    /// Always contains the next bit to be returned by `next`.
    cached_first: u8,
    /// Always contains the next bit to be returned by `next_back`.
    cached_last: u8,
    inner: BaseBitSlice<'a, M, A>,
}

impl<'a, M: Mutability, A: EdgeAliasing> Bits<'a, M, A> {
    fn load_first(&self) -> u8 {
        unsafe {
            A::load_byte_containing(
                self.inner.reborrow(),
                self.inner.bits.start,
            )
        }
    }
    fn load_last(&self) -> u8 {
        unsafe {
            A::load_byte_containing(
                self.inner.reborrow(),
                self.inner.bits.end - 1,
            )
        }
    }
}

impl<'a, M: Mutability, A: EdgeAliasing> Iterator for Bits<'a, M, A> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.inner.bits.into_range().is_empty() {
            return None;
        }
        let bit_idx = self.inner.bits.start % 8;
        let bit = (self.cached_first & (1 << bit_idx)) != 0;
        self.inner.bits.start += 1;
        if bit_idx == 7 && !self.inner.bits.into_range().is_empty() {
            self.cached_first = self.load_first();
        }
        Some(bit)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.bits.into_range().size_hint()
    }
}

impl<'a, M: Mutability, A: EdgeAliasing> ExactSizeIterator for Bits<'a, M, A> {
    fn len(&self) -> usize {
        self.inner.bits.into_range().len()
    }
}

impl<'a, M: Mutability, A: EdgeAliasing> DoubleEndedIterator
    for Bits<'a, M, A>
{
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.inner.bits.into_range().is_empty() {
            return None;
        }
        self.inner.bits.end -= 1;
        let bit_idx = self.inner.bits.end % 8;
        let bit = (self.cached_last & (1 << bit_idx)) != 0;
        if bit_idx == 0 && !self.inner.bits.into_range().is_empty() {
            self.cached_last = self.load_last();
        }
        Some(bit)
    }
}

pub type UnaliasedBitSlice<'a, M> = BaseBitSlice<'a, M, UnaliasedEdges>;
pub type AliasedBitSlice<'a, M> = BaseBitSlice<'a, M, AliasedEdges>;

#[cfg(test)]
mod tests {
    use crate::{BaseBitSlice, ByteBitRange, ConstSync, UnaliasedEdges};

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
                        BaseBitSlice::<ConstSync, UnaliasedEdges>::from_value(
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
}
