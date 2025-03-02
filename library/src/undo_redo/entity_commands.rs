use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, InsertMode},
        component::{Component, ComponentId},
        entity::{Entity, EntityClonerBuilder},
        system::{entity_command::insert_by_id, EntityCommand},
        world::{FromWorld, OccupiedEntry},
    },
    ptr::OwningPtr,
};

use super::*;

pub trait RevEntityWorldMut {
    /// Reversible version of [`modify_component`](EntityWorldMut::insert).
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::insert_if_new).
    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::insert_by_id).
    ///
    /// # Safety
    ///
    /// - [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::insert_by_ids).
    ///
    /// # Safety
    ///
    /// - Each [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - Each [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::remove).
    fn rev_remove<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::remove_with_requires).
    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::retain).
    fn rev_retain<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::remove_by_id).
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::remove_by_ids).
    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::clear).
    fn rev_clear(&mut self) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::despawn).
    fn rev_despawn(self);

    /// Reversible version of [`modify_component`](EntityWorldMut::despawn_recursive).
    fn rev_despawn_recursive(self);

    /// Reversible version of [`modify_component`](EntityWorldMut::is_despawned).
    fn rev_is_despawned(&self) -> bool;

    /// Reversible version of [`modify_component`](EntityWorldMut::clone_with).
    fn rev_clone_with(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::clone_and_spawn).
    fn rev_clone_and_spawn(&mut self) -> Entity;

    /// Reversible version of [`modify_component`](EntityWorldMut::clone_and_spawn_with).
    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`modify_component`](EntityWorldMut::clone_components).
    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;

    /// Reversible version of [`modify_component`](EntityWorldMut::move_components).
    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;
}

impl RevEntityWorldMut for EntityWorldMut<'_> {
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        insert_inner(self, bundle, InsertMode::Replace)
    }

    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        insert_inner(self, bundle, InsertMode::Keep)
    }

    unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        todo!()
    }

    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        todo!()
    }

    fn rev_remove<T: Bundle>(&mut self) -> &mut Self {
        remove_inner::<T, false>(self)
    }

    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        remove_inner::<T, true>(self)
    }

    fn rev_retain<T: Bundle>(&mut self) -> &mut Self {
        todo!()
    }

    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        todo!()
    }

    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        todo!()
    }

    fn rev_clear(&mut self) -> &mut Self {
        todo!()
    }

    fn rev_despawn(self) {
        todo!()
    }

    fn rev_despawn_recursive(self) {
        todo!()
    }

    fn rev_is_despawned(&self) -> bool {
        todo!()
    }

    fn rev_clone_with(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    fn rev_clone_and_spawn(&mut self) -> Entity {
        todo!()
    }

    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity {
        todo!()
    }

    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        todo!()
    }

    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        todo!()
    }
}

pub trait RevEntry<'w, 'a, T: Component> {
    fn insert_entry(self, component: T) -> OccupiedEntry<'w, 'a, T>;
    fn or_insert(self, default: T) -> OccupiedEntry<'w, 'a, T>;
    fn or_insert_with<F: FnOnce() -> T>(self, default: F) -> OccupiedEntry<'w, 'a, T>;
}

pub trait RevEntryDefault<'w, 'a, T: Component + Default> {
    fn or_default(self) -> OccupiedEntry<'w, 'a, T>;
}

struct RevInsertKeep {
    entity: Entity,
    buffer: Entity,
    components: Box<[ComponentId]>,
}

impl RevInsertKeep {
    fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
        // todo falsch, DespawnAtOutOfLog nicht bewegen!
        let mut mover = move_components(world, self.components.iter().copied(), false);
        if UNDO {
            mover.clone_entity(world, self.entity, self.buffer);
        } else {
            mover.clone_entity(world, self.buffer, self.entity);
        }
    }
}

impl UndoRedo for RevInsertKeep {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo::<true>(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo::<false>(world);
    }
}

struct RevInsertReplace<Insert, Backup> {
    entity: Entity,
    buffer: Entity,
    components: ReplaceComponents<Insert, Backup>,
}

