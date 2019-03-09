use fnv::FnvHashMap;
use itertools::multizip;
use std::iter::Repeat;
use std::iter::Take;
use std::marker::PhantomData;
use std::slice::Iter;
use std::slice::IterMut;

#[cfg(feature = "rayon")]
use rayon::prelude::*;

use crate::*;

/// A type which can construct a default entity filter.
pub trait DefaultFilter {
    /// The type of entity filter constructed.
    type Filter: Filter;

    /// constructs an entity filter.
    fn filter() -> Self::Filter;
}

/// A type which can fetch a strongly-typed view of the data contained
/// within a `Chunk`.
pub trait View<'a>: Sized + Send + Sync + 'static {
    /// The iterator over the chunk data.
    type Iter: Iterator + 'a;

    /// Pulls data out of a chunk.
    fn fetch(chunk: &'a Chunk) -> Self::Iter;

    /// Validates that the view does not break any component borrowing rules.
    fn validate() -> bool;

    /// Determines if the view reads the specified data type.
    fn reads<T: EntityData>() -> bool;

    /// Determines if the view writes to the specified data type.
    fn writes<T: EntityData>() -> bool;
}

#[doc(hidden)]
pub trait ViewElement {
    type Component;
}

/// Converts a `View` into a `Query`.
pub trait IntoQuery: DefaultFilter + for<'a> View<'a> {
    /// Converts the `View` type into a `Query`.
    fn query() -> QueryDef<Self, <Self as DefaultFilter>::Filter>;
}

impl<T: DefaultFilter + for<'a> View<'a>> IntoQuery for T {
    fn query() -> QueryDef<Self, <Self as DefaultFilter>::Filter> {
        if !Self::validate() {
            panic!("invalid view, please ensure the view contains no duplicate component types");
        }

        QueryDef {
            view: PhantomData,
            filter: Self::filter(),
        }
    }
}

/// Reads a single entity data component type from a `Chunk`.
#[derive(Debug)]
pub struct Read<T: EntityData>(PhantomData<T>);

impl<T: EntityData> DefaultFilter for Read<T> {
    type Filter = EntityDataFilter<T>;

    fn filter() -> Self::Filter {
        EntityDataFilter::new()
    }
}

impl<'a, T: EntityData> View<'a> for Read<T> {
    type Iter = BorrowedIter<'a, Iter<'a, T>>;

    fn fetch(chunk: &'a Chunk) -> Self::Iter {
        chunk.entity_data().unwrap().into_iter()
    }

    fn validate() -> bool {
        true
    }

    fn reads<D: EntityData>() -> bool {
        TypeId::of::<T>() == TypeId::of::<D>()
    }

    fn writes<D: EntityData>() -> bool {
        false
    }
}

impl<T: EntityData> ViewElement for Read<T> {
    type Component = T;
}

/// Writes to a single entity data component type in a `Chunk`.
#[derive(Debug)]
pub struct Write<T: EntityData>(PhantomData<T>);

impl<T: EntityData> DefaultFilter for Write<T> {
    type Filter = EntityDataFilter<T>;
    fn filter() -> Self::Filter {
        EntityDataFilter::new()
    }
}

impl<'a, T: EntityData> View<'a> for Write<T> {
    type Iter = BorrowedIter<'a, IterMut<'a, T>>;

    fn fetch(chunk: &'a Chunk) -> Self::Iter {
        chunk.entity_data_mut().unwrap().into_iter()
    }

    fn validate() -> bool {
        true
    }

    fn reads<D: EntityData>() -> bool {
        TypeId::of::<T>() == TypeId::of::<D>()
    }

    fn writes<D: EntityData>() -> bool {
        TypeId::of::<T>() == TypeId::of::<D>()
    }
}

impl<T: EntityData> ViewElement for Write<T> {
    type Component = T;
}

/// Reads a single shared data component type in a `Chunk`.
#[derive(Debug)]
pub struct Shared<T: SharedData>(PhantomData<T>);

impl<T: SharedData> DefaultFilter for Shared<T> {
    type Filter = SharedDataFilter<T>;
    fn filter() -> Self::Filter {
        SharedDataFilter::new()
    }
}

impl<'a, T: SharedData> View<'a> for Shared<T> {
    type Iter = Take<Repeat<&'a T>>;

