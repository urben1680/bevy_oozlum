use super::{BuffersUndoRedo, RevWorld, UndoRedo};
use crate::{
    meta::{MetaPastLen, RevDirection, RevMeta},
    undo_redo::{
        AddRemoveRelated, EntityRevDespawnedError, IsRevDespawned, RevBundle, RevDespawned,
        get_new_related, get_new_related_entities, mark_entity,
    },
};
use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::Component,
    entity::Entity,
    hierarchy::{ChildOf, Children},
    relationship::{
        RelatedSpawner, Relationship, RelationshipSourceCollection, RelationshipTarget,
    },
    world::EntityWorldMut,
};
use core::marker::PhantomData;

#[cfg(test)]
mod test;

/// Extension trait for [`RevEntityWorldMut`] with reversible variants of verious methods.
pub trait RevEntityWorldMut<'w> {
    /// Shorthand method of [`EntityWorldMut::buffer_undo_redo`] with applying
    /// `undo_redo.redo(&mut self)` immediately. Useful when there is no difference between doing
    /// and redoing.
    fn redo_and_buffer(&mut self, meta_past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.redo_and_buffer_with_caller(meta_past_len, undo_redo, MaybeLocation::caller());
    }

    #[doc(hidden)]
    fn redo_and_buffer_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    );

    /// Shorthand method of [`RevMeta::get_running_direction`].
    fn get_running_direction(&self) -> Option<RevDirection>;

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`World::rev_spawn`](super::World::rev_spawn).
    ///
    /// When possible, use `World::rev_spawn` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        include_unlinked_related: bool,
    ) -> &mut Self {
        self.rev_mark_spawned_with_caller(
            meta_past_len,
            include_unlinked_related,
            MaybeLocation::caller(),
        )
        .unwrap()
    }

    #[doc(hidden)]
    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// See the [`RevDespawned`] documentation to understand the mechanics of reversible
    /// spawn/despawn.
    fn rev_despawn(self, meta_past_len: MetaPastLen);

    #[doc(hidden)]
    fn rev_despawn_with_caller(
        self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<(), EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::with_related_entities`].
    #[track_caller]
    fn rev_with_related_entities<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce(&mut RelatedSpawner<'_, R>),
    ) -> &mut Self {
        self.rev_with_related_entities_with_caller(meta_past_len, func, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_with_related_entities_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce(&mut RelatedSpawner<'_, R>),
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::with_children`].
    #[track_caller]
    fn rev_with_children(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce(&mut RelatedSpawner<'_, ChildOf>),
    ) -> &mut Self {
        self.rev_with_children_with_caller(meta_past_len, func, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_with_children_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce(&mut RelatedSpawner<'_, ChildOf>),
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_with_related_entities_with_caller(meta_past_len, func, caller)
    }

    /// Reversible version of [`EntityWorldMut::with_related`].
    #[track_caller]
    fn rev_with_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut Self {
        self.rev_with_related_with_caller::<R>(meta_past_len, bundle, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_with_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::with_child`].
    #[track_caller]
    fn rev_with_child(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> &mut Self {
        self.rev_with_child_with_caller(meta_past_len, bundle, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_with_child_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_with_related_with_caller::<ChildOf>(meta_past_len, bundle, caller)
    }

    /// Reversible version of [`EntityWorldMut::add_related`].
    #[track_caller]
    fn rev_add_related<R: Relationship, E: AsRef<[Entity]> + Send + 'static>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: E,
    ) -> &mut Self {
        self.rev_add_related_with_caller::<R, E>(meta_past_len, related, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_add_related_with_caller<R: Relationship, E: AsRef<[Entity]> + Send + 'static>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: E,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::add_children`].
    #[track_caller]
    fn rev_add_children<E: AsRef<[Entity]> + Send + 'static>(
        &mut self,
        meta_past_len: MetaPastLen,
        children: E,
    ) -> &mut Self {
        self.rev_add_children_with_caller(meta_past_len, children, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_add_children_with_caller<E: AsRef<[Entity]> + Send + 'static>(
        &mut self,
        meta_past_len: MetaPastLen,
        children: E,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_add_related_with_caller::<ChildOf, _>(meta_past_len, children, caller)
    }

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    #[track_caller]
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
    ) -> &mut Self {
        self.rev_add_one_related_with_caller::<R>(meta_past_len, entity, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_add_one_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::add_child`].
    #[track_caller]
    fn rev_add_child(&mut self, meta_past_len: MetaPastLen, child: Entity) -> &mut Self {
        self.rev_add_child_with_caller(meta_past_len, child, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_add_child_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        child: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_add_one_related_with_caller::<ChildOf>(meta_past_len, child, caller)
    }

    /// Reversible version of [`EntityWorldMut::detach_all_related`].
    #[track_caller]
    fn rev_detach_all_related<R: Relationship>(&mut self, meta_past_len: MetaPastLen) -> &mut Self {
        self.rev_detach_all_related_with_caller::<R>(meta_past_len, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_detach_all_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::detach_all_children`].
    #[track_caller]
    fn rev_detach_all_children(&mut self, meta_past_len: MetaPastLen) -> &mut Self {
        self.rev_detach_all_children_with_caller(meta_past_len, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_detach_all_children_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_detach_all_related_with_caller::<ChildOf>(meta_past_len, caller)
    }

    /// Reversible version of [`EntityWorldMut::remove_related`].
    #[track_caller]
    fn rev_remove_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: &[Entity],
    ) -> &mut Self {
        self.rev_remove_related_with_caller::<R>(meta_past_len, related, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_remove_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: &[Entity],
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::detach_children`].
    #[track_caller]
    fn rev_detach_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: &[Entity],
    ) -> &mut Self {
        self.rev_detach_children_with_caller(meta_past_len, children, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_detach_children_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        children: &[Entity],
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_remove_related_with_caller::<ChildOf>(meta_past_len, children, caller)
    }

    /// Reversible version of [`EntityWorldMut::detach_child`].
    #[track_caller]
    fn rev_detach_child(&mut self, meta_past_len: MetaPastLen, child: Entity) -> &mut Self {
        self.rev_detach_child_with_caller(meta_past_len, child, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_detach_child_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        child: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_remove_related_with_caller::<ChildOf>(meta_past_len, &[child], caller)
    }

    /// Reversible version of [`EntityWorldMut::replace_related`].
    #[track_caller]
    fn rev_replace_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: &[Entity],
    ) -> &mut Self {
        self.rev_replace_related_with_caller::<R>(meta_past_len, related, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_replace_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: &[Entity],
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::replace_children`].
    #[track_caller]
    fn rev_replace_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: &[Entity],
    ) -> &mut Self {
        self.rev_replace_children_witch_caller(meta_past_len, children, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_replace_children_witch_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        children: &[Entity],
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_replace_related_with_caller::<ChildOf>(meta_past_len, children, caller)
    }

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    ///
    /// See the [`RevDespawned`] documentation to understand the mechanics of reversible
    /// spawn/despawn.
    #[track_caller]
    fn rev_despawn_related<S: RelationshipTarget>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self {
        self.rev_despawn_related_with_caller::<S>(meta_past_len, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_despawn_related_with_caller<S: RelationshipTarget>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::despawn_children`].
    ///
    /// See the [`RevDespawned`] documentation to understand the mechanics of reversible
    /// spawn/despawn.
    #[track_caller]
    fn rev_despawn_children(&mut self, meta_past_len: MetaPastLen) -> &mut Self {
        self.rev_despawn_children_with_caller(meta_past_len, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_despawn_children_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_despawn_related_with_caller::<Children>(meta_past_len, caller)
    }

    /// Reversible version of [`EntityWorldMut::entry`].
    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T>;

    /// Reversible version of [`EntityWorldMut::insert`].
    #[track_caller]
    fn rev_insert<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
    ) -> &mut Self {
        self.rev_insert_with_caller(meta_past_len, bundle, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_insert_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    #[track_caller]
    fn rev_insert_if_new<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
    ) -> &mut Self {
        self.rev_insert_if_new_with_caller(meta_past_len, bundle, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_insert_if_new_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    /// Reversible version of [`EntityWorldMut::remove`]. Let the second generic be inferred as `_`.
    #[track_caller]
    fn rev_remove<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self {
        self.rev_remove_with_caller::<T, _>(meta_past_len, MaybeLocation::caller())
            .unwrap()
    }

    #[doc(hidden)]
    fn rev_remove_with_caller<T: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    #[doc(hidden)]
    fn assert_not_rev_despawned(&mut self) -> Result<(), EntityRevDespawnedError>;
}

impl<'w> RevEntityWorldMut<'w> for EntityWorldMut<'w> {
    fn redo_and_buffer_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        self.world_scope(|world| {
            world.redo_and_buffer_with_caller(meta_past_len, undo_redo, caller)
        })
    }

    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T> {
        if self.contains::<T>() {
            RevComponentEntry::Occupied(RevOccupiedComponentEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        } else {
            RevComponentEntry::Vacant(RevVacantComponentEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        }
    }

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

    fn get_running_direction(&self) -> Option<RevDirection> {
        self.get_resource::<RevMeta>()
            .and_then(RevMeta::get_running_direction)
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

    fn rev_with_related_entities_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce(&mut RelatedSpawner<'_, R>),
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let new_related =
            get_new_related_entities::<R>(self, |entity| entity.with_related_entities(func));
        self.world_scope(|world| {
            world.rev_mark_spawned_batch_with_caller(meta_past_len, &*new_related, true, caller)
        });
        let id = self.id();
        self.buffer_undo_redo_with_caller(
            meta_past_len,
            AddRemoveRelated::<R, _, true>::new(id, new_related, caller),
            caller,
        );
        Ok(self)
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

    fn rev_add_related_with_caller<R: Relationship, E: AsRef<[Entity]> + Send + 'static>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: E,
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
        related: &[Entity],
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        if self.contains::<R::RelationshipTarget>() {
            let id = self.id();
            self.remove_related::<R>(related)
                .buffer_undo_redo_with_caller(
                    meta_past_len,
                    AddRemoveRelated::<R, _, false>::new(
                        id,
                        Box::<[Entity]>::from(related),
                        caller,
                    ),
                    caller,
                )
        }
        Ok(self)
    }

    fn rev_replace_related_with_caller<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: &[Entity],
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_detach_all_related_with_caller::<R>(meta_past_len, caller)?
            .rev_add_related_with_caller::<R, _>(
                meta_past_len,
                Box::<[Entity]>::from(related),
                caller,
            )
    }

    #[track_caller]
    fn rev_despawn(self, meta_past_len: MetaPastLen) {
        self.rev_despawn_with_caller(meta_past_len, MaybeLocation::caller())
            .unwrap();
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
        let id = self.id();
        if size_of::<MaybeLocation>() == 0 {
            if self.is_rev_despawned() {
                return Err(EntityRevDespawnedError {
                    entity: id,
                    caller: MaybeLocation::caller(),
                });
            }
        } else if let Some(marker) = self.get::<RevDespawned>() {
            return Err(EntityRevDespawnedError {
                entity: id,
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
        self.entity_world_mut.rev_insert(meta_past_len, component);
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
        self.entity_world_mut.rev_insert(meta_past_len, component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }
}
