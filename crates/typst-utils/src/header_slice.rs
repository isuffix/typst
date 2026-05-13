use slice_dst::SliceWithHeader;

use std::hash::Hash;
use std::iter::FusedIterator;
use std::ops::Range;
use std::sync::Arc;

/// An [`Arc`]-owned typed containing a header struct and a slice of items
/// stored contiguously in memory as a "dynamically sized type." Storing a
/// header and slice together allows us to remove an indirection when accessing
/// the slice.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct ArcHeaderSlice<H, T>(Arc<SliceWithHeader<H, T>>);

impl<H: Clone, T: Clone + Default> ArcHeaderSlice<H, T> {
    /// Create a new [`ArcHeaderSlice`].
    pub fn new(header: H, vec: Vec<T>) -> Self {
        Self(SliceWithHeader::new(header, vec))
    }

    /// Access the header immutably.
    pub fn header(&self) -> &H {
        &self.0.header
    }

    /// Access the header mutably. This will implicitly clone if there is more
    /// than one reference to the underlying [`Arc`].
    pub fn header_mut(&mut self) -> &mut H {
        &mut self.make_mut().header
    }

    /// Access the slice immutably.
    pub fn slice(&self) -> &[T] {
        &self.0.slice
    }

    /// Access the slice mutably. This will implicitly clone if there is more
    /// than one reference to the underlying [`Arc`].
    pub fn slice_mut(&mut self) -> &mut [T] {
        &mut self.make_mut().slice
    }

    /// Internal mutable accessor. We can't use [`Arc::make_mut`] since it has a
    /// `CloneToUninit` bound on the inner type which is unstable. Instead we
    /// duplicate its functionality.
    fn make_mut(&mut self) -> &mut SliceWithHeader<H, T> {
        if Arc::get_mut(&mut self.0).is_none() {
            let header = self.0.header.clone();
            let slice_iter = self.0.slice.iter().cloned();
            self.0 = SliceWithHeader::new(header, slice_iter);
        }
        Arc::get_mut(&mut self.0).unwrap()
    }

    /// Overwrite the header and replace items in a range of the slice with new
    /// items. Unless the new items and the range have the same length, this
    /// will allocate a new slice.
    pub fn replace_range(
        &mut self,
        header: H,
        range: Range<usize>,
        new_items: impl ExactSizeIterator<Item = T>,
    ) {
        if range.len() == new_items.len() {
            // We can reuse the same allocation :)
            if let Some(inner) = Arc::get_mut(&mut self.0) {
                inner.header = header;
            } else {
                let slice_iter = self.0.slice.iter().cloned();
                self.0 = SliceWithHeader::new(header, slice_iter);
            }
            let inner = Arc::get_mut(&mut self.0).unwrap();
            for (old, new) in inner.slice[range].iter_mut().zip(new_items) {
                *old = new;
            }
        } else {
            // We need to reallocate the slice :(
            if let Some(inner) = Arc::get_mut(&mut self.0) {
                // We don't need to clone the replaced items :)
                // But we do need `std::mem::take` to move them safely >:(
                let (before, mid) = inner.slice.split_at_mut(range.start);
                let (_drop, after) = mid.split_at_mut(range.end - before.len());
                self.0 = SliceWithHeader::new(
                    header,
                    KnownLenIter {
                        remaining: before.len() + new_items.len() + after.len(),
                        iter: before
                            .iter_mut()
                            .map(std::mem::take) // This is where we use the `Default` bound on `T`.
                            .chain(new_items)
                            .chain(after.iter_mut().map(std::mem::take)),
                    },
                );
            } else {
                // We need to clone the replaced items :(
                let (before, mid) = self.0.slice.split_at(range.start);
                let (_drop, after) = mid.split_at(range.end - before.len());
                self.0 = SliceWithHeader::new(
                    header,
                    KnownLenIter {
                        remaining: before.len() + new_items.len() + after.len(),
                        iter: before
                            .iter()
                            .cloned()
                            .chain(new_items)
                            .chain(after.iter().cloned()),
                    },
                );
            }
        }
    }
}

struct KnownLenIter<Item, Iter: Iterator<Item = Item>> {
    remaining: usize,
    iter: Iter,
}

impl<Item, Iter: Iterator<Item = Item>> Iterator for KnownLenIter<Item, Iter> {
    type Item = Item;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.iter.next();
        if self.remaining == 0 {
            assert!(item.is_none());
        } else {
            assert!(item.is_some());
            self.remaining -= 1;
        }
        item
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<Item, Iter: Iterator<Item = Item>> FusedIterator for KnownLenIter<Item, Iter> {}

impl<Item, Iter: Iterator<Item = Item>> ExactSizeIterator for KnownLenIter<Item, Iter> {
    fn len(&self) -> usize {
        self.remaining
    }
}
