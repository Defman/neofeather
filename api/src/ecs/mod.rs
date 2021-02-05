use std::{collections::HashMap, convert::TryFrom, marker::PhantomData, vec};

mod type_layout;
use anyhow::Result;
pub use type_layout::*;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryAccess {
    None,
    Read(TypeLayout),
    Write(TypeLayout),
    Optional(Box<QueryAccess>),
    With(TypeLayout, Box<QueryAccess>),
    Without(TypeLayout, Box<QueryAccess>),
    Union(Vec<QueryAccess>),
}

impl QueryAccess {
    fn read<T: Component>() -> Self {
        QueryAccess::Read(T::layout())
    }

    fn write<T: Component>() -> Self {
        QueryAccess::Read(T::layout())
    }

    fn union(accesses: Vec<QueryAccess>) -> Self {
        QueryAccess::Union(accesses)
    }
}

pub trait Component: Serialize + DeserializeOwned + IntoTypeLayout + 'static {}

impl<T> Component for T where T: Serialize + DeserializeOwned + IntoTypeLayout + 'static {}

pub trait WorldQuery {
    type Fetch: for<'a> Fetch<'a>;
}

impl<'a, T> WorldQuery for &'a T
where
    T: Component,
{
    type Fetch = FetchRead<T>;
}

pub struct FetchRead<T>(T);

impl<'a, T: Component> Fetch<'a> for FetchRead<T> {
    type Item = &'a T;

    #[inline]
    fn access() -> QueryAccess {
        QueryAccess::read::<T>()
    }
}

pub struct FetchWrite<T>(T);

impl<'a, T: Component> Fetch<'a> for FetchWrite<T> {
    type Item = &'a mut T;

    fn access() -> QueryAccess {
        QueryAccess::write::<T>()
    }
}

impl<'a, T> WorldQuery for &'a mut T
where
    T: Component,
{
    type Fetch = FetchWrite<T>;
}

pub trait Fetch<'a>: Sized {
    type Item;

    fn access() -> QueryAccess;
}

macro_rules! tuples {
    (@rec) => {};
    (@rec $head:ident $(, $tail:ident)*) => {
        tuples!($($tail),*);
    };
    ($($generic:ident),*) => {
        impl<'a, $($generic),*> Fetch<'a> for ($($generic,)*)
        where
        $(
            $generic: WorldQuery
        ),*
        {
            type Item = ($($generic::Fetch,)*);

            fn access() -> QueryAccess {
                QueryAccess::union(vec![$($generic::Fetch::access()),*])
            }
        }

        impl<$($generic),*> WorldQuery for ($($generic,)*)
        where
        $(
            $generic: WorldQuery
        ),*
        {
            type Fetch = ($($generic,)*);
        }

        tuples!(@rec $($generic),*);
    };
}

tuples!(T4, T3, T2, T1);

pub struct Query<Q: WorldQuery> {
    entities: Vec<Q>,
}

impl<Q: WorldQuery> Query<Q> {
    fn from_data(data: Vec<Vec<u8>>) -> Result<Self> {
        todo!()
    }
    
    pub fn iter_mut(&mut self) -> QueryIter<Q> {
        QueryIter::new(&mut self.entities)
    }
}

pub struct QueryIter<'a, Q> {
    entities: &'a mut Vec<Q>,
    next: usize,
}

impl<'a, Q: WorldQuery> QueryIter<'a, Q> {
    fn new(entities: &'a mut Vec<Q>) -> Self {
        QueryIter { next: 0, entities }
    }
}

impl<'a, Q: WorldQuery> Iterator for QueryIter<'a, Q> {
    type Item = Q;
    fn next(&mut self) -> Option<Self::Item> {
        // let next = self.next;
        // self.next += 1;
        // self.entities.get_mut(next).map(|q| *q)
        todo!()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Entity {
    // TODO: Single Vec<u8> that can be deserialized into multiple Vec<u8>?
    pub components: Vec<(TypeLayout, Vec<u8>)>,
}
