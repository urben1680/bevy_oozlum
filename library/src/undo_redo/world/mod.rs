use std::{
    hash::Hash,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, NoBundleEffect},
        component::ComponentId,
        entity::Entity,
        resource::Resource,
        world::{FromWorld, World},
    },
    log::warn,
};

use super::*;

pub struct RevWorld<'w> {
    pub(super) world: &'w mut World,
    pub(super) frame: u64,
}

impl<'w> TryFrom<&'w mut World> for RevWorld<'w> {
    type Error = RevMetaNotLogError;
    fn try_from(world: &'w mut World) -> Result<Self, Self::Error> {
        non_log_frame(world.get_resource()).map(|frame| Self { world, frame })
    }
}

impl Deref for RevWorld<'_> {
    type Target = World;
    fn deref(&self) -> &Self::Target {
        &self.world
    }
}

impl DerefMut for RevWorld<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.world
    }
}

impl<'w> RevWorld<'w> {
    pub fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevEntityError> {
        let bundle = components_to_bundle(self, components);
        self.buffer_bundle(entity, at, bundle)
    }

    pub fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevEntityError> {
        let marker = DisabledToDespawn::for_buffer(self.frame);
        buffer_components_cached(self, entity, key, components, marker)
    }

    pub fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevEntityError> {
        let marker = DisabledToDespawn::for_buffer(self.frame);
        buffer_bundle(self, entity, at, bundle, marker)
    }

    #[track_caller]
    fn try_insert_batch_inner<I, B>(
        &mut self,
        batch: I,
        insert_mode: InsertMode,
    ) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        let marker = DisabledToDespawn::for_buffer(self.frame);
        let mut invalid = Vec::new();
        let mut rev_despawned = Vec::new();
        let batch: Vec<_> = batch
            .into_iter()
            .filter(|&(entity, _)| {
                self
                    .get_entity(entity)
                    .map_err(|err| invalid.push(err))
                    .map(|entity| entity.location().archetype_id)
                    .is_ok_and(|archetype_id| pre_insert::<B>(self, entity, archetype_id, insert_mode, marker)
                        .map_err(|err| match err {
                            RevEntityError::EntityRevDespawnedError(err) => rev_despawned.push(err),
                            _ => unreachable!("EntityDoesNotExistError collected earlier, no other errors returned by pre_insert"),
                        })
                        .is_ok())
            })
            .collect();
        match insert_mode {
            InsertMode::Replace => self.insert_batch(batch),
            InsertMode::Keep => self.insert_batch_if_new(batch),
        }

