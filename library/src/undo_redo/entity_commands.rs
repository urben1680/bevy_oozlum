use bevy::ecs::{
    bundle::{Bundle, InsertMode},
    component::{Component, ComponentId},
    entity::Entity,
    system::{entity_command::insert_by_id, EntityCommand},
    world::FromWorld,
};

use super::*;

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

fn insert_inner<B: Bundle>(mut entity: EntityWorldMut, bundle: B, mode: InsertMode) {
    let bundle_id = unsafe {
        // SAFETY: registering bundle leaves entity location unaffected
        entity.world_mut().register_bundle::<B>().id()
    };
    let bundle_info = entity.world().bundles().get(bundle_id).unwrap();

    if mode == InsertMode::Replace {
        let backup = archetype_insert_replace_backup(bundle_info, entity.archetype());
        if !backup.is_empty() {
            let insert = archetype_insert_replace(bundle_info, entity.archetype());
            let id = entity.id();
            entity.world_scope(|world| {
                let marker = DespawnAtOutOfLog::from_world(world);
                let buffer = world.spawn(marker).id();
                let undo_redo = RevInsertReplace {
                    entity: id,
                    buffer,
                    components: ReplaceComponents { insert, backup },
                };
                undo_redo.init(world);
                world.buffer_undo_redo(undo_redo);
            });
            entity.insert(bundle);
            return;
        }
    }

    let undo_redo = RevInsertKeep {
        entity: entity.id(),
        buffer: entity.world().entities().reserve_entity(),
        components: archetype_insert_keep(bundle_info, entity.archetype()),
    };
    entity.buffer_undo_redo(undo_redo);
    entity.insert_if_new(bundle);
}

/// Reversible version of [`insert`](bevy::ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<B: Bundle>(bundle: B, mode: InsertMode) -> impl EntityCommand {
    move |entity: EntityWorldMut| {
        insert_inner(entity, bundle, mode);
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
                let marker = DespawnAtOutOfLog::from_world(world);
                let buffer = world.spawn(marker).id();
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
                buffer: entity.world().entities().reserve_entity(),
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
        insert_inner(entity, value, mode);
    }
}

/// Reversible version of [`remove`](bevy::ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<T: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`remove_with_requires`](bevy::ecs::system::entity_command::remove_with_requires).
#[track_caller]
pub fn rev_remove_with_requires<T: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`remove_by_id`](bevy::ecs::system::entity_command::remove_by_id).
#[track_caller]
pub fn rev_remove_by_id(component_id: ComponentId) -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`clear`](bevy::ecs::system::entity_command::clear).
#[track_caller]
pub fn rev_clear() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`retain`](bevy::ecs::system::entity_command::retain).
#[track_caller]
pub fn rev_retain<T: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`despawn`](bevy::ecs::system::entity_command::despawn).
#[track_caller]
pub fn rev_despawn() -> impl EntityCommand {
    todo!()
}