    fn fetch(chunk: &'a Chunk) -> Self::Iter {
        let data: &T = chunk.shared_data().unwrap();
        std::iter::repeat(data).take(chunk.len())
    }

    fn validate() -> bool {
        true
    }

    fn reads<D: EntityData>() -> bool {
        false
    }

    fn writes<D: EntityData>() -> bool {
        false
    }
}

impl<T: SharedData> ViewElement for Shared<T> {
    type Component = Shared<T>;
}

macro_rules! impl_view_tuple {
    ( $( $ty: ident ),* ) => {
        impl<$( $ty: ViewElement + DefaultFilter ),*> DefaultFilter for ($( $ty, )*) {
            type Filter = And<($( $ty::Filter, )*)>;

            fn filter() -> Self::Filter {
                And {
                    filters: ($( $ty::filter(), )*)
                }
            }
        }

        impl<'a, $( $ty: ViewElement + View<'a> ),* > View<'a> for ($( $ty, )*) {
            type Iter = itertools::Zip<($( $ty::Iter, )*)>;

            fn fetch(chunk: &'a Chunk) -> Self::Iter {
                multizip(($( $ty::fetch(chunk), )*))
            }

            fn validate() -> bool {
                let types = &[$( TypeId::of::<$ty::Component>() ),*];
                for i in 0..types.len() {
                    for j in (i + 1)..types.len() {
                        if unsafe { types.get_unchecked(i) == types.get_unchecked(j) } {
                            return false;
                        }
                    }
                }

                true
            }

            fn reads<Data: EntityData>() -> bool {
                $( $ty::reads::<Data>() )||*
            }

            fn writes<Data: EntityData>() -> bool {
                $( $ty::reads::<Data>() )||*
            }
        }
    };
}

impl_view_tuple!(A);
impl_view_tuple!(A, B);
impl_view_tuple!(A, B, C);
impl_view_tuple!(A, B, C, D);
impl_view_tuple!(A, B, C, D, E);

/// Filters chunks to determine which are to be included in a `Query`.
pub trait Filter: Sync + Sized {
    /// Determines if an archetype matches the filter's conditions.
    fn filter_archetype(&self, archetype: &Archetype) -> bool;

    /// Determines if a chunk matches the filter's conditions.
    fn filter_chunk(&self, chunk: &Chunk) -> bool;
}

pub mod filter {
    ///! Contains functions for constructing filters.
    use super::*;

    /// Creates an entity data filter which includes chunks that contain
    /// entity data components of type `T`.
    pub fn entity_data<T: EntityData>() -> EntityDataFilter<T> {
        EntityDataFilter::new()
    }

    /// Creates a shared data filter which includes chunks that contain
    /// shared data components of type `T`.
    pub fn shared_data<T: SharedData>() -> SharedDataFilter<T> {
        SharedDataFilter::new()
    }

    /// Creates a shared data filter which includes chunks that contain
    /// specific shared data values.
    pub fn shared_data_value<'a, T: SharedData>(data: &'a T) -> SharedDataValueFilter<'a, T> {
        SharedDataValueFilter::new(data)
    }

    /// Creates a filter which includes chunks for which entity data components
    /// of type `T` have changed since the filter was last executed.
    pub fn changed<T: EntityData>() -> EntityDataChangedFilter<T> {
        EntityDataChangedFilter::new()
    }
}

/// A passthrough filter which allows all chunks.
#[derive(Debug)]
pub struct Passthrough;

impl Filter for Passthrough {
    #[inline]
    fn filter_archetype(&self, _: &Archetype) -> bool {
        true
    }

    #[inline]
    fn filter_chunk(&self, _: &Chunk) -> bool {
        true
    }
}

impl std::ops::Not for Passthrough {
    type Output = Not<Self>;

    fn not(self) -> Self::Output {
        Not { filter: self }
    }
}

impl<Rhs: Filter> std::ops::BitAnd<Rhs> for Passthrough {
    type Output = And<(Self, Rhs)>;

    fn bitand(self, rhs: Rhs) -> Self::Output {
        And {
            filters: (self, rhs),
        }
    }
}

