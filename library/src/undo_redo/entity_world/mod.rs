use core::marker::PhantomData;

use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::Component,
    entity::Entity,
    relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
    world::EntityWorldMut,
};

use crate::{
    meta::MetaPastLen,
    undo_redo::{
        AddRemoveRelated, EntityRevDespawnedError, IsRevDespawned, RevBundle, RevDespawned,
        RevWorld, get_new_related, mark_entity,
    },
};

use super::BuffersUndoRedo;

#[cfg(test)]
mod test;

pub(super) trait RevEntityWorld {
    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_despawn_with_caller(
        self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<(), EntityRevDespawnedError>;

    fn rev_with_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_add_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_add_one_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_detach_all_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_remove_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_replace_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_despawn_related_with_caller<S: RelationshipTarget>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_insert_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_insert_if_new_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_remove_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn assert_not_rev_despawned(&mut self) -> Result<(), EntityRevDespawnedError>;
}

impl<'w> RevEntityWorld for EntityWorldMut<'w> {
    fn rev_insert_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        bundle.rev_insert(meta_past_len, self, InsertMode::Replace, caller);
        Ok(self)
    }

    fn rev_insert_if_new_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        bundle.rev_insert(meta_past_len, self, InsertMode::Keep, caller);
        Ok(self)
    }

    fn rev_remove_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        T::rev_remove(meta_past_len, self, caller);
        Ok(self)
    }

    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        mark_entity::<true>(meta_past_len, self, include_unlinked_related, caller);
        Ok(self)
    }

    fn rev_despawn_with_caller(
        mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<(), EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        mark_entity::<false>(meta_past_len, &mut self, false, caller);
        Ok(())
    }

    fn rev_with_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let new_related = get_new_related::<R>(self, |entity| entity.with_related::<R>(bundle));
        let id = self.id();
        self.world_scope(|world| {
            if world.rev_mark_spawned_with_caller(meta_past_len, new_related, true, caller) {
                world.buffer_undo_redo_with_caller(
                    meta_past_len,
                    AddRemoveRelated::<R, _, true>::new(id, [new_related], caller),
                    caller,
                );
            }
        });
        Ok(self)
    }

    fn rev_add_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let id = self.id();
        self.add_related::<R>(related.as_ref())
            .buffer_undo_redo_with_caller(
                meta_past_len,
                AddRemoveRelated::<R, _, true>::new(id, related, caller),
                caller,
            );
        Ok(self)
    }

    fn rev_add_one_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let id = self.id();
        self.add_one_related::<R>(entity)
            .buffer_undo_redo_with_caller(
                meta_past_len,
                AddRemoveRelated::<R, _, true>::new(id, [entity], caller),
                caller,
            );
        Ok(self)
    }

    fn rev_detach_all_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        self.get::<R::RelationshipTarget>()
            .map(|related| related.collection().iter().collect::<Vec<_>>())
            .map(|related| {
                let id = self.id();
                self.detach_all_related::<R>().buffer_undo_redo_with_caller(
                    meta_past_len,
                    AddRemoveRelated::<R, _, false>::new(id, related, caller),
                    caller,
                )
            });
        Ok(self)
    }

    fn rev_remove_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        if self.contains::<R::RelationshipTarget>() {
            let id = self.id();
            self.remove_related::<R>(related.as_ref())
                .buffer_undo_redo_with_caller(
                    meta_past_len,
                    AddRemoveRelated::<R, _, false>::new(id, related, caller),
                    caller,
                )
        }
        Ok(self)
    }

    fn rev_replace_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_detach_all_related_with_caller::<R>(meta_past_len, caller)?
            .rev_add_related_with_caller::<R>(meta_past_len, related, caller)
    }

    fn rev_despawn_related_with_caller<S: RelationshipTarget>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let Some(target) = self.get::<S>() else {
            return Ok(self);
        };
        let related: Vec<Entity> = target.collection().iter().collect();
        let id = self.id();
        self.world_scope(|world| {
            world.rev_despawn_batch_with_caller(meta_past_len, &*related, caller)
        });
        self.redo_and_buffer_with_caller(
            meta_past_len,
            AddRemoveRelated::<S::Relationship, _, false>::new(id, related, caller),
            caller,
        );
        Ok(self)
    }

    fn assert_not_rev_despawned(&mut self) -> Result<(), EntityRevDespawnedError> {
        if size_of::<MaybeLocation>() == 0 {
            if self.is_rev_despawned() {
                return Err(EntityRevDespawnedError {
                    entity: self.id(),
                    caller: MaybeLocation::caller(),
                });
            }
        } else if let Some(&marker) = self.get::<RevDespawned>() {
            return Err(EntityRevDespawnedError {
                entity: self.id(),
                caller: marker.0,
            });
        }
        Ok(())
    }
}

