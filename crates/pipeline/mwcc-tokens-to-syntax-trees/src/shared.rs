//! Copy-on-write parser state.
//!
//! Speculative C++ declaration probes clone the complete `Parser`. Its
//! collections grow with the translation unit, so deep copies made every probe
//! quadratic in the size of large C++ units. `Shared` makes a clone an `Arc`
//! bump; the first mutable access through a still-shared handle copies just
//! that collection.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

#[derive(Default, PartialEq, Eq)]
pub(crate) struct Shared<T>(Arc<T>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(Arc::clone(&self.0))
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Shared(Arc::new(value))
    }
}

impl<T: fmt::Debug> fmt::Debug for Shared<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<T: Clone> Shared<T> {
    /// Take the value, leaving a default, without copying when unshared.
    pub(crate) fn take(&mut self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut **self)
    }

    /// Recover the owned value, copying only if another handle remains.
    pub(crate) fn into_inner(self) -> T {
        Arc::try_unwrap(self.0).unwrap_or_else(|shared| (*shared).clone())
    }
}

impl<T: Clone + IntoIterator> IntoIterator for Shared<T> {
    type Item = T::Item;
    type IntoIter = T::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.into_inner().into_iter()
    }
}

impl<A, T: FromIterator<A>> FromIterator<A> for Shared<T> {
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Shared::from(T::from_iter(iter))
    }
}

impl<'a, T> IntoIterator for &'a Shared<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&*self.0).into_iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut Shared<T>
where
    &'a mut T: IntoIterator,
{
    type Item = <&'a mut T as IntoIterator>::Item;
    type IntoIter = <&'a mut T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        Arc::make_mut(&mut self.0).into_iter()
    }
}
