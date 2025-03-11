use bevy::{
    ecs::{
        archetype::Archetype,
        bundle::{Bundle, InsertMode},
        component::{Component, ComponentId},
        entity::{Entity, EntityClonerBuilder},
        hierarchy::{ChildSpawner, Children},
        relationship::{RelatedSpawner, Relationship, RelationshipTarget},
        system::{entity_command::insert_by_id, EntityCommand},
        world::OccupiedEntry,
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

    /// Reversible version of [`EntityWorldMut::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        func: impl FnOnce(&mut RelatedSpawner<R>),
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_related`].
    fn rev_add_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self;

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

    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self;

    fn rev_buffer_components_at_undo(
        &mut self,
        components: impl IntoIterator<Item = ComponentId>,
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
        let buffer_entity = self.world_scope(|world| {
            let bundle_id = world.register_bundle::<T>().id();
            world.buffer_components_at_undo_cached(
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
        if buffer_entity.is_none() {
            self.insert_if_new(bundle);
        }
        self
    }

    unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        rev_insert_inner(self, component_id);
        self.insert_by_id(component_id, component)
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
            world.buffer_components_cached(
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
            world.buffer_components_cached(
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
            world.buffer_components_cached(
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
            world.buffer_components(entity, [component_id]);
        });
        self
    }

    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        let entity = self.id();
        self.world_scope(|world| {
            world.buffer_components(entity, component_ids.into_iter().copied());
        });
        self
    }

    fn rev_clear(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        let entity = self.id();
        self.world_scope(|world| {
            world.buffer_components_cached(
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

    fn rev_clone_and_spawn(&mut self) -> Entity {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let entity = self.clone_and_spawn();
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let entity = self.clone_and_spawn_with(config);
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        let bundle_id = unsafe {
            // SAFETY: Bundle registration does not affect entity location
            self.world_mut().register_bundle::<B>().id()
        };
        let source_archetype_id = self.archetype().id();
        let target_archetype_id = self
            .world()
            .entities()
            .get(target)
            .unwrap_or_else(|| {
                // trigger vanilla panic
                self.clone_components::<B>(target);
                unreachable!()
            })
            .archetype_id;
        self.world_scope(|world| {
            world.buffer_components_cached(
                target,
                unique_for_location!(source_archetype_id, target_archetype_id, bundle_id),
                |world| {
                    let source_archetype = get_archetype(&world, source_archetype_id);
                    let target_archetype = get_archetype(&world, target_archetype_id);
                    world
                        .bundles()
                        .get(bundle_id)
                        .unwrap()
                        .explicit_components()
                        .iter()
                        .copied()
                        .filter(|&component_id| {
                            source_archetype.contains(component_id)
                                && target_archetype.contains(component_id)
                        })
                        .collect::<Vec<_>>()
                },
            );
            world.buffer_components_at_undo_cached(
                target,
                unique_for_location!(source_archetype_id, target_archetype_id, bundle_id),
                |world| {
                    let source_archetype = get_archetype(&world, source_archetype_id);
                    let target_archetype = get_archetype(&world, target_archetype_id);
                    let bundle_info = world.bundles().get(bundle_id).unwrap();
                    bundle_info
                        .explicit_components()
                        .iter()
                        .copied()
                        .filter(|&component_id| source_archetype.contains(component_id))
                        .chain(bundle_info.required_components().iter().copied().filter(
                            |&component_id| {
                                source_archetype.contains(component_id)
                                    && !target_archetype.contains(component_id)
                            },
                        ))
                        .collect::<Vec<_>>()
                },
            );
        });
        self.clone_components::<B>(target)
    }

    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        struct RevMove {
            source: Entity,
            target: Entity,
            components: Box<[ComponentId]>,
        }

        impl RevMove {
            fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
                let (target, source) = if UNDO {
                    (self.target, self.source)
                } else {
                    (self.source, self.target)
                };
                EntityCloner::build(world)
                    .deny_all()
                    .without_required_components(|builder| {
                        builder.allow_by_ids(self.components.iter().copied());
                    })
                    .move_components(true)
                    .clone_entity(source, target);
            }
        }

        impl UndoRedo for RevMove {
            fn undo(&mut self, world: &mut World) {
                self.undo_redo::<true>(world);
            }
            fn redo(&mut self, world: &mut World) {
                self.undo_redo::<false>(world);
            }
        }

        let source = self.id();
        let bundle_id = unsafe {
            // SAFETY: Bundle registration does not affect entity location
            self.world_mut().register_bundle::<B>().id()
        };
        let source_archetype_id = self.archetype().id();
        let target_archetype_id = self
            .world()
            .entities()
            .get(target)
            .unwrap_or_else(|| {
                // trigger vanilla panic
                self.move_components::<B>(target);
                unreachable!()
            })
            .archetype_id;
        let key = unique_for_location!(source_archetype_id, target_archetype_id, bundle_id);
        self.world_scope(|world| {
            world.buffer_components_cached(target, key, |world| {
                let source_archetype = get_archetype(&world, source_archetype_id);
                let target_archetype = get_archetype(&world, target_archetype_id);
                world
                    .bundles()
                    .get(bundle_id)
                    .unwrap()
                    .contributed_components() // EntityWorldMut::move_components treats required and explicit components the same
                    .iter()
                    .copied()
                    .filter(|&component_id| {
                        source_archetype.contains(component_id)
                            && target_archetype.contains(component_id)
                    })
                    .collect::<Vec<_>>()
            });

            let components = world
                .resource::<ComponentBufferRes>()
                .get_buffer_components(key)
                .iter()
                .copied()
                .collect();
            let mut undo_redo = RevMove {
                source,
                target,
                components,
            };
            undo_redo.redo(world);
            world.buffer_undo_redo(undo_redo);
        });
        self
    }

    fn rev_with_related<R: Relationship>(
        &mut self,
        func: impl FnOnce(&mut RelatedSpawner<R>),
    ) -> &mut Self {
        todo!()
    }

    fn rev_add_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self {
        let id = self.id();
        self.world_scope(|world| {
            for related in related {
                world.entity_mut(*related).rev_insert(R::from(id));
            }
        });
        self
    }

    fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self {
        self.rev_add_related::<R>(&[entity])
    }

    fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self {
        todo!();
    }

    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self {
        todo!()
    }

    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self {
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
        self.world_scope(|world| world.buffer_components(entity, components));
        self
    }

    fn rev_buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self {
        let entity = self.id();
        self.world_scope(|world| world.buffer_components_cached(entity, cache, components));
        self
    }

    fn rev_buffer_components_at_undo(
        &mut self,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> &mut Self {
        let entity = self.id();
        let world = unsafe {
            // SAFETY: only resources are mutated, no component moves happen yet
            self.world_mut()
        };
        world.buffer_components_at_undo(entity, components);
        self
    }

    fn rev_buffer_components_at_undo_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
    ) -> &mut Self {
        let entity = self.id();
        let world = unsafe {
            // SAFETY: only resources are mutated, no component moves happen yet
            self.world_mut()
        };
        world.buffer_components_at_undo_cached(entity, cache, components);
        self
    }
}

fn get_archetype(world: &World, archetype_id: ArchetypeId) -> &Archetype {
    world.archetypes().get(archetype_id).unwrap()
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
        match mode {
            InsertMode::Keep => entity.rev_insert_if_new(bundle),
            InsertMode::Replace => entity.rev_insert(bundle),
        };
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
        if entity.contains_id(component_id) && mode == InsertMode::Keep {
            return;
        }
        rev_insert_inner(&mut entity, component_id);
        insert_by_id(component_id, value, mode).apply(entity)
    }
}

fn rev_insert_inner(entity: &mut EntityWorldMut, component_id: ComponentId) {
    let archetype_id = entity.archetype().id();
    if entity.contains_id(component_id) {
        entity.rev_buffer_components([component_id]);
    }
    entity.rev_buffer_components_at_undo_cached(
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
    );
}

/// Reversible version of [`insert_from_world`](bevy::ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(mode: InsertMode) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        if entity.contains::<T>() && mode == InsertMode::Keep {
            return;
        }
        let value = entity.world_scope(|world| T::from_world(world));
        entity.rev_insert(value);
    }
}

/// Reversible version of [`remove`](bevy::ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<B: Bundle>() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_remove::<B>();
    }
}

/// Reversible version of [`remove_with_requires`](bevy::ecs::system::entity_command::remove_with_requires).
#[track_caller]
pub fn rev_remove_with_requires<B: Bundle>() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_remove_with_requires::<B>();
    }
}

/// Reversible version of [`remove_by_id`](bevy::ecs::system::entity_command::remove_by_id).
#[track_caller]
pub fn rev_remove_by_id(component_id: ComponentId) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        entity.rev_remove_by_id(component_id);
    }
}

/// Reversible version of [`clear`](bevy::ecs::system::entity_command::clear).
#[track_caller]
pub fn rev_clear() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.clear();
    }
}

/// Reversible version of [`retain`](bevy::ecs::system::entity_command::retain).
#[track_caller]
pub fn rev_retain<B: Bundle>() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_retain::<B>();
    }
}

/// Reversible version of [`despawn`](bevy::ecs::system::entity_command::despawn).
#[track_caller]
pub fn rev_despawn() -> impl EntityCommand {
    |entity: EntityWorldMut| {
        entity.rev_despawn();
    }
}
