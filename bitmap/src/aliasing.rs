use std::{marker::PhantomData, ops::Range, sync::atomic::Ordering};

use az::Az;
use radium::Radium;

use crate::{
    div_ceil_8, div_floor_8,
    mutability::{MutMutability, Mutability},
    BaseBitSlice, ByteBitRange,
};

pub unsafe trait Aliasing:
    Sized + Copy + std::fmt::Debug + 'static
{
    /// Version of this aliasing type, but statically known to not have
    /// partially-referenced bytes.
    type NoEdges: Aliasing;

    /// `false` if bit slices of this aliasing type semantically uphold
    /// "aliasing XOR mutability" for their referenced bits.
    /// `true` if bit slices of this aliasing type may observe modifications
    /// that were not made through that bit slice.
    const SEMANTICALLY_ALIASED: bool;

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

    unsafe fn load_byte<M: Mutability>(byte: *const u8, is_inner: bool) -> u8;

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        is_inner: bool,
        value: u8,
        mask: u8,
    );
}

/// This bitslice may access its wholly-referenced bytes without interior
/// mutability.
pub unsafe trait UnaliasedInnerBytesAliasing:
    Aliasing<NoEdges = UnaliasedNoEdges>
{
}

/// This bitslice may access any of its (wholly or partially) referenced bytes
/// without interior mutability.
pub unsafe trait UnaliasedAliasing: UnaliasedInnerBytesAliasing {}

/// This bitslice may be aliased and must use interior mutability to access all
/// of its bytes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Aliased;

/// This bitslice may be aliased and must use interior mutability to access all
/// of its bytes, but does not have any partially-referenced bytes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AliasedNoEdges;

/// This bitslice must use interior mutability to access its
/// partially-referenced bytes.
///
/// The main difference between this and [`Aliased`] is that bit slices
/// with this aliasing type uphold the "aliasing XOR mutability" semantically
/// with respect to their bits, as well as technically with respect
/// to their "inner" bytes, so interior-mutable accesses are only needed at the
/// edges.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct AliasedEdgesOnly;

/// This bitslice can access its partially-referenced bytes normally.
///
/// The main difference between this and [`AliasedEdgesOnly`] is that bit slices
/// with this aliasing type uphold the "aliasing XOR mutability" rule with
/// respect to all of their underlying bytes, not just the "inner" bytes, so
/// interior-mutable accesses are not needed at the edges.
///
/// The bits on the edges of this slice are not important and may be freely
/// overwritten if this slice is mutable.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Unaliased;

/// This bitslice is unaliased and does not have any partially-referenced bytes.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct UnaliasedNoEdges;

/// This bitslice consists of only one partially-referenced byte.
/// The aliasing of that byte is given by the `A` type parameter.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct JustAnEdge<A: Aliasing>(PhantomData<A>);

unsafe impl Aliasing for Aliased {
    type NoEdges = AliasedNoEdges;
    const SEMANTICALLY_ALIASED: bool = true;

    fn fill<'a, M: MutMutability>(
        slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    ) {
        let value = if value { 255 } else { 0 };
        for (byte, bits) in slice.raw_bytes() {
            unsafe {
                Self::store_byte::<M>(
                    byte,
                    bits.mask() == 255,
                    value,
                    bits.mask(),
                );
            }
        }
    }

    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8 {
        debug_assert!(slice.bits.into_range().contains(&bit_idx));
        let byte_idx = bit_idx / 8;
        let byte_ptr =
            unsafe { slice.data.as_ptr().cast_const().add(byte_idx) };
        let byte: &<M as Mutability>::Edge = unsafe { &*byte_ptr.cast() };
        byte.load(Ordering::Relaxed)
    }

    unsafe fn load_byte<M: Mutability>(byte: *const u8, _is_inner: bool) -> u8 {
        let edge: &<M as Mutability>::Edge = unsafe { &*byte.cast() };
        edge.load(Ordering::Relaxed)
    }

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        _is_inner: bool,
        value: u8,
        mask: u8,
    ) {
        let byte: &<M as Mutability>::Edge =
            unsafe { &*byte.cast_const().cast() };
        if mask == 255 {
            byte.store(value, Ordering::Relaxed);
        } else if value & mask == 0 {
            byte.fetch_and(!mask, Ordering::Relaxed);
        } else if value & mask == mask {
            byte.fetch_or(mask, Ordering::Relaxed);
        } else {
            byte.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |byte| {
                let new_byte = (byte & !mask) | (value & mask);
                Some(new_byte)
            })
            .ok();
        }
    }
}

