use bevy::ecs::{
    bundle::{Bundle, InsertMode},
    component::{Component, ComponentId},
    entity::{Entity, EntityCloneBuilder},
    system::{
        entity_command::{insert, insert_if_new},
        EntityCommand,
    },
    world::FromWorld,
};

use super::*;

/// Reversible version of [`insert`](bevy::ecs::system::entity_command::insert).
/// An [`EntityCommand`] that adds the components in a [`Bundle`] to an entity,
/// replacing any that were already present.
#[track_caller]
pub fn rev_insert<B: Bundle>(bundle: B) -> impl EntityCommand {
    struct RevInsert {
        entity: Entity,
        buffer: Entity,
        insert: Box<[ComponentId]>,
        backup: Box<[ComponentId]>,
    }

    impl RevInsert {
        fn init(&self, world: &mut World) {
            let mut builder = EntityCloneBuilder::new(world);
            let components = self.backup.clone();
            builder
                .deny_all()
                .without_required_components(|builder| {
                    builder.allow_by_ids(components);
                })
                .move_components(true);
            builder.clone_entity(self.entity, self.buffer);
        }
        fn undo_redo<const UNDO: bool>(&mut self, world: &mut World) {
            let (components1, components2) = if UNDO {
                (&self.insert, &self.backup)
            } else {
                (&self.backup, &self.insert)
            };

            empty_entity_scope(world, |world, empty_entity| {
                let mut builder = EntityCloneBuilder::new(world);
                let components1 = components1.clone();
                builder
                    .deny_all()
                    .without_required_components(|builder| {
                        builder.allow_by_ids(components1);
                    })
                    .move_components(true);
                builder.clone_entity(self.entity, *empty_entity);

                let mut builder = EntityCloneBuilder::new(world);
                let components2 = components2.clone();
                builder
                    .deny_all()
                    .without_required_components(|builder| {
                        builder.allow_by_ids(components2);
                    })
                    .move_components(true);
                builder.clone_entity(self.buffer, self.entity);

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

    todo!()
}

/// Reversible version of [`insert_if_new`](bevy::ecs::system::entity_command::insert_if_new).
/// An [`EntityCommand`] that adds the components in a [`Bundle`] to an entity,
/// except for any that were already present.
#[track_caller]
pub fn rev_insert_if_new<B: Bundle>(bundle: B) -> impl EntityCommand {
    struct RevInsertIfNew {
        entity: Entity,
        buffer: Entity,
        components: Box<[ComponentId]>,
    }

    impl RevInsertIfNew {
        fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
            let components = self.components.clone();
            let mut builder = EntityCloneBuilder::new(world);
            builder
                .deny_all()
                .without_required_components(|builder| {
                    builder.allow_by_ids(components);
                })
                .move_components(true);
            if UNDO {
                builder.clone_entity(self.entity, self.buffer);
            } else {
                builder.clone_entity(self.buffer, self.entity);
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

    struct RevInsertIfNewBuffer(Entity);
    /*
        impl Finalize for RevInsertIfNewBuffer {
            fn finalize_redone(self: Box<Self>, world: &mut World) {
                world.despawn(self.0);
            }
            fn finalize_undone(self: Box<Self>, world: &mut World) {
                world.despawn(self.0);
            }
        }
    */
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
        entity
            .buffer_undo_redo(undo_redo)
            .buffer_finalize(RevInsertIfNewBuffer(buffer));
        insert_if_new(bundle);
    }
}

/// Reversible version of [`insert_by_id`](bevy::ecs::system::entity_command::insert_by_id).
/// An [`EntityCommand`] that adds a dynamic component to an entity.
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