/// [`ComponentEntry`](bevy_ecs::world::ComponentEntry) variant with additional reversible methods.
pub enum RevComponentEntry<'w, 'a, T: Component> {
    /// An occupied entry.
    Occupied(RevOccupiedComponentEntry<'w, 'a, T>),
    /// A vacant entry.
    Vacant(RevVacantComponentEntry<'w, 'a, T>),
}

/// [`ComponentEntry`](bevy_ecs::world::OccupiedComponentEntry) variant with additional reversible
/// methods.
pub struct RevOccupiedComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

/// [`ComponentEntry`](bevy_ecs::world::VacantComponentEntry) variant with additional reversible
/// methods.
pub struct RevVacantComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

impl<'w, 'a, T: Component> RevComponentEntry<'w, 'a, T> {
    /// See [`ComponentEntry::target_entity`](bevy_ecs::world::ComponentEntry::insert_entry).
    #[track_caller]
    pub fn insert_entry(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.insert(component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.insert(component),
        }
    }

    /// Reversible version of
    /// [`ComponentEntry::target_entity`](bevy_ecs::world::ComponentEntry::insert_entry).
    #[track_caller]
    pub fn rev_insert_entry(
        self,
        meta_past_len: MetaPastLen,
        component: T,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.rev_insert(meta_past_len, component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.rev_insert(meta_past_len, component),
        }
    }

    /// See [`ComponentEntry::or_insert`](bevy_ecs::world::ComponentEntry::or_insert).
    #[track_caller]
    pub fn or_insert(self, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Reversible version of [`ComponentEntry::or_insert`](bevy_ecs::world::ComponentEntry::or_insert).
    #[track_caller]
    pub fn rev_or_insert(
        self,
        meta_past_len: MetaPastLen,
        default: T,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(meta_past_len, default),
        }
    }

    /// See [`ComponentEntry::or_insert_with`](bevy_ecs::world::ComponentEntry::or_insert_with).
    #[track_caller]
    pub fn or_insert_with<F: FnOnce() -> T>(
        self,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Reversible version of
    /// [`ComponentEntry::or_insert_with`](bevy_ecs::world::ComponentEntry::or_insert_with).
    #[track_caller]
    pub fn rev_or_insert_with<F: FnOnce() -> T>(
        self,
        meta_past_len: MetaPastLen,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(meta_past_len, default()),
        }
    }
}

impl<'w, 'a, T: Component + Default> RevComponentEntry<'w, 'a, T> {
    /// See [`ComponentEntry::or_insert_with`](bevy_ecs::world::ComponentEntry::or_default).
    #[track_caller]
    pub fn or_default(self) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(Default::default()),
        }
    }

    /// Reversible version of
    /// [`ComponentEntry::or_insert_with`](bevy_ecs::world::ComponentEntry::or_default).
    #[track_caller]
    pub fn rev_or_default(
        self,
        meta_past_len: MetaPastLen,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(meta_past_len, Default::default()),
        }
    }
}

impl<'w, 'a, T: Component> RevOccupiedComponentEntry<'w, 'a, T> {
    /// See
    /// [`OccupiedComponentEntry::or_insert_with`](bevy_ecs::world::OccupiedComponentEntry::insert).
    #[track_caller]
    pub fn insert(&mut self, component: T) {
        self.entity_world_mut.insert(component);
    }

    /// Reversible version of
    /// [`OccupiedComponentEntry::or_insert_with`](bevy_ecs::world::OccupiedComponentEntry::insert).
    #[track_caller]
    pub fn rev_insert(&mut self, meta_past_len: MetaPastLen, component: T) {
        self.entity_world_mut
            .rev_insert_with_caller(meta_past_len, component, MaybeLocation::caller())
            .unwrap();
    }

    /// See [`OccupiedComponentEntry::take`](bevy_ecs::world::OccupiedComponentEntry::take).
    #[track_caller]
    pub fn take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.take().unwrap()
    }
}

impl<'w, 'a, T: Component> RevVacantComponentEntry<'w, 'a, T> {
    /// See [`VacantComponentEntry::insert`](bevy_ecs::world::VacantComponentEntry::insert).
    #[track_caller]
    pub fn insert(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.insert(component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }

    /// Reversible version of
    /// [`VacantComponentEntry::insert`](bevy_ecs::world::VacantComponentEntry::insert).
    #[track_caller]
    pub fn rev_insert(
        self,
        meta_past_len: MetaPastLen,
        component: T,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut
            .rev_insert_with_caller(meta_past_len, component, MaybeLocation::caller())
            .unwrap();
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }
}
