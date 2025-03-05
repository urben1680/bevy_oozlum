use bevy::{
    ecs::{
        bundle::{Bundle, InsertMode},
        component::{Component, ComponentId},
        entity::{Entity, EntityClonerBuilder},
        hierarchy::{ChildSpawner, Children},
        system::{entity_command::insert_by_id, EntityCommand},
        world::{EntityRef, OccupiedEntry},
    },
    ptr::OwningPtr,
};

use super::*;

pub trait RevEntityWorldMut {
    /// Reversible version of [`EntityWorldMut::insert`].
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_by_id`].
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

    /// Reversible version of [`EntityWorldMut::insert_by_ids`].
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

    /// Reversible version of [`EntityWorldMut::remove`].
    fn rev_remove<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_with_requires`].
    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::retain`].
    fn rev_retain<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_id`].
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_ids`].
    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear`].
    fn rev_clear(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    ///
    /// Until then the entity is disabled via the [`DespawnAtOutOfLog`] component.
    fn rev_despawn(self);

    /// Reversible version of [`EntityWorldMut::is_despawned`].
    fn rev_is_despawned(&self) -> bool;

    /// Reversible version of [`EntityWorldMut::clone_with`].
    fn rev_clone_with(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    fn rev_clone_and_spawn(&mut self) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with`].
    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_components`].
    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::move_components`].
    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::with_children`].
    fn rev_with_children(&mut self, func: impl FnOnce(&mut ChildSpawner)) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_children`].
    fn rev_add_children(&mut self, children: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_child`].
    fn rev_add_child(&mut self, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::with_child`].
    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self;

    fn rev_buffer_components(
        &mut self,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> &mut Self;

    fn rev_buffer_components_at_undo(
        &mut self,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> &mut Self;

    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self;

    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self;
}

impl RevEntityWorldMut for EntityWorldMut<'_> {
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        let archetype_id = self.archetype().id();
        let bundle_id = unsafe {
            // SAFETY: Bundle registration does not affect entity location
            self.world_mut().register_bundle::<T>().id()
        };
        self.rev_buffer_components_cached(
            unique_for_location!(archetype_id, bundle_id),
            |world: &mut World| {
                let archetype = world.archetypes().get(archetype_id).unwrap();
                world
                    .bundles()
                    .get(bundle_id)
                    .unwrap()
                    .explicit_components()
                    .iter()
                    .copied()
                    .filter(|component_id| archetype.contains(*component_id))
                    .collect::<Vec<ComponentId>>()
            },
        )
        .rev_buffer_components_at_undo_cached(
            unique_for_location!(archetype_id, bundle_id),
            |world: &mut World| {
                let archetype = world.archetypes().get(archetype_id).unwrap();
                let bundle = world.bundles().get(bundle_id).unwrap();
                bundle
                    .required_components()
                    .iter()
                    .copied()
                    .filter(|component_id| !archetype.contains(*component_id))
                    .chain(bundle.explicit_components().iter().copied())
                    .collect::<Vec<ComponentId>>()
            },
        )
        .insert(bundle)
    }

    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        let archetype_id = self.archetype().id();
        let entity = self.id();
        let noop = self.world_scope(|world| {
            let bundle_id = world.register_bundle::<T>().id();
            world.rev_buffer_components_at_undo_cached(
                entity,
                unique_for_location!(archetype_id, bundle_id),
                |world: &mut World| {
                    let archetype = world.archetypes().get(archetype_id).unwrap();
                    world
                        .bundles()
                        .get(bundle_id)
                        .unwrap()
                        .contributed_components()
                        .iter()
                        .copied()
                        .filter(|component_id| !archetype.contains(*component_id))
                        .collect::<Vec<ComponentId>>()
                },
            )
        });
        if !noop {
            self.insert_if_new(bundle);
        }
        self
    }

    unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        let archetype_id = self.archetype().id();
        if self.contains_id(component_id) {
            self.rev_buffer_components([component_id]);
        }
        self.rev_buffer_components_at_undo_cached(
            unique_for_location!(archetype_id, component_id),
            |world: &mut World| {
                let archetype = world.archetypes().get(archetype_id).unwrap();
                world
                    .components()
                    .get_info(component_id)
                    .unwrap()
                    .required_components()
                    .iter_ids()
                    .filter(|component_id| !archetype.contains(*component_id))
                    .chain([component_id])
                    .collect::<Vec<ComponentId>>()
            },
        )
        .insert_by_id(component_id, component)
    }

    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        let archetype = self.archetype();
        let backup_components = component_ids
            .into_iter()
            .copied()
            .filter(|component_id| archetype.contains(*component_id))
            .collect::<Vec<ComponentId>>();
        let insert_components = component_ids
            .into_iter()
            .copied()
            .flat_map(|component_id| {
                self.world()
                    .components()
                    .get_info(component_id)
                    .unwrap()
                    .required_components()
                    .iter_ids()
                    .chain([component_id])
            })
            .filter(|component_id| !archetype.contains(*component_id))
            .collect::<HashSet<ComponentId>>();
        if !backup_components.is_empty() {
            self.rev_buffer_components(backup_components);
        }
        self.rev_buffer_components_at_undo(insert_components)
            .insert_by_ids(component_ids, iter_components)
    }

    fn rev_remove<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        let entity = self.id();
        self.world_scope(|world| {
            let bundle_id = world.register_bundle::<T>().id();
            world.rev_buffer_components_cached(
                entity,
                unique_for_location!(archetype_id, bundle_id),
                |world: &mut World| {
                    let archetype = world.archetypes().get(archetype_id).unwrap();
                    world
                        .bundles()
                        .get(bundle_id)
                        .unwrap()
                        .explicit_components()
                        .into_iter()
                        .copied()
                        .filter(|component_id| archetype.contains(*component_id))
                        .collect::<Vec<ComponentId>>()
                },
            );
        });
        self
    }

    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        let entity = self.id();
        self.world_scope(|world| {
            let bundle_id = world.register_bundle::<T>().id();
            world.rev_buffer_components_cached(
                entity,
                unique_for_location!(archetype_id, bundle_id),
                |world: &mut World| {
                    let archetype = world.archetypes().get(archetype_id).unwrap();
                    world
                        .bundles()
                        .get(bundle_id)
                        .unwrap()
                        .contributed_components()
                        .into_iter()
                        .copied()
                        .filter(|component_id| archetype.contains(*component_id))
                        .collect::<Vec<ComponentId>>()
                },
            );
        });
        self
    }

    fn rev_retain<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        let entity = self.id();
        self.world_scope(|world| {
            let bundle_id = world.register_bundle::<T>().id();
            world.rev_buffer_components_cached(
                entity,
                unique_for_location!(archetype_id, bundle_id),
                |world: &mut World| {
                    let bundle_components = world
                        .bundles()
                        .get(bundle_id)
                        .unwrap()
                        .contributed_components()
                        .into_iter()
                        .copied()
                        .collect::<HashSet<ComponentId>>();
                    world
                        .archetypes()
                        .get(archetype_id)
                        .unwrap()
                        .components()
                        .filter(|component_id| !bundle_components.contains(component_id))
                        .collect::<Vec<ComponentId>>()
                },
            );
        });
        self
    }

    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        if !self.contains_id(component_id) {
            return self;
        }
        let entity = self.id();
        self.world_scope(|world| {
            world.rev_buffer_components(entity, [component_id]);
        });
        self
    }

    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        let entity = self.id();
        self.world_scope(|world| {
            world.rev_buffer_components(entity, component_ids.into_iter().copied());
        });
        self
    }

    fn rev_clear(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        let entity = self.id();
        self.world_scope(|world| {
            world.rev_buffer_components_cached(
                entity,
                unique_for_location!(archetype_id),
                |world: &mut World| {
                    world
                        .archetypes()
                        .get(archetype_id)
                        .unwrap()
                        .components()
                        .collect::<Box<[ComponentId]>>() // stored as such in archetype
                },
            );
        });
        self
    }

    fn rev_despawn(self) {
        rev_despawn_inner(self);
    }

    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
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

    fn rev_with_children(&mut self, func: impl FnOnce(&mut ChildSpawner)) -> &mut Self {
        todo!()
    }

    fn rev_add_children(&mut self, children: &[Entity]) -> &mut Self {
        todo!()
    }

    fn rev_add_child(&mut self, child: Entity) -> &mut Self {
        todo!()
    }

    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self {
        todo!()
    }

    fn rev_buffer_components(
        &mut self,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> &mut Self {
        let entity = self.id();
        self.world_scope(|world| world.rev_buffer_components(entity, components));
        self
    }

    fn rev_buffer_components_at_undo(
        &mut self,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> &mut Self {
        let entity = self.id();
        let world = unsafe {
            // SAFETY: only resources are mutated
            self.world_mut()
        };
        world.rev_buffer_components_at_undo(entity, components);
        self
    }

    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self {
        let entity = self.id();
        self.world_scope(|world| world.rev_buffer_components_cached(entity, cache, components));
        self
    }

    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self {
        let entity = self.id();
        let world = unsafe {
            // SAFETY: only resources are mutated
            self.world_mut()
        };
        world.rev_buffer_components_at_undo_cached(entity, cache, components);
        self
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
    todo!()
}

/// Reversible version of [`remove`](bevy::ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<B: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`remove_with_requires`](bevy::ecs::system::entity_command::remove_with_requires).
#[track_caller]
pub fn rev_remove_with_requires<B: Bundle>() -> impl EntityCommand {
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
pub fn rev_retain<B: Bundle>() -> impl EntityCommand {
    todo!()
}

/// Reversible version of [`despawn`](bevy::ecs::system::entity_command::despawn).
#[track_caller]
pub fn rev_despawn() -> impl EntityCommand {
    todo!()
}