impl<Insert, Backup> RevInsertReplace<Insert, Backup>
where
    for<'a> &'a Insert: IntoIterator<Item = &'a ComponentId>,
    for<'a> &'a Backup: IntoIterator<Item = &'a ComponentId>,
{
    fn init(&self, world: &mut World) {
        move_components(world, (&self.components.backup).into_iter().copied(), false).clone_entity(
            world,
            self.entity,
            self.buffer,
        );
    }
    fn undo_redo<const UNDO: bool>(&mut self, world: &mut World) {
        let (mut mover1, mut mover2) = self.components.movers::<UNDO>(world);
        world.empty_entity_scope(|world, empty_entity| {
            mover1.clone_entity(world, self.entity, *empty_entity);
            mover2.clone_entity(world, self.buffer, self.entity);
            std::mem::swap(&mut self.buffer, empty_entity);
        })
    }
}

impl<Insert, Backup> UndoRedo for RevInsertReplace<Insert, Backup>
where
    Insert: Send + 'static,
    Backup: Send + 'static,
    for<'a> &'a Insert: IntoIterator<Item = &'a ComponentId>,
    for<'a> &'a Backup: IntoIterator<Item = &'a ComponentId>,
{
    fn undo(&mut self, world: &mut World) {
        self.undo_redo::<true>(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo::<false>(world);
    }
}

fn bundle_id_and_buffer<B: Bundle>(entity: &mut EntityWorldMut) -> (BundleId, Entity) {
    let marker = DespawnAtOutOfLog::from(entity.world());
    unsafe {
        // SAFETY:
        // - registering bundle
        // - spawning a new entity
        // ... leave the entity location unaffected
        let world = entity.world_mut();
        (world.register_bundle::<B>().id(), world.spawn(marker).id())
    }
}

fn insert_inner<'a, 'w: 'a, B: Bundle>(
    entity: &'a mut EntityWorldMut<'w>,
    bundle: B,
    mode: InsertMode,
) -> &'a mut EntityWorldMut<'w> {
    let (bundle_id, buffer) = bundle_id_and_buffer::<B>(entity);
    let bundle_info = entity.world().bundles().get(bundle_id).unwrap();

    if mode == InsertMode::Replace {
        let backup = archetype_insert_replace_remove(bundle_info, entity.archetype());
        if !backup.is_empty() {
            let insert = archetype_insert_replace(bundle_info, entity.archetype());
            let id = entity.id();
            entity.world_scope(|world| {
                let undo_redo = RevInsertReplace {
                    entity: id,
                    buffer,
                    components: ReplaceComponents { insert, backup },
                };
                undo_redo.init(world);
                world.buffer_undo_redo(undo_redo);
            });
            entity.insert(bundle);
            return entity;
        }
    }

    let undo_redo = RevInsertKeep {
        entity: entity.id(),
        buffer,
        components: archetype_insert_keep(bundle_info, entity.archetype()),
    };
    entity.buffer_undo_redo(undo_redo);
    entity.insert_if_new(bundle)
}

struct Remove<Components> {
    entity: Entity,
    buffer: Entity,
    components: Components,
}

impl<Components> Remove<Components>
where
    for<'a> &'a Components: IntoIterator<Item = &'a ComponentId>,
{
    fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
        let mut mover = move_components(world, (&self.components).into_iter().copied(), false);
        if UNDO {
            mover.clone_entity(world, self.buffer, self.entity);
        } else {
            mover.clone_entity(world, self.entity, self.buffer);
        }
    }
}

impl<Components> UndoRedo for Remove<Components>
where
    Components: Send + 'static,
    for<'a> &'a Components: IntoIterator<Item = &'a ComponentId>,
{
    fn undo(&mut self, world: &mut World) {
        self.undo_redo::<true>(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo::<false>(world);
    }
}

fn remove_inner<'a, 'w, B: Bundle, const WITH_REQUIRES: bool>(
    entity: &'a mut EntityWorldMut<'w>,
) -> &'a mut EntityWorldMut<'w> {
    let (bundle_id, buffer) = bundle_id_and_buffer::<B>(entity);
    let bundle_info = entity.world().bundles().get(bundle_id).unwrap();
    let components = if WITH_REQUIRES {
        archetype_replace_with_requires(bundle_info, entity.archetype())
    } else {
        archetype_insert_replace_remove(bundle_info, entity.archetype())
    };
    let undo_redo = Remove {
        entity: entity.id(),
        buffer,
        components,
    };
    entity.world_scope(|world| {
        undo_redo.undo_redo::<false>(world);
    });
    if WITH_REQUIRES {
        entity.remove_with_requires::<B>();
    } else {
        entity.remove::<B>();
    }
    entity.buffer_undo_redo(undo_redo)
}

/// Reversible version of [`insert`](bevy::ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<B: Bundle>(bundle: B, mode: InsertMode) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        insert_inner(&mut entity, bundle, mode);
    }
}