impl<Rhs: Filter> std::ops::BitOr<Rhs> for Passthrough {
    type Output = Or<(Self, Rhs)>;

    fn bitor(self, rhs: Rhs) -> Self::Output {
        Or {
            filters: (self, rhs),
        }
    }
}

/// A filter which negates `F`.
#[derive(Debug)]
pub struct Not<F> {
    filter: F,
}

impl<F: Filter> Filter for Not<F> {
    #[inline]
    fn filter_archetype(&self, archetype: &Archetype) -> bool {
        !self.filter.filter_archetype(archetype)
    }

    #[inline]
    fn filter_chunk(&self, chunk: &Chunk) -> bool {
        !self.filter.filter_chunk(chunk)
    }
}

impl<T: Filter> std::ops::Not for Not<T> {
    type Output = T;

    fn not(self) -> Self::Output {
        self.filter
    }
}

impl<F: Filter, Rhs: Filter> std::ops::BitAnd<Rhs> for Not<F> {
    type Output = And<(Self, Rhs)>;

    fn bitand(self, rhs: Rhs) -> Self::Output {
        And {
            filters: (self, rhs),
        }
    }
}

impl<F: Filter, Rhs: Filter> std::ops::BitOr<Rhs> for Not<F> {
    type Output = Or<(Self, Rhs)>;

    fn bitor(self, rhs: Rhs) -> Self::Output {
        Or {
            filters: (self, rhs),
        }
    }
}

/// A filter which requires all filters within `T` match.
#[derive(Debug)]
pub struct And<T> {
    filters: T,
}

macro_rules! impl_and_filter {
    ( $( $ty: ident ),* ) => {
        impl<$( $ty: Filter ),*> Filter for And<($( $ty, )*)> {
            #[inline]
            fn filter_archetype(&self, archetype: &Archetype) -> bool {
                #![allow(non_snake_case)]
                let ($( $ty, )*) = &self.filters;
                $( $ty.filter_archetype(archetype) )&&*
            }

            #[inline]
            fn filter_chunk(&self, chunk: &Chunk) -> bool {
                #![allow(non_snake_case)]
                let ($( $ty, )*) = &self.filters;
                $( $ty.filter_chunk(chunk) )&&*
            }
        }

        impl<$( $ty: Filter ),*> std::ops::Not for And<($( $ty, )*)> {
            type Output = Not<Self>;

            fn not(self) -> Self::Output {
                Not { filter: self }
            }
        }

        impl<$( $ty: Filter ),*, Rhs: Filter> std::ops::BitAnd<Rhs> for And<($( $ty, )*)> {
            type Output = And<($( $ty, )* Rhs)>;

            fn bitand(self, rhs: Rhs) -> Self::Output {
                #![allow(non_snake_case)]
                let ($( $ty, )*) = self.filters;
                And {
                    filters: ($( $ty, )* rhs),
                }
            }
        }

        impl<$( $ty: Filter ),*, Rhs: Filter> std::ops::BitOr<Rhs> for And<($( $ty, )*)> {
            type Output = Or<(Self, Rhs)>;

            fn bitor(self, rhs: Rhs) -> Self::Output {
                Or {
                    filters: (self, rhs),
                }
            }
        }
    };
}

impl_and_filter!(A);
impl_and_filter!(A, B);
impl_and_filter!(A, B, C);
impl_and_filter!(A, B, C, D);
impl_and_filter!(A, B, C, D, E);
impl_and_filter!(A, B, C, D, E, F);

/// A filter which requires that any filter within `T` match.
#[derive(Debug)]
pub struct Or<T> {
    filters: T,
}

