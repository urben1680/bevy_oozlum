use super::{BuffersUndoRedo, UndoRedo};
use crate::{
    meta::{MetaPastLen, RevDirection, RevMeta},
    undo_redo::{
        RevEntityWorldMut, RevInsertResourceNew, RevInsertResourceOverwrite, RevRemoveResource,
        mark_entities, mark_entity,
    },
};
use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    change_detection::MaybeLocation,
    component::ComponentId,
    entity::{Entity, SpawnError},
    resource::Resource,
    world::{EntityWorldMut, FromWorld, Mut, World},
};

#[cfg(test)]
mod test;

pub trait RevWorld {
    #[track_caller]
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

    fn get_running_direction(&self) -> Option<RevDirection>;

    #[track_caller]
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
    ) -> bool {
        self.rev_mark_spawned_with_caller(
            meta_past_len,
            entity,
            include_unlinked_related,
            MaybeLocation::caller(),
        )
    }

    #[doc(hidden)]
    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> bool;

    /// Helper method to mark a spawned batch as reversibly spawned.
    ///
    /// When possible, use [`World::rev_spawn_batch`] instead.
    #[track_caller]
    fn rev_mark_spawned_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
    ) {
        self.rev_mark_spawned_batch_with_caller(
            meta_past_len,
            entities,
            include_unlinked_related,
            MaybeLocation::caller(),
        );
    }

    #[doc(hidden)]
    fn rev_mark_spawned_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
        caller: MaybeLocation,
    );

    #[track_caller]
    fn rev_mark_despawned(&mut self, meta_past_len: MetaPastLen, entity: Entity) -> bool {
        self.rev_mark_despawned_with_caller(meta_past_len, entity, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_mark_despawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> bool;

    /// Helper method to mark a spawned batch as reversibly despawned.
    #[track_caller]
    fn rev_mark_despawned_batch(&mut self, meta_past_len: MetaPastLen, entities: &[Entity]) {
        self.rev_mark_despawned_batch_with_caller(meta_past_len, entities, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_mark_despawned_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        caller: MaybeLocation,
    );

    /// Reversible version of [`World::spawn`].
    #[track_caller]
    fn rev_spawn(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> EntityWorldMut<'_> {
        self.rev_spawn_with_caller(meta_past_len, bundle, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_spawn_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_>;

    /// Reversible version of [`World::spawn_at`].
    #[track_caller]
    fn rev_spawn_at(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        self.rev_spawn_at_with_caller(meta_past_len, entity, bundle, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_spawn_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError>;

    /// Reversible version of [`World::spawn_empty`].
    #[track_caller]
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityWorldMut<'_> {
        self.rev_spawn_empty_with_caller(meta_past_len, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_spawn_empty_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_>;

    /// Reversible version of [`World::spawn_empty_at`].
    #[track_caller]
    fn rev_spawn_empty_at(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        self.rev_spawn_empty_at_with_caller(meta_past_len, entity, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_spawn_empty_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError>;

    /// Reversible version of [`World::spawn_batch`].
    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, meta_past_len: MetaPastLen, iter: I) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>,
    {
        self.rev_spawn_batch_with_caller(meta_past_len, iter, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_spawn_batch_with_caller<I>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>;

    /// Reversible version of [`World::get_resource_or_init`].
    #[track_caller]
    fn rev_get_resource_or_init<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> Mut<'_, R> {
        self.rev_get_resource_or_init_with_caller(meta_past_len, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_get_resource_or_init_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    #[track_caller]
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        self.rev_get_resource_or_insert_with_with_caller(
            meta_past_len,
            func,
            MaybeLocation::caller(),
        )
    }

    #[doc(hidden)]
    fn rev_get_resource_or_insert_with_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
        caller: MaybeLocation,
    ) -> Mut<'_, R>;

    /// Reversible version of [`World::init_resource`].
    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> ComponentId {
        self.rev_init_resource_with_caller::<R>(meta_past_len, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_init_resource_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> ComponentId;

    /// Reversible version of [`World::insert_resource`].
    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen, resource: R) {
        self.rev_insert_resource_with_caller(meta_past_len, resource, MaybeLocation::caller());
    }

    #[doc(hidden)]
    fn rev_insert_resource_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        resource: R,
        caller: MaybeLocation,
    );

    /// Reversible version of [`World::remove_resource`].
    #[track_caller]
    fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out> {
        self.rev_remove_resource_with_caller(meta_past_len, c, MaybeLocation::caller())
    }

    #[doc(hidden)]
    fn rev_remove_resource_with_caller<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
        caller: MaybeLocation,
    ) -> Option<Out>;
}

impl RevWorld for World {
    fn redo_and_buffer_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        mut undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        undo_redo.redo(self);
        self.buffer_undo_redo_with_caller(meta_past_len, undo_redo, caller)
    }

    fn get_running_direction(&self) -> Option<RevDirection> {
        self.get_resource::<RevMeta>()?.get_running_direction()
    }

    fn rev_mark_spawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) -> bool {
        let Ok(mut entity) = self.get_entity_mut(entity) else {
            return false;
        };
        mark_entity::<true>(meta_past_len, &mut entity, include_unlinked_related, caller)
    }

    fn rev_mark_spawned_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        include_unlinked_related: bool,
        caller: MaybeLocation,
    ) {
        mark_entities::<true>(
            meta_past_len,
            self,
            entities,
            include_unlinked_related,
            caller,
        );
    }

    fn rev_mark_despawned_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> bool {
        let Ok(mut entity) = self.get_entity_mut(entity) else {
            return false;
        };
        mark_entity::<false>(meta_past_len, &mut entity, false, caller)
    }

    fn rev_mark_despawned_batch_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: &[Entity],
        caller: MaybeLocation,
    ) {
        mark_entities::<false>(meta_past_len, self, entities, false, caller);
    }

    fn rev_get_resource_or_init_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> Mut<'_, R> {
        self.rev_init_resource_with_caller::<R>(meta_past_len, caller);
        self.resource_mut::<R>()
    }

    fn rev_get_resource_or_insert_with_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        func: impl FnOnce() -> R,
        caller: MaybeLocation,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceNew::<R>::new(caller),
                caller,
            );
        }
        self.get_resource_or_insert_with(func)
    }

    fn rev_init_resource_with_caller<R: Resource + FromWorld>(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> ComponentId {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceNew::<R>::new(caller),
                caller,
            );
        }
        self.init_resource::<R>()
    }

    fn rev_insert_resource_with_caller<R: Resource>(
        &mut self,
        meta_past_len: MetaPastLen,
        resource: R,
        caller: MaybeLocation,
    ) {
        match self.remove_resource::<R>() {
            Some(resource) => self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceOverwrite::new(resource, caller),
                caller,
            ),
            None => self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevInsertResourceNew::<R>::new(caller),
                caller,
            ),
        }
        self.insert_resource(resource);
    }

    fn rev_remove_resource_with_caller<R: Resource, Out>(
        &mut self,
        meta_past_len: MetaPastLen,
        c: impl FnOnce(&R) -> Out,
        caller: MaybeLocation,
    ) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo_with_caller(
                meta_past_len,
                RevRemoveResource::new(resource, caller),
                caller,
            );
            out
        })
    }

    fn rev_spawn_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_> {
        let mut entity = self.spawn(bundle);
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        entity
    }

    fn rev_spawn_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        bundle: impl Bundle,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        let mut entity = self.spawn_at(entity, bundle)?;
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        Ok(entity)
    }

    fn rev_spawn_empty_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        caller: MaybeLocation,
    ) -> EntityWorldMut<'_> {
        let mut entity = self.spawn_empty();
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        entity
    }

    fn rev_spawn_empty_at_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        caller: MaybeLocation,
    ) -> Result<EntityWorldMut<'_>, SpawnError> {
        let mut entity = self.spawn_empty_at(entity)?;
        entity
            .rev_mark_spawned_with_caller(meta_past_len, true, caller)
            .unwrap();
        Ok(entity)
    }

    fn rev_spawn_batch_with_caller<I>(
        &mut self,
        meta_past_len: MetaPastLen,
        iter: I,
        caller: MaybeLocation,
    ) -> Vec<Entity>
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>>,
    {
        let entities = self.spawn_batch(iter).collect::<Vec<_>>();
        mark_entities::<true>(meta_past_len, self, &*entities, true, caller);
        entities
    }
}