unsafe impl Aliasing for AliasedNoEdges {
    type NoEdges = AliasedNoEdges;
    const SEMANTICALLY_ALIASED: bool = true;

    fn fill<'a, M: MutMutability>(
        slice: BaseBitSlice<'a, M, Self>,
        value: bool,
    ) {
        let value = if value { 255 } else { 0 };
        for (byte, bits) in slice.raw_bytes() {
            debug_assert_eq!(bits.mask(), 255);
            unsafe { Self::store_byte::<M>(byte, true, value, 255) }
        }
    }

    unsafe fn load_byte_containing<'a, M: Mutability>(
        slice: BaseBitSlice<'a, M, Self>,
        bit_idx: usize,
    ) -> u8 {
        unsafe {
            Aliased::load_byte_containing::<M>(
                transmute!(slice as BaseBitSlice),
                bit_idx,
            )
        }
    }

    unsafe fn load_byte<M: Mutability>(byte: *const u8, is_inner: bool) -> u8 {
        unsafe { Aliased::load_byte::<M>(byte, is_inner) }
    }

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        is_inner: bool,
        value: u8,
        mask: u8,
    ) {
        debug_assert!(is_inner);
        unsafe { Aliased::store_byte::<M>(byte, true, value, mask) }
    }
}

unsafe impl Aliasing for AliasedEdgesOnly {
    type NoEdges = UnaliasedNoEdges;
    const SEMANTICALLY_ALIASED: bool = false;

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

    unsafe fn load_byte<M: Mutability>(byte: *const u8, is_inner: bool) -> u8 {
        if !is_inner {
            unsafe { Aliased::load_byte::<M>(byte, is_inner) }
        } else {
            unsafe { Unaliased::load_byte::<M>(byte, is_inner) }
        }
    }

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        is_inner: bool,
        value: u8,
        mask: u8,
    ) {
        if !is_inner {
            unsafe { Aliased::store_byte::<M>(byte, is_inner, value, mask) }
        } else {
            unsafe { Unaliased::store_byte::<M>(byte, is_inner, value, mask) }
        }
    }
}

unsafe impl Aliasing for Unaliased {
    type NoEdges = UnaliasedNoEdges;
    const SEMANTICALLY_ALIASED: bool = false;

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

    unsafe fn load_byte<M: Mutability>(byte: *const u8, _is_inner: bool) -> u8 {
        unsafe { *byte }
    }

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        _is_inner: bool,
        value: u8,
        mask: u8,
    ) {
        unsafe {
            *byte = (*byte & !mask) | (value & mask);
        }
    }
}
unsafe impl Aliasing for UnaliasedNoEdges {
    type NoEdges = UnaliasedNoEdges;
    const SEMANTICALLY_ALIASED: bool = false;

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

    unsafe fn load_byte<M: Mutability>(byte: *const u8, _is_inner: bool) -> u8 {
        unsafe { Unaliased::load_byte::<M>(byte, true) }
    }

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        _is_inner: bool,
        value: u8,
        mask: u8,
    ) {
        unsafe { Unaliased::store_byte::<M>(byte, true, value, mask) }
    }
}
unsafe impl<A: Aliasing> Aliasing for JustAnEdge<A> {
    type NoEdges = A::NoEdges;
    const SEMANTICALLY_ALIASED: bool = A::SEMANTICALLY_ALIASED;

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

    unsafe fn load_byte<M: Mutability>(byte: *const u8, is_inner: bool) -> u8 {
        debug_assert!(
            !is_inner,
            "JustAnEdge statically guarantees that there are no inner bytes"
        );
        unsafe { A::load_byte::<M>(byte, is_inner) }
    }

    unsafe fn store_byte<M: MutMutability>(
        byte: *mut u8,
        is_inner: bool,
        value: u8,
        mask: u8,
    ) {
        unsafe { A::store_byte::<M>(byte, is_inner, value, mask) }
    }
}

unsafe impl UnaliasedAliasing for Unaliased {}
unsafe impl UnaliasedAliasing for UnaliasedNoEdges {}
unsafe impl<A: UnaliasedAliasing> UnaliasedAliasing for JustAnEdge<A> {}

unsafe impl UnaliasedInnerBytesAliasing for AliasedEdgesOnly {}
unsafe impl UnaliasedInnerBytesAliasing for Unaliased {}
unsafe impl UnaliasedInnerBytesAliasing for UnaliasedNoEdges {}
unsafe impl<A: UnaliasedInnerBytesAliasing> UnaliasedInnerBytesAliasing
    for JustAnEdge<A>
{
}