macro_rules! impl_or_filter {
    ( $( $ty: ident ),* ) => {
        impl<$( $ty: Filter ),*> Filter for Or<($( $ty, )*)> {
            #[inline]
            fn filter_archetype(&self, archetype: &Archetype) -> bool {
                #![allow(non_snake_case)]
                let ($( $ty, )*) = &self.filters;
                $( $ty.filter_archetype(archetype) )||*
            }

            #[inline]
            fn filter_chunk(&self, chunk: &Chunk) -> bool {
                #![allow(non_snake_case)]
                let ($( $ty, )*) = &self.filters;
                $( $ty.filter_chunk(chunk) )||*
            }
        }

        impl<$( $ty: Filter ),*> std::ops::Not for Or<($( $ty, )*)> {
            type Output = Not<Self>;

            fn not(self) -> Self::Output {
                Not { filter: self }
            }
        }

        impl<$( $ty: Filter ),*, Rhs: Filter> std::ops::BitAnd<Rhs> for Or<($( $ty, )*)> {
            type Output = And<(Self, Rhs)>;

            fn bitand(self, rhs: Rhs) -> Self::Output {
                And {
                    filters: (self, rhs),
                }
            }
        }

        impl<$( $ty: Filter ),*, Rhs: Filter> std::ops::BitOr<Rhs> for Or<($( $ty, )*)> {
            type Output = Or<($( $ty, )* Rhs)>;

            fn bitor(self, rhs: Rhs) -> Self::Output {
                #![allow(non_snake_case)]
                let ($( $ty, )*) = self.filters;
                Or {
                    filters: ($( $ty, )* rhs),
                }
            }
        }
    };
}

impl_or_filter!(A);
impl_or_filter!(A, B);
impl_or_filter!(A, B, C);
impl_or_filter!(A, B, C, D);
impl_or_filter!(A, B, C, D, E);
impl_or_filter!(A, B, C, D, E, F);

/// A filter which requires the chunk contain entity data components of type `T`.
#[derive(Debug)]
pub struct EntityDataFilter<T>(PhantomData<T>);

impl<T: EntityData> EntityDataFilter<T> {
    fn new() -> Self {
        EntityDataFilter(PhantomData)
    }
}

impl<T: EntityData> Filter for EntityDataFilter<T> {
    #[inline]
    fn filter_archetype(&self, archetype: &Archetype) -> bool {
        archetype.has_component::<T>()
    }

    #[inline]
    fn filter_chunk(&self, _: &Chunk) -> bool {
        true
    }
}

impl<T: EntityData> std::ops::Not for EntityDataFilter<T> {
    type Output = Not<Self>;

    fn not(self) -> Self::Output {
        Not { filter: self }
    }
}

impl<Rhs: Filter, T: EntityData> std::ops::BitAnd<Rhs> for EntityDataFilter<T> {
    type Output = And<(Self, Rhs)>;

    fn bitand(self, rhs: Rhs) -> Self::Output {
        And {
            filters: (self, rhs),
        }
    }
}

impl<Rhs: Filter, T: EntityData> std::ops::BitOr<Rhs> for EntityDataFilter<T> {
    type Output = Or<(Self, Rhs)>;

    fn bitor(self, rhs: Rhs) -> Self::Output {
        Or {
            filters: (self, rhs),
        }
    }
}

/// A filter which requires the chunk contain shared data components of type `T`.
#[derive(Debug)]
pub struct SharedDataFilter<T>(PhantomData<T>);

impl<T: SharedData> SharedDataFilter<T> {
    fn new() -> Self {
        SharedDataFilter(PhantomData)
    }
}

impl<T: SharedData> Filter for SharedDataFilter<T> {
    #[inline]
    fn filter_archetype(&self, archetype: &Archetype) -> bool {
        archetype.has_shared::<T>()
    }

    #[inline]
    fn filter_chunk(&self, _: &Chunk) -> bool {
        true
    }
}

impl<T: SharedData> std::ops::Not for SharedDataFilter<T> {
    type Output = Not<Self>;

    fn not(self) -> Self::Output {
        Not { filter: self }
    }
}

impl<Rhs: Filter, T: SharedData> std::ops::BitAnd<Rhs> for SharedDataFilter<T> {
    type Output = And<(Self, Rhs)>;

    fn bitand(self, rhs: Rhs) -> Self::Output {
        And {
            filters: (self, rhs),
        }
    }
}

impl<Rhs: Filter, T: SharedData> std::ops::BitOr<Rhs> for SharedDataFilter<T> {
    type Output = Or<(Self, Rhs)>;

    fn bitor(self, rhs: Rhs) -> Self::Output {
        Or {
            filters: (self, rhs),
        }
    }
}

