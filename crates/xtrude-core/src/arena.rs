// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Insertion-ordered arena with never-reused typed ids.
//!
//! Every mesh entity lives in an arena slot; its id is the slot index and
//! is never reused, so:
//!
//! - iterating the arena visits entities in creation order (the order
//!   every tie-break relies on);
//! - a removed entity's id simply stops resolving: "this object is no
//!   longer in the mesh";
//! - cloning the arena keeps every id valid (an identity-preserving
//!   snapshot).

use std::fmt;
use std::marker::PhantomData;

/// A typed index into an [`Arena`].
pub struct Id<T> {
    raw: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Id {
            raw,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.raw
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.raw as usize
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<T> Eq for Id<T> {}
impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}
impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}
impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.raw)
    }
}

/// The arena itself: `Vec<Option<T>>` plus a live count.
#[derive(Clone)]
pub struct Arena<T> {
    slots: Vec<Option<T>>,
    live: usize,
    /// First slot that may still be live (skips the dead prefix on iteration).
    first: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            slots: Vec::new(),
            live: 0,
            first: 0,
        }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.live
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    pub fn insert(&mut self, value: T) -> Id<T> {
        let raw = u32::try_from(self.slots.len()).unwrap_or(u32::MAX);
        self.slots.push(Some(value));
        self.live += 1;
        Id::from_raw(raw)
    }

    pub fn remove(&mut self, id: Id<T>) -> Option<T> {
        let slot = self.slots.get_mut(id.index())?;
        let out = slot.take();
        if out.is_some() {
            self.live -= 1;
            while self.first < self.slots.len() && self.slots[self.first].is_none() {
                self.first += 1;
            }
        }
        out
    }

    /// Put a removed entity back in its own slot (undo support).
    pub fn restore(&mut self, id: Id<T>, value: T) {
        let idx = id.index();
        if idx >= self.slots.len() {
            self.slots.resize_with(idx + 1, || None);
        }
        if self.slots[idx].is_none() {
            self.live += 1;
        }
        self.slots[idx] = Some(value);
        if idx < self.first {
            self.first = idx;
        }
    }

    #[inline]
    pub fn contains(&self, id: Id<T>) -> bool {
        matches!(self.slots.get(id.index()), Some(Some(_)))
    }

    #[inline]
    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.slots.get(id.index()).and_then(|s| s.as_ref())
    }

    #[inline]
    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        self.slots.get_mut(id.index()).and_then(|s| s.as_mut())
    }

    pub fn clear(&mut self) {
        // Ids are never reused, even across a clear: keep the slot count.
        for s in self.slots.iter_mut() {
            *s = None;
        }
        self.live = 0;
        self.first = self.slots.len();
    }

    /// Live entries in creation order.
    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> + '_ {
        self.slots[self.first.min(self.slots.len())..]
            .iter()
            .enumerate()
            .filter_map(move |(i, s)| {
                s.as_ref()
                    .map(|v| (Id::from_raw((i + self.first) as u32), v))
            })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Id<T>, &mut T)> + '_ {
        let first = self.first.min(self.slots.len());
        self.slots[first..]
            .iter_mut()
            .enumerate()
            .filter_map(move |(i, s)| s.as_mut().map(|v| (Id::from_raw((i + first) as u32), v)))
    }

    /// Live ids in creation order.
    pub fn ids(&self) -> Vec<Id<T>> {
        self.iter().map(|(id, _)| id).collect()
    }
}

impl<T> std::ops::Index<Id<T>> for Arena<T> {
    type Output = T;
    #[inline]
    fn index(&self, id: Id<T>) -> &T {
        match self.get(id) {
            Some(v) => v,
            None => panic!("stale mesh id {:?}", id),
        }
    }
}

impl<T> std::ops::IndexMut<Id<T>> for Arena<T> {
    #[inline]
    fn index_mut(&mut self, id: Id<T>) -> &mut T {
        match self.get_mut(id) {
            Some(v) => v,
            None => panic!("stale mesh id {:?}", id),
        }
    }
}