        if invalid.is_empty() && rev_despawned.is_empty() {
            Ok(())
        } else {
            Err(RevEntitiesError {
                invalid,
                rev_despawned,
            })
        }
    }

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    #[track_caller]
    pub fn rev_despawn(&mut self, entity: Entity) -> bool {
        self.rev_try_despawn(entity)
            .inspect_err(|err| warn!("entity {entity} could not be reversibly despawned: {err}"))
            .is_ok()
    }

    /// Reversible version of [`World::get_entity_mut`].
    pub fn rev_get_entity_mut(
        &mut self,
        entity: Entity,
    ) -> Result<RevEntityWorldMut<EntityWorldMut>, RevEntityError> {
        let frame = self.frame;
        let entity_world_mut = match self.get_entity_mut(entity) {
            Ok(entity_world_mut) => entity_world_mut,
            Err(EntityMutableFetchError::EntityDoesNotExist(err)) => return Err(err.into()),
            Err(EntityMutableFetchError::AliasedMutability(_)) => {
                unreachable!("only one entity queued")
            }
        };
        if let Some(&marker) = entity_world_mut.get::<DisabledToDespawn>() {
            return Err(RevEntityError::EntityRevDespawnedError(
                EntityRevDespawnedError { entity, marker },
            ));
        }
        Ok(RevEntityWorldMut {
            entity_world_mut,
            frame,
            _marker: PhantomData
        })
    }

    /// Reversible version of [`World::entity_mut`].
    pub fn rev_entity_mut(&mut self, entity: Entity) -> RevEntityWorldMut<EntityWorldMut> {
        self.rev_get_entity_mut(entity)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::get_resource_or_init`].
    pub fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R> {
        self.rev_init_resource::<R>();
        self.resource_mut::<R>()
    }

    /// Reversible version of [`World::get_resource_or_insert_with`].
    pub fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
        self.get_resource_or_insert_with(func)
    }

    // rev_init_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::init_resource`].
    pub fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        if !self.contains_resource::<R>() {
            self.init_resource::<R>();
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
    }

    /// Reversible version of [`World::insert_batch`].
    #[track_caller]
    pub fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch(batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::insert_batch_if_new`].
    #[track_caller]
    pub fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch_if_new(batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    // rev_insert_non_send_by_id
    // out of scope due Send bound on UndoRedo

    // rev_insert_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::insert_resource`].
    pub fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        let swap = ResourceSwap(self.remove_resource::<R>());
        self.insert_resource(resource);
        self.buffer_undo_redo(swap);
    }

    // rev_insert_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_remove_non_send_by_id
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::remove_resource`].
    pub fn rev_remove_resource<R: Resource, Out>(
        &mut self,
        c: impl FnOnce(&R) -> Out,
    ) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo(ResourceSwap(Some(resource)));
            out
        })
    }

    /// rev_remove_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    /// Reversible version of [`World::spawn`].
    #[track_caller]
    pub fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> RevEntityWorldMut<EntityWorldMut> {
        let frame = self.frame;
        let entity_world_mut = self.spawn(bundle);
        RevEntityWorldMut {
            entity_world_mut,
            frame,
            _marker: PhantomData
        }
    }

    /// Reversible version of [`World::spawn_batch`].
    #[track_caller]
    pub fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        let marker = DisabledToDespawn::for_spawn_despawn(self.frame);
        let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
        self.buffer_undo_redo(Spawn::<Arc<[Entity]>> {
            spawned: entities.clone(),
            marker,
        });
        entities
    }

    /// Reversible version of [`World::spawn_empty`].
    #[track_caller]
    pub fn rev_spawn_empty(&mut self, despawn_at_undo: bool) -> RevEntityWorldMut<EntityWorldMut> {
        let frame = self.frame;
        let mut entity_world_mut = self.spawn_empty();
        if despawn_at_undo {
            let entity = entity_world_mut.id();
            let marker = DisabledToDespawn::for_spawn_despawn(frame);
            entity_world_mut.buffer_undo_redo(Spawn {
                spawned: [entity],
                marker,
            });
        }
        RevEntityWorldMut {
            entity_world_mut,
            frame,
            _marker: PhantomData
        }
    }

    /// Reversible version of [`World::try_despawn`].
    pub fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevEntityError> {
        // todo: reduce err check
        rev_despawn_inner(self.rev_get_entity_mut(entity)?)
    }

    /// Reversible version of [`World::try_insert_batch`].
    pub fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.try_insert_batch_inner(batch, InsertMode::Replace)
    }

    /// Reversible version of [`World::try_insert_batch_if_new`].
    pub fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), RevEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.try_insert_batch_inner(batch, InsertMode::Keep)
    }
}

#[cfg(test)]
mod test;

/*
pub trait RevWorld {
    // buffer methods

    fn buffer_components_in_progress(&self) -> Option<BufferInProgress>;

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevMetaOrEntityError>;

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevMetaOrEntityError>;

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevMetaOrEntityError>;

    // additional fallible methods

    fn rev_try_spawn_batch<I>(&mut self, iter: I) -> Result<Arc<[Entity]>, RevMetaNotLogError>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    #[track_caller]
    fn rev_despawn(&mut self, entity: Entity) -> bool {
        self.rev_try_despawn(entity)
            .inspect_err(|err| warn!("entity {entity} could not be reversibly despawned: {err}"))
            .is_ok()
    }

    /// Reversible version of [`World::get_resource_or_init`].
    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R>;

    // rev_init_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);

    /// Reversible version of [`World::insert_batch`].
    #[track_caller]
    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch(batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Reversible version of [`World::insert_batch_if_new`].
    #[track_caller]
    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_insert_batch_if_new(batch)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    // rev_insert_non_send_by_id
    // out of scope due Send bound on UndoRedo

    // rev_insert_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);

    // rev_insert_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_remove_non_send_by_id
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource, Out>(&mut self, c: impl FnOnce(&R) -> Out) -> Option<Out>;

    /// rev_remove_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_spawn
    // implemented via DespawnAtUndo BundleEffect

    /// Reversible version of [`World::spawn_batch`].
    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        self.rev_try_spawn_batch(iter)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    // rev_spawn_empty
    // implemented via DespawnAtUndo BundleEffect

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevMetaOrEntityError>;

    /// Reversible version of [`World::try_insert_batch`].
    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), RevMetaOrEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), RevMetaOrEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;
}

impl RevWorld for World {
    fn buffer_components_in_progress(&self) -> Option<BufferInProgress> {
        buffer_components_in_progress(self)
    }

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevMetaOrEntityError> {
        let bundle = components_to_bundle(self, components);
        self.buffer_bundle(entity, at, bundle)
    }

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevMetaOrEntityError> {
        let marker = DisabledToDespawn::for_buffer(self.get_resource::<RevMeta>())?;
        buffer_components_cached(self, entity, key, components, marker)
    }

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevMetaOrEntityError> {
        let marker = DisabledToDespawn::for_buffer(self.get_resource::<RevMeta>())?;
        buffer_bundle(self, entity, at, bundle, marker)
    }

    #[track_caller]
    fn rev_try_spawn_batch<I>(&mut self, iter: I) -> Result<Arc<[Entity]>, RevMetaNotLogError>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        let marker = DisabledToDespawn::for_spawn_despawn(self.get_resource::<RevMeta>())?;
        let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
        self.buffer_undo_redo(Spawn::<Arc<[Entity]>> {
            spawned: entities.clone(),
            marker,
        });
        Ok(entities)
    }

    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        if !self.contains_resource::<R>() {
            self.init_resource::<R>();
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
    }

    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        let swap = ResourceSwap(self.remove_resource::<R>());
        self.insert_resource(resource);
        self.buffer_undo_redo(swap);
    }

    fn rev_remove_resource<R: Resource, Out>(&mut self, c: impl FnOnce(&R) -> Out) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo(ResourceSwap(Some(resource)));
            out
        })
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevMetaOrEntityError> {
        rev_despawn_inner(self.get_entity_mut(entity).map_err(|err| match err {
            EntityMutableFetchError::EntityDoesNotExist(err) => err,
            EntityMutableFetchError::AliasedMutability(_) => unreachable!(),
        })?)
    }

    #[track_caller]
    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), RevMetaOrEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Replace)
    }

    #[track_caller]
    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), RevMetaOrEntitiesError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Keep)
    }

    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R> {
        self.rev_init_resource::<R>();
        self.resource_mut::<R>()
    }

    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
        self.get_resource_or_insert_with(func)
    }
}
*/
