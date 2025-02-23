use bevy::ecs::{
    bundle::{Bundle, InsertMode},
    component::{Component, ComponentId},
    entity::Entity,
    system::EntityCommand,
    world::FromWorld,
};

use super::*;

/// Reversible version of [`insert`](bevy::ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<B: Bundle>(bundle: B) -> impl EntityCommand {
    struct RevInsert {
        entity: Entity,
        buffer: Entity,
        components: ReplaceComponents
    }

    impl RevInsert {
        fn init(&self, world: &mut World) {
            move_components(world, self.components.backup.clone(), false)
                .clone_entity(world, self.entity, self.buffer);
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

    impl UndoRedo for RevInsert {
        fn undo(&mut self, world: &mut World) {
            self.undo_redo::<true>(world);
        }
        fn redo(&mut self, world: &mut World) {
            self.undo_redo::<false>(world);
        }
    }

    move |mut entity: EntityWorldMut| {
        let bundle_id = unsafe {
            // SAFETY: get_bundle_id removes a bundle form another empty entity, current entity is unaffected
            get_bundle_id::<B>(entity.world_mut())
        };
        let bundle_info = entity.world().bundles().get(bundle_id).expect("todo");
        let backup = archetype_insert_replace_backup(bundle_info, entity.archetype());
        if backup.is_empty() {
            rev_insert_if_new(bundle);
            return;
        }
        let insert = archetype_insert_replace(bundle_info, entity.archetype());
        let id = entity.id();
        entity.world_scope(|world| {
            let marker = DespawnAtOutOfLog::from_world(world);
            let buffer = world.spawn(marker).id();
            let undo_redo = RevInsert {
                entity: id,
                buffer,
                components: ReplaceComponents {
                    insert,
                    backup
                }
            };
            undo_redo.init(world);
            world.buffer_undo_redo(undo_redo);
        });
        entity.insert(bundle);
    }
}

/// Reversible version of [`insert_if_new`](bevy::ecs::system::entity_command::insert_if_new).
#[track_caller]
pub fn rev_insert_if_new<B: Bundle>(bundle: B) -> impl EntityCommand {
    struct RevInsertIfNew {
        entity: Entity,
        buffer: Entity,
        components: Box<[ComponentId]>,
    }

    impl RevInsertIfNew {
        fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
            // todo falsch, DespawnAtOutOfLog nicht bewegen!
            let mut mover = move_components(world, self.components.clone(), false);
            if UNDO {
                mover.clone_entity(world, self.entity, self.buffer);
            } else {
                mover.clone_entity(world, self.buffer, self.entity);
            }
        }
    }

    impl UndoRedo for RevInsertIfNew {
        fn undo(&mut self, world: &mut World) {
            self.undo_redo::<true>(world);
        }
        fn redo(&mut self, world: &mut World) {
            self.undo_redo::<false>(world);
        }
    }
    
    move |mut entity: EntityWorldMut| {
        let bundle_id = unsafe {
            // SAFETY: does not change current entity's location, only registers bundle by removing it from an empty entity
            get_bundle_id::<B>(entity.world_mut())
        };
        let bundle_info = entity.world().bundles().get(bundle_id).expect("todo");
        let buffer = entity.world().entities().reserve_entity();
        let undo_redo = RevInsertIfNew {
            entity: entity.id(),
            buffer,
            components: archetype_insert_if_new(bundle_info, entity.archetype()),
        };
        entity.buffer_undo_redo(undo_redo);
        entity.insert_if_new(bundle);
    }
}

/// Reversible version of [`insert_by_id`](bevy::ecs::system::entity_command::insert_by_id).
#[track_caller]
pub fn rev_insert_by_id<T: Send + 'static>(
    component_id: ComponentId,
    value: T,
) -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`insert_from_world`](bevy::ecs::system::entity_command::insert_from_world).
/// An [`EntityCommand`] that adds a component to an entity using
/// the component's [`FromWorld`] implementation.
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(mode: InsertMode) -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`remove`](bevy::ecs::system::entity_command::remove).
/// An [`EntityCommand`] that removes the components in a [`Bundle`] from an entity.
#[track_caller]
pub fn rev_remove<T: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`remove_with_requires`](bevy::ecs::system::entity_command::remove_with_requires).
/// An [`EntityCommand`] that removes the components in a [`Bundle`] from an entity,
/// as well as the required components for each component removed.
#[track_caller]
pub fn rev_remove_with_requires<T: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`remove_by_id`](bevy::ecs::system::entity_command::remove_by_id).
/// An [`EntityCommand`] that removes a dynamic component from an entity.
#[track_caller]
pub fn rev_remove_by_id(component_id: ComponentId) -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`clear`](bevy::ecs::system::entity_command::clear).
/// An [`EntityCommand`] that removes all components from an entity.
#[track_caller]
pub fn rev_clear() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`retain`](bevy::ecs::system::entity_command::retain).
/// An [`EntityCommand`] that removes all components from an entity,
/// except for those in the given [`Bundle`].
#[track_caller]
pub fn rev_retain<T: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`despawn`](bevy::ecs::system::entity_command::despawn).
/// An [`EntityCommand`] that despawns an entity.
///
/// # Note
///
/// This will also despawn any [`Children`](crate::hierarchy::Children) entities, and any other [`RelationshipTarget`](crate::relationship::RelationshipTarget) that is configured
/// to despawn descendants. This results in "recursive despawn" behavior.
#[track_caller]
pub fn rev_despawn() -> impl EntityCommand {
    todo!()
}