/// A filter which requires the chunk contain entity data values of a specific value.
#[derive(Debug)]
pub struct SharedDataValueFilter<'a, T> {
    value: &'a T,
}

impl<'a, T: SharedData> SharedDataValueFilter<'a, T> {
    fn new(value: &'a T) -> Self {
        SharedDataValueFilter { value }
    }
}

impl<'a, T: SharedData> Filter for SharedDataValueFilter<'a, T> {
    #[inline]
    fn filter_archetype(&self, _: &Archetype) -> bool {
        true
    }

    #[inline]
    fn filter_chunk(&self, chunk: &Chunk) -> bool {
        chunk.shared_data::<T>().map_or(false, |s| s == self.value)
    }
}

impl<'a, T: SharedData> std::ops::Not for SharedDataValueFilter<'a, T> {
    type Output = Not<Self>;

    fn not(self) -> Self::Output {
        Not { filter: self }
    }
}

impl<'a, Rhs: Filter, T: SharedData> std::ops::BitAnd<Rhs> for SharedDataValueFilter<'a, T> {
    type Output = And<(Self, Rhs)>;

    fn bitand(self, rhs: Rhs) -> Self::Output {
        And {
            filters: (self, rhs),
        }
    }
}

impl<'a, Rhs: Filter, T: SharedData> std::ops::BitOr<Rhs> for SharedDataValueFilter<'a, T> {
    type Output = Or<(Self, Rhs)>;

    fn bitor(self, rhs: Rhs) -> Self::Output {
        Or {
            filters: (self, rhs),
        }
    }
}

/// A filter which requires that entity data of type `T` has changed within the
/// chunk since the last time the filter was executed.
pub struct EntityDataChangedFilter<T: EntityData> {
    versions: Mutex<FnvHashMap<ChunkId, usize>>,
    phantom: PhantomData<T>,
}

impl<T: EntityData> EntityDataChangedFilter<T> {
    fn new() -> EntityDataChangedFilter<T> {
        EntityDataChangedFilter {
            versions: Mutex::new(FnvHashMap::default()),
            phantom: PhantomData,
        }
    }
}

impl<T: EntityData> std::ops::Not for EntityDataChangedFilter<T> {
    type Output = Not<Self>;

    fn not(self) -> Self::Output {
        Not { filter: self }
    }
}

impl<T: EntityData> Filter for EntityDataChangedFilter<T> {
    #[inline]
    fn filter_archetype(&self, _: &Archetype) -> bool {
        true
    }

    fn filter_chunk(&self, chunk: &Chunk) -> bool {
        use std::collections::hash_map::Entry;
        if let Some(version) = chunk.entity_data_version::<T>() {
            let mut versions = self.versions.lock();
            match versions.entry(chunk.id()) {
                Entry::Occupied(mut entry) => {
                    let existing = entry.get_mut();
                    let changed = *existing != version;
                    *existing = version;
                    changed
                }
                Entry::Vacant(entry) => {
                    entry.insert(version);
                    true
                }
            }
        } else {
            false
        }
    }
}

impl<Rhs: Filter, T: EntityData> std::ops::BitAnd<Rhs> for EntityDataChangedFilter<T> {
    type Output = And<(Self, Rhs)>;

    fn bitand(self, rhs: Rhs) -> Self::Output {
        And {
            filters: (self, rhs),
        }
    }
}

impl<Rhs: Filter, T: EntityData> std::ops::BitOr<Rhs> for EntityDataChangedFilter<T> {
    type Output = Or<(Self, Rhs)>;

    fn bitor(self, rhs: Rhs) -> Self::Output {
        Or {
            filters: (self, rhs),
        }
    }
}

/// An iterator which filters chunks by filter `F` and yields `ChunkView`s.
pub struct ChunkViewIter<'data, 'filter, V: View<'data>, F: Filter> {
    archetypes: Iter<'data, Archetype>,
    filter: &'filter F,
    frontier: Option<Iter<'data, Chunk>>,
    view: PhantomData<V>,
}

