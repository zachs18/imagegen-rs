use std::{cell::Cell, marker::PhantomData, sync::atomic::AtomicU8};

use radium::Radium;

/// The mutability of a [`BitMapView`] or [`BaseBitSlice`].
///
/// The types implementing this trait can be either represent mutable
/// ([`MutMutability`]) or immutable ([`ConstMutability`]) bit collections, and
/// can orthogonally either represent thread-safe ([`SyncMutability`]) or
/// non-thread-safe ([`UnsyncMutability`]) bit collections.
///
/// Note that the thread-safety of a non-semantically-aliased bit collection
/// type only applies to the "edges" of the bit collection, i.e. bytes
/// to which the bit collection does not wholly refer, and whose other bits may
/// have a different mutability under a different reference. The thread-safety
/// of such a bit collection can be changed if the bit collection only refers to
/// entire bytes, or (in the case of [`UnaliasedBitSlice`]) as a static
/// guarantee that the other bits of the "edges" are not referred to by anything
/// that could violate aliasing rules by using non-atomic loads/stores.
pub unsafe trait Mutability: Sized + std::fmt::Debug + 'static {
    type Mut: MutMutability;
    type Const: ConstMutability;
    type Sync: SyncMutability;
    type Unsync: UnsyncMutability;

    /// Must be either `Cell<u8>` or `AtomicU8`.
    type Edge: Radium<Item = u8>;
}

/// Bit slices with this mutability allow mutating the bits of the slice.
pub unsafe trait MutMutability: Mutability<Mut = Self> {}
unsafe impl<M: Mutability<Mut = M>> MutMutability for M {}

/// Bit slices with this mutability do not allow mutating the bits of the slice.
///
/// Note that, if the bit slice's aliasing type is semantically aliased, the
/// bits may change through other references.
pub unsafe trait ConstMutability:
    Mutability<Const = Self> + Copy
{
}
unsafe impl<M: Mutability<Const = M> + Copy> ConstMutability for M {}

/// Bit slices with this mutability can be sent and shared between threads.
pub unsafe trait SyncMutability:
    Mutability<Edge = AtomicU8, Sync = Self>
{
}
unsafe impl<M: Mutability<Edge = AtomicU8, Sync = Self>> SyncMutability for M {}

/// Bit slices with this mutability cannot be sent or shared between threads.
pub unsafe trait UnsyncMutability:
    Mutability<Edge = Cell<u8>, Unsync = Self>
{
}
unsafe impl<M: Mutability<Edge = Cell<u8>, Unsync = Self>> UnsyncMutability
    for M
{
}

/// This mutability marker type indicates that a given bit slice reference is
/// mutable and thread-safe.
///
/// Bit slices references with this mutability are not `Copy`.
///
/// Note that unaliased bit slices can always be converted between
/// thread-safe and non-thread-safe versions.
///
/// ```rust
/// # use bitmap::{BitSlice, MutableSync, ConstUnsync, ByteBitRange};
/// let mut bytes = [42];
/// let slice = BitSlice::<MutableSync>::from_bytes_mut(&mut bytes, 0..6);
/// std::thread::scope(|scope| {
///     scope.spawn(|| {
///         assert_eq!(
///             slice.bits().collect::<Vec<bool>>(),
///             [false, true, false, true, false, true]
///         );
///     });
/// });
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct MutableSync;

/// This mutability marker type indicates that a given bit slice reference is
/// immutable and thread-safe.
///
/// Bit slices references with this mutability are `Copy`.
///
/// Note that unaliased bit slices can always be converted between
/// thread-safe and non-thread-safe versions.
///
/// ```rust
/// # use bitmap::{BitSlice, ConstSync, ConstUnsync, ByteBitRange};
/// let slice = BitSlice::<ConstSync>::from_value(42, ByteBitRange::from(0..6));
/// std::thread::spawn(move || {
///     assert_eq!(
///         slice.bits().collect::<Vec<bool>>(),
///         [false, true, false, true, false, true]
///     );
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

/// This mutability marker type indicates that a given bit slice references is
/// mutable and non-thread-safe.
///
/// Bit slices references with this mutability are not `Copy`.
///
/// Note that unaliased bit slices can always be converted between
/// thread-safe and non-thread-safe versions.
///
/// ```rust
/// # use bitmap::{BitSlice, MutableUnsync, ByteBitRange};
/// let mut bytes = [42];
/// let mut slice = BitSlice::<MutableUnsync>::from_bytes_mut(&mut bytes, 0..6);
/// assert_eq!(
///     slice.bits().collect::<Vec<bool>>(),
///     [false, true, false, true, false, true]
/// );
/// slice.fill(true);
/// assert_eq!(slice.bits().collect::<Vec<bool>>(), [true; 6]);
/// ```
///
///
/// ```rust,compile_fail,E0277
/// # use bitmap::{BitSlice, MutableUnsync, ByteBitRange};
/// let mut bytes = [42];
/// let mut slice = BitSlice::<MutableUnsync>::from_bytes_mut(&mut bytes, 0..6);
/// assert_eq!(slice.bits().collect::<Vec<bool>>(), [false, true, false, true, false, true]);
/// std::thread::scope(|scope| {
///     scope.spawn(|| {
///         slice.fill(true);
///     });
/// });
/// assert_eq!(slice.bits().collect::<Vec<bool>>(), [true; 6]);
/// ```
#[non_exhaustive]
#[derive(Debug)]
pub struct MutableUnsync(PhantomData<*const ()>);

/// This mutability marker type indicates that a given bit slice reference is
/// immutable and non-thread-safe.
///
/// Bit slices references with this mutability are `Copy`.
///
/// Note that unaliased bit slices can always be converted between
/// thread-safe and non-thread-safe versions.
///
/// ```rust
/// # use bitmap::{AliasedBitSlice, ConstUnsync, ByteBitRange};
/// let slice = AliasedBitSlice::<ConstUnsync>::from_value(
///     42,
///     ByteBitRange::from(0..6),
/// );
/// assert_eq!(
///     slice.bits().collect::<Vec<bool>>(),
///     [false, true, false, true, false, true]
/// );
/// ```
///
/// ```rust,compile_fail,E0277
/// # use bitmap::{BitSlice, ConstUnsync, ByteBitRange};
/// let slice = BitSlice::<ConstUnsync>::from_value(42, ByteBitRange::from(0..6));
/// assert_eq!(slice.bits().collect::<Vec<bool>>(), [false, true, false, true, false, true]);
/// std::thread::spawn(move || {
///     assert_eq!(slice.bits().collect::<Vec<bool>>(), [false, true, false, true, false, true]);
/// });
/// ```
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
