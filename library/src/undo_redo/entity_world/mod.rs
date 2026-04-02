use alloc::vec::Vec;
use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    entity::Entity,
    relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
    world::EntityWorldMut,
};

use crate::{
    meta::NotLog,
    undo_redo::{
        AddRemoveRelated, EntityRevDespawnedError, IsRevDespawned, RevBundle, RevDespawned,
        RevWorld, UndoRedo, get_new_related, mark_entity,
    },
};

#[cfg(test)]
mod test;

pub(super) trait RevEntityWorld {
    fn queue_undo_redo(&mut self, not_log: NotLog, undo_redo: impl UndoRedo, caller: MaybeLocation);

    fn redo_and_queue(&mut self, not_log: NotLog, undo_redo: impl UndoRedo, caller: MaybeLocation);

    fn rev_mark_spawned(
        &mut self,
        not_log: NotLog,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_despawn(
        self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<(), EntityRevDespawnedError>;

    fn rev_with_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_add_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_add_one_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_detach_all_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_remove_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_replace_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_despawn_related<S: RelationshipTarget>(
        &mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_insert<T: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_insert_if_new<T: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn rev_remove<T: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError>;

    fn assert_not_rev_despawned(&mut self) -> Result<(), EntityRevDespawnedError>;
}

impl<'w> RevEntityWorld for EntityWorldMut<'w> {
    fn queue_undo_redo(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        self.world_scope(|world| {
            world.queue_undo_redo(not_log, undo_redo, caller);
        })
    }

    fn redo_and_queue(&mut self, not_log: NotLog, undo_redo: impl UndoRedo, caller: MaybeLocation) {
        self.world_scope(|world| {
            world.redo_and_queue(not_log, undo_redo, caller);
        })
    }

    fn rev_insert<T: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        bundle.rev_insert(not_log, self, InsertMode::Replace, caller);
        Ok(self)
    }

    fn rev_insert_if_new<T: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        bundle: T,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        bundle.rev_insert(not_log, self, InsertMode::Keep, caller);
        Ok(self)
    }

    fn rev_remove<T: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        T::rev_remove(not_log, self, caller);
        Ok(self)
    }

    fn rev_mark_spawned(
        &mut self,
        not_log: NotLog,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        mark_entity::<true>(not_log, self, include_unlinked_related, caller);
        Ok(self)
    }

    fn rev_despawn(
        mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<(), EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        mark_entity::<false>(not_log, &mut self, false, caller);
        Ok(())
    }

    fn rev_with_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let Some(new_related) =
            get_new_related::<R>(self, |entity| entity.with_related::<R>(bundle))
        else {
            return Ok(self);
        };
        let id = self.id();
        self.world_scope(|world| {
            if world.rev_mark_spawned(not_log, new_related, true, caller) {
                world.queue_undo_redo(
                    not_log,
                    AddRemoveRelated::<R, _, true>::new(id, [new_related], caller),
                    caller,
                );
            }
        });
        Ok(self)
    }

    fn rev_add_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let id = self.id();
        self.add_related::<R>(related.as_ref()).queue_undo_redo(
            not_log,
            AddRemoveRelated::<R, _, true>::new(id, related, caller),
            caller,
        );
        Ok(self)
    }

    fn rev_add_one_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let id = self.id();
        self.add_one_related::<R>(entity).queue_undo_redo(
            not_log,
            AddRemoveRelated::<R, _, true>::new(id, [entity], caller),
            caller,
        );
        Ok(self)
    }

    fn rev_detach_all_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let related = self
            .get::<R::RelationshipTarget>()
            .map(|related| related.collection().iter().collect::<Vec<_>>());
        if let Some(related) = related {
            let id = self.id();
            self.detach_all_related::<R>().queue_undo_redo(
                not_log,
                AddRemoveRelated::<R, _, false>::new(id, related, caller),
                caller,
            )
        }
        Ok(self)
    }

    fn rev_remove_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        if self.contains::<R::RelationshipTarget>() {
            let id = self.id();
            self.remove_related::<R>(related.as_ref()).queue_undo_redo(
                not_log,
                AddRemoveRelated::<R, _, false>::new(id, related, caller),
                caller,
            )
        }
        Ok(self)
    }

    fn rev_replace_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.rev_detach_all_related::<R>(not_log, caller)?
            .rev_add_related::<R>(not_log, related, caller)
    }

    fn rev_despawn_related<S: RelationshipTarget>(
        &mut self,
        not_log: NotLog,
        caller: MaybeLocation,
    ) -> Result<&mut Self, EntityRevDespawnedError> {
        self.assert_not_rev_despawned()?;
        let Some(target) = self.get::<S>() else {
            return Ok(self);
        };
        let related: Vec<Entity> = target.collection().iter().collect();
        let id = self.id();
        self.world_scope(|world| world.rev_despawn_batch(not_log, &related, caller));
        self.redo_and_queue(
            not_log,
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