/// Reversible version of [`insert_by_id`](bevy::ecs::system::entity_command::insert_by_id).
///
/// # Safety
///
/// - [`ComponentId`] must be from the same world as the target entity.
/// - `T` must have the same layout as the one passed during `component_id` creation.
pub unsafe fn rev_insert_by_id<T: Component + Send + 'static>(
    component_id: ComponentId,
    value: T,
    mode: InsertMode,
) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        let marker = DespawnAtOutOfLog::from(entity.world());
        let buffer = unsafe {
            // SAFETY: Spawning a new entity does not affect the current entity's location
            entity.world_mut().spawn(marker).id()
        };

        let components = |include_component: bool| -> Box<[ComponentId]> {
            let archetype = entity.archetype();
            entity
                .world()
                .components()
                .get_info(component_id)
                .expect("todo")
                .required_components()
                .iter_ids()
                .filter(|component_id| !archetype.contains(*component_id))
                .chain(include_component.then_some(component_id))
                .collect()
        };

        let contains_component = entity.contains_id(component_id);
        if contains_component && mode == InsertMode::Replace {
            let insert = components(true);
            let id = entity.id();
            entity.world_scope(|world| {
                let undo_redo = RevInsertReplace {
                    entity: id,
                    buffer,
                    components: ReplaceComponents {
                        insert,
                        backup: [component_id],
                    },
                };
                undo_redo.init(world);
                world.buffer_undo_redo(undo_redo);
            });
        } else {
            let undo_redo = RevInsertKeep {
                entity: entity.id(),
                buffer,
                components: components(contains_component),
            };
            entity.buffer_undo_redo(undo_redo);
        }
        unsafe {
            // SAFETY:
            // - `component_id` safety is ensured by the caller
            // - `ptr` is valid within the `make` block
            insert_by_id(component_id, value, mode);
        }
    }
}

/// Reversible version of [`insert_from_world`](bevy::ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(mode: InsertMode) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        let value = entity.world_scope(|world| T::from_world(world));
        insert_inner(&mut entity, value, mode);
    }
}

/// Reversible version of [`remove`](bevy::ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<B: Bundle>() -> impl EntityCommand {
    remove_inner::<B, false>
}

/// Reversible version of [`remove_with_requires`](bevy::ecs::system::entity_command::remove_with_requires).
#[track_caller]
pub fn rev_remove_with_requires<B: Bundle>() -> impl EntityCommand {
    remove_inner::<B, true>
}

/// Reversible version of [`remove_by_id`](bevy::ecs::system::entity_command::remove_by_id).
#[track_caller]
pub fn rev_remove_by_id(component_id: ComponentId) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        if !entity.contains_id(component_id) {
            return;
        }
        let marker = DespawnAtOutOfLog::from(entity.world());
        let buffer = unsafe {
            // SAFETY: spawning a new entity leaves the current entity's location unaffected
            entity.world_mut().spawn(marker).id()
        };
        let undo_redo = Remove {
            entity: entity.id(),
            buffer,
            components: [component_id],
        };
        entity.world_scope(|world| {
            undo_redo.undo_redo::<false>(world);
        });
        entity.remove_by_id(component_id);
        entity.buffer_undo_redo(undo_redo);
    }
}

/// Reversible version of [`clear`](bevy::ecs::system::entity_command::clear).
#[track_caller]
pub fn rev_clear() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`retain`](bevy::ecs::system::entity_command::retain).
#[track_caller]
pub fn rev_retain<B: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`despawn`](bevy::ecs::system::entity_command::despawn).
#[track_caller]
pub fn rev_despawn() -> impl EntityCommand {
    todo!()
}
