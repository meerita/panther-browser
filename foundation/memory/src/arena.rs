// @file foundation/memory/src/arena.rs
// @description Defines the safe generational index arena.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// A generational identifier into an [`Arena`].
///
/// The identifier pairs a slot index with a generation. It is a handle, not a
/// pointer. When a slot is freed and later reused, the slot generation advances,
/// so an identifier from the previous occupant no longer resolves. This rejects
/// a stale identifier that targets a reused slot, aligned with the
/// `purr-graphics` generation model.
///
/// Only an [`Arena`] constructs an identifier, so an identifier always names a
/// slot the arena issued.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArenaId {
    index: usize,
    generation: u64,
}

impl ArenaId {
    /// The slot index this identifier names.
    pub fn index(self) -> usize {
        self.index
    }

    /// The generation this identifier was issued with.
    pub fn generation(self) -> u64 {
        self.generation
    }
}

enum Slot<T> {
    Occupied {
        generation: u64,
        value: T,
    },
    Free {
        generation: u64,
        next_free: Option<usize>,
    },
}

/// A generational index arena backed by a `Vec`.
///
/// The arena stores values in contiguous slots and hands out generational
/// identifiers. A freed slot joins a free list and is reused on the next insert,
/// with its generation advanced so prior identifiers to it stop resolving. The
/// arena uses only safe Rust and adds no external dependency.
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: Option<usize>,
    len: usize,
}

impl<T> Arena<T> {
    /// Creates an empty arena.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
        }
    }

    /// Creates an empty arena with room for `capacity` values before it grows.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            free_head: None,
            len: 0,
        }
    }

    /// The number of live values.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the arena holds no live value.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Inserts a value and returns its identifier.
    ///
    /// Reuses a freed slot when one exists, otherwise appends a new slot.
    pub fn insert(&mut self, value: T) -> ArenaId {
        self.len += 1;

        let Some(index) = self.free_head else {
            let index = self.slots.len();
            self.slots.push(Slot::Occupied {
                generation: 0,
                value,
            });
            return ArenaId {
                index,
                generation: 0,
            };
        };

        let Slot::Free {
            generation,
            next_free,
        } = self.slots[index]
        else {
            unreachable!("the free list only links free slots");
        };
        self.free_head = next_free;
        self.slots[index] = Slot::Occupied { generation, value };
        ArenaId { index, generation }
    }

    /// Borrows the value the identifier names, or `None` if the identifier is
    /// stale or out of range.
    pub fn get(&self, id: ArenaId) -> Option<&T> {
        match self.slots.get(id.index)? {
            Slot::Occupied { generation, value } if *generation == id.generation => Some(value),
            _ => None,
        }
    }

    /// Mutably borrows the value the identifier names, or `None` if the
    /// identifier is stale or out of range.
    pub fn get_mut(&mut self, id: ArenaId) -> Option<&mut T> {
        match self.slots.get_mut(id.index)? {
            Slot::Occupied { generation, value } if *generation == id.generation => Some(value),
            _ => None,
        }
    }

    /// Removes and returns the value the identifier names, or `None` if the
    /// identifier is stale or out of range.
    ///
    /// The freed slot advances its generation, so the removed identifier and any
    /// copy of it stop resolving.
    pub fn remove(&mut self, id: ArenaId) -> Option<T> {
        match self.slots.get(id.index)? {
            Slot::Occupied { generation, .. } if *generation == id.generation => {}
            _ => return None,
        }

        let next_generation = id.generation.wrapping_add(1);
        let freed = std::mem::replace(
            &mut self.slots[id.index],
            Slot::Free {
                generation: next_generation,
                next_free: self.free_head,
            },
        );
        self.free_head = Some(id.index);
        self.len -= 1;

        match freed {
            Slot::Occupied { value, .. } => Some(value),
            Slot::Free { .. } => unreachable!("the slot was verified occupied above"),
        }
    }

    /// Removes every value and invalidates every prior identifier.
    ///
    /// Each occupied slot advances its generation as it is freed, so an
    /// identifier issued before the call stops resolving.
    pub fn clear(&mut self) {
        let mut next_free = None;
        for index in (0..self.slots.len()).rev() {
            let generation = match &self.slots[index] {
                Slot::Occupied { generation, .. } => generation.wrapping_add(1),
                Slot::Free { generation, .. } => *generation,
            };
            self.slots[index] = Slot::Free {
                generation,
                next_free,
            };
            next_free = Some(index);
        }
        self.free_head = next_free;
        self.len = 0;
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Arena;

    #[test]
    fn insert_then_get_returns_the_value() {
        let mut arena = Arena::new();
        let id = arena.insert(42);
        assert_eq!(arena.get(id), Some(&42));
        assert_eq!(arena.len(), 1);
        assert!(!arena.is_empty());
    }

    #[test]
    fn get_mut_edits_in_place() {
        let mut arena = Arena::new();
        let id = arena.insert(1);
        *arena.get_mut(id).unwrap() = 2;
        assert_eq!(arena.get(id), Some(&2));
    }

    #[test]
    fn remove_returns_the_value_and_empties_the_slot() {
        let mut arena = Arena::new();
        let id = arena.insert("value");
        assert_eq!(arena.remove(id), Some("value"));
        assert_eq!(arena.get(id), None);
        assert!(arena.is_empty());
        assert_eq!(arena.remove(id), None);
    }

    #[test]
    fn stale_id_from_a_reused_slot_returns_none() {
        let mut arena = Arena::new();
        let first = arena.insert(10);
        assert_eq!(arena.remove(first), Some(10));

        let second = arena.insert(20);
        assert_eq!(second.index(), first.index());
        assert_ne!(second.generation(), first.generation());

        assert_eq!(arena.get(first), None);
        assert_eq!(arena.get(second), Some(&20));
    }

    #[test]
    fn clear_empties_the_arena_and_invalidates_prior_ids() {
        let mut arena = Arena::new();
        let a = arena.insert(1);
        let b = arena.insert(2);
        arena.clear();

        assert!(arena.is_empty());
        assert_eq!(arena.get(a), None);
        assert_eq!(arena.get(b), None);

        let c = arena.insert(3);
        assert_eq!(arena.get(c), Some(&3));
        assert_eq!(arena.get(a), None);
    }
}