impl<'filter, 'data, F, V> Iterator for ChunkViewIter<'data, 'filter, V, F>
where
    F: Filter,
    V: View<'data>,
{
    type Item = ChunkView<'data, V>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut inner) = self.frontier {
                for x in &mut inner.next() {
                    if self.filter.filter_chunk(x) {
                        return Some(ChunkView {
                            chunk: x,
                            view: PhantomData,
                        });
                    }
                }
            }
            loop {
                match self.archetypes.next() {
                    Some(archetype) => {
                        if self.filter.filter_archetype(archetype) {
                            self.frontier = Some(archetype.chunks().iter());
                            break;
                        }
                    }
                    None => return None,
                }
            }
        }
    }
}

/// An iterator which iterates through all entity data in all chunks.
pub struct ChunkDataIter<'data, 'query, V: View<'data>, F: Filter> {
    iter: ChunkViewIter<'data, 'query, V, F>,
    frontier: Option<V::Iter>,
    view: PhantomData<V>,
}

impl<'data, 'query, F: Filter, V: View<'data>> Iterator for ChunkDataIter<'data, 'query, V, F> {
    type Item = <V::Iter as Iterator>::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut inner) = self.frontier {
                if let elt @ Some(_) = inner.next() {
                    return elt;
                }
            }
            match self.iter.next() {
                Some(mut inner) => self.frontier = Some(inner.iter()),
                None => return None,
            }
        }
    }
}

/// An iterator which iterates through all entity data in all chunks, zipped with entity ID.
pub struct ChunkEntityIter<'data, 'query, V: View<'data>, F: Filter> {
    iter: ChunkViewIter<'data, 'query, V, F>,
    frontier: Option<ZipEntities<'data, V>>,
    view: PhantomData<V>,
}

impl<'data, 'query, V: View<'data>, F: Filter> Iterator for ChunkEntityIter<'data, 'query, V, F> {
    type Item = (Entity, <V::Iter as Iterator>::Item);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut inner) = self.frontier {
                if let elt @ Some(_) = inner.next() {
                    return elt;
                }
            }
            match self.iter.next() {
                Some(mut inner) => self.frontier = Some(inner.iter_entities()),
                None => return None,
            }
        }
    }
}

/// Queries for entities within a `World`.
///
/// # Examples
///
/// Queries can be constructed from any `View` type, including tuples of `View`s.
///
/// ```rust
/// # use legion::prelude::*;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Position;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Velocity;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Model;
/// // A query which matches any entity with a `Position` component
/// let query = Read::<Position>::query();
///
/// // A query which matches any entity with both a `Position` and a `Velocity` component
/// let query = <(Read<Position>, Read<Velocity>)>::query();
/// ```
///
/// The view determines what data is accessed, and whether it is accessed mutably or not.
///
/// ```rust
/// # use legion::prelude::*;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Position;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Velocity;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Model;
/// // A query which writes `Position`, reads `Velocity` and reads `Model`
/// // Shared data is read-only, and is distinguished from entity data reads with `Shared<T>`.
/// let query = <(Write<Position>, Read<Velocity>, Shared<Model>)>::query();
/// ```
///
/// By default, a query will filter its results to include only entities with the data
/// types accessed by the view. However, additional filters can be specified if needed:
///
/// ```rust
/// # use legion::prelude::*;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Position;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Velocity;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Model;
/// #[derive(Copy, Clone, Debug, PartialEq)]
/// struct Static;
///
/// // A query which also requires that entities have `Static` shared data
/// // In this case, `Static` is used as a marker type
/// let query = <(Read<Position>, Shared<Model>)>::query().filter(shared_data::<Static>());
/// ```
///
/// Filters can be combined with bitwise operators:
///
/// ```rust
/// # use legion::prelude::*;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Position;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Velocity;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Model;
/// #[derive(Copy, Clone, Debug, PartialEq)]
/// struct Static;
///
/// // This query matches entities with positions and a model
/// // But it also requires that the entity is not static, or has moved (even if static)
/// let query = <(Read<Position>, Shared<Model>)>::query()
///     .filter(!shared_data::<Static>() | changed::<Position>());
/// ```
///
/// Filters can be iterated through to pull data out of a `World`:
///
/// ```rust
/// # use legion::prelude::*;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Position;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Velocity;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Model;
/// # let universe = Universe::new(None);
/// # let world = universe.create_world();
/// // A query which writes `Position`, reads `Velocity` and reads `Model`
/// // Shared data is read-only, and is distinguished from entity data reads with `Shared<T>`.
/// let query = <(Write<Position>, Read<Velocity>, Shared<Model>)>::query();
///
/// for (pos, vel, model) in query.iter(&world) {
///     // `.iter` yields tuples of references to a single entity's data:
///     // pos: &mut Position
///     // vel: &Velocity
///     // model: &Model
/// }
/// ```
///
/// The lower level `iter_chunks` function allows access to each underlying chunk of entity data.
/// This allows you to run code for each shared data value, or to retrieve a contiguous data slice.
///
/// ```rust
/// # use legion::prelude::*;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Position;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Velocity;
/// # #[derive(Copy, Clone, Debug, PartialEq)]
/// # struct Model;
/// # let universe = Universe::new(None);
/// # let world = universe.create_world();
/// let query = <(Write<Position>, Read<Velocity>, Shared<Model>)>::query();
///
/// for chunk in query.iter_chunks(&world) {
///     let model = chunk.shared_data::<Model>();
///     let positions = chunk.data_mut::<Position>();
///     let velocities = chunk.data::<Velocity>();
/// }
/// ```
///
/// The `ChunkView` yielded from `iter_chunks` allows access to all shared data in the chunk (queried for or not),
/// but entity data slices can only be accessed if they were requested in the query's view. Attempting to access
/// other data types, or attempting to write to components that were only requested via a `Read` will panic.
pub trait Query {
    /// The chunk filter used to determine which chunks to include in the output.
    type Filter: Filter;

    /// The view used to determine which components are accessed.
    type View: for<'data> View<'data>;

    /// Adds an additional filter to the query.
    fn filter<T: Filter>(self, filter: T) -> QueryDef<Self::View, And<(Self::Filter, T)>>;

    /// Gets an iterator which iterates through all chunks that match the query.
    fn iter_chunks<'a, 'data>(
        &'a self,
        world: &'data World,
    ) -> ChunkViewIter<'data, 'a, Self::View, Self::Filter>;

    /// Gets an iterator which iterates through all entity data that matches the query.
    fn iter<'a, 'data>(
        &'a self,
        world: &'data World,
    ) -> ChunkDataIter<'data, 'a, Self::View, Self::Filter>;

    /// Gets an iterator which iterates through all entity data that matches the query, and also yields the the `Entity` IDs.
    fn iter_entities<'a, 'data>(
        &'a self,
        world: &'data World,
    ) -> ChunkEntityIter<'data, 'a, Self::View, Self::Filter>;

    /// Iterates through all entity data that matches the query.
    fn for_each<'a, 'data, T>(&'a self, world: &'data World, mut f: T)
    where
        T: Fn(<<Self::View as View<'data>>::Iter as Iterator>::Item),
    {
        self.iter(world).for_each(&mut f);
    }

    /// Iterates through all entity data that matches the query in parallel.
    #[cfg(feature = "par-iter")]
    fn par_for_each<'a, T>(&'a self, world: &'a World, f: T)
    where
        T: Fn(<<Self::View as View<'a>>::Iter as Iterator>::Item) + Send + Sync;
}

/// Queries for entities within a `World`.
#[derive(Debug)]
pub struct QueryDef<V: for<'a> View<'a>, F: Filter> {
    view: PhantomData<V>,
    filter: F,
}

impl<V: for<'a> View<'a>, F: Filter> Query for QueryDef<V, F> {
    type View = V;
    type Filter = F;

    fn filter<T: Filter>(self, filter: T) -> QueryDef<Self::View, And<(Self::Filter, T)>> {
        QueryDef {
            view: self.view,
            filter: And {
                filters: (self.filter, filter),
            },
        }
    }

    fn iter_chunks<'a, 'data>(
        &'a self,
        world: &'data World,
    ) -> ChunkViewIter<'data, 'a, Self::View, Self::Filter> {
        ChunkViewIter {
            archetypes: world.archetypes.iter(),
            filter: &self.filter,
            frontier: None,
            view: PhantomData,
        }
    }

    fn iter<'a, 'data>(
        &'a self,
        world: &'data World,
    ) -> ChunkDataIter<'data, 'a, Self::View, Self::Filter> {
        ChunkDataIter {
            iter: self.iter_chunks(world),
            frontier: None,
            view: PhantomData,
        }
    }

    fn iter_entities<'a, 'data>(
        &'a self,
        world: &'data World,
    ) -> ChunkEntityIter<'data, 'a, Self::View, Self::Filter> {
        ChunkEntityIter {
            iter: self.iter_chunks(world),
            frontier: None,
            view: PhantomData,
        }
    }

    #[cfg(feature = "par-iter")]
    fn par_for_each<'a, T>(&'a self, world: &'a World, f: T)
    where
        T: Fn(<<V as View<'a>>::Iter as Iterator>::Item) + Send + Sync,
    {
        self.par_iter_chunks(world).for_each(|mut chunk| {
            for data in chunk.iter() {
                f(data);
            }
        });
    }
}

impl<V: for<'a> View<'a>, F: Filter> QueryDef<V, F> {
    /// Gets a parallel iterator of chunks that match the query.
    #[cfg(feature = "par-iter")]
    pub fn par_iter_chunks<'a>(
        &'a self,
        world: &'a World,
    ) -> impl ParallelIterator<Item = ChunkView<'a, V>> {
        let filter = &self.filter;
        let archetypes = &world.archetypes;
        archetypes
            .par_iter()
            .filter(move |a| filter.filter_archetype(a))
            .flat_map(|a| a.chunks())
            .filter(move |c| filter.filter_chunk(c))
            .map(|c| ChunkView {
                chunk: c,
                view: PhantomData,
            })
    }
}

/// An iterator which yields view data tuples and entity IDs from a `ChunkView`.
pub struct ZipEntities<'data, V: View<'data>> {
    entities: &'data [Entity],
    data: <V as View<'data>>::Iter,
    index: usize,
    view: PhantomData<V>,
}

impl<'data, V: View<'data>> Iterator for ZipEntities<'data, V> {
    type Item = (Entity, <V::Iter as Iterator>::Item);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(data) = self.data.next() {
            let i = self.index;
            self.index += 1;
            unsafe { Some((*self.entities.get_unchecked(i), data)) }
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.entities.len() - self.index;
        (len, Some(len))
    }
}

/// A type-safe view of a `Chunk`.
#[derive(Debug)]
pub struct ChunkView<'a, V: View<'a>> {
    chunk: &'a Chunk,
    view: PhantomData<V>,
}

impl<'a, V: View<'a>> ChunkView<'a, V> {
    /// Get a slice of all entities contained within the chunk.
    pub fn entities(&self) -> &'a [Entity] {
        unsafe { self.chunk.entities() }
    }

    /// Get an iterator of all data contained within the chunk.
    pub fn iter(&mut self) -> V::Iter {
        V::fetch(self.chunk)
    }

    /// Get an iterator of all data and entity IDs contained within the chunk.
    pub fn iter_entities(&mut self) -> ZipEntities<'a, V> {
        ZipEntities {
            entities: self.entities(),
            data: V::fetch(self.chunk),
            index: 0,
            view: PhantomData,
        }
    }

    /// Get a shared data value.
    pub fn shared_data<T: SharedData>(&self) -> Option<&T> {
        self.chunk.shared_data()
    }

    /// Get a slice of entity data.
    ///
    /// # Panics
    ///
    /// This method performs runtime borrow checking. It will panic if
    /// any other code is concurrently writing to the data slice.
    pub fn data<T: EntityData>(&self) -> Option<BorrowedSlice<'a, T>> {
        if !V::reads::<T>() {
            panic!("data type not readable via this query");
        }
        self.chunk.entity_data()
    }

    /// Get a mutable slice of entity data.
    ///
    /// # Panics
    ///
    /// This method performs runtime borrow checking. It will panic if
    /// any other code is concurrently accessing the data slice.
    pub fn data_mut<T: EntityData>(&self) -> Option<BorrowedMutSlice<'a, T>> {
        if !V::writes::<T>() {
            panic!("data type not writable via this query");
        }
        self.chunk.entity_data_mut()
    }
}