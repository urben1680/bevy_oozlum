use std::{any::TypeId, marker::PhantomData};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleFromComponents},
        component::{Component, ComponentId},
        entity::{Entity, EntityClonerBuilder, OptIn, OptOut},
        hierarchy::ChildOf,
        relationship::{
            OrderedRelationshipSourceCollection, Relationship, RelationshipSourceCollection,
            RelationshipTarget,
        },
    },
    ptr::OwningPtr,
};

use crate::meta::NonLogNow;

use super::*;

#[cfg(test)]
mod test;

pub trait RevEntityWorldMut<'w> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    fn buffer_bundle(
        &mut self,
        now: NonLogNow,
        at: BufferAt,
        after_now_before_undo: impl FnOnce(&mut World),
        bundle: BundleId,
        if_new: bool
    ) -> Result<Option<Entity>, EntityRevDespawnedError>;

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`EntityWorldMut::add_child`].
    fn rev_add_child(&mut self, now: NonLogNow, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_children`].
    fn rev_add_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    fn rev_add_one_related<R: Relationship>(&mut self, now: NonLogNow, entity: Entity)
    -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_related`].
    fn rev_add_related<R: Relationship>(&mut self, now: NonLogNow, related: &[Entity])
    -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear`].
    fn rev_clear(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear_children`].
    fn rev_clear_children(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear_related`].
    fn rev_clear_related<R: Relationship>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn(&mut self, now: NonLogNow) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with_opt_in`].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with_opt_out].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_components`].
    fn rev_clone_components<B: Bundle>(&mut self, now: NonLogNow, target: Entity) -> &mut Self;

    // rev_clone_with
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    fn rev_despawn(self, now: NonLogNow);

    /// Reversible version of [`EntityWorldMut::despawn_children`].
    fn rev_despawn_children(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::entry`].
    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T>;

    /// Reversible version of [`EntityWorldMut::insert`].
    fn rev_insert<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_by_id`].
    ///
    /// # Safety
    ///
    /// - [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    unsafe fn rev_insert_by_id(
        &mut self,
        now: NonLogNow,
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
        now: NonLogNow,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_children`].
    fn rev_insert_children(
        &mut self,
        now: NonLogNow,
        index: usize,
        children: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    fn rev_insert_if_new<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle + Clone,
    ) -> &mut Self;

    // rev_insert_reflect
    // out of scope due complexity

    // rev_insert_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::insert_related`].
    fn rev_insert_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        index: usize,
        related: &[Entity],
    ) -> &mut Self
    where
        <R::RelationshipTarget as RelationshipTarget>::Collection:
            OrderedRelationshipSourceCollection;

    // rev_insert_with_relationship_hook_mode
    // missing EntityCloner API with RelationshipHookMode

    /// Reversible version of [`EntityWorldMut::move_components`].
    fn rev_move_components<B: Bundle>(&mut self, now: NonLogNow, target: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove`].
    fn rev_remove<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_id`].
    fn rev_remove_by_id(&mut self, now: NonLogNow, component_id: ComponentId) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_ids`].
    fn rev_remove_by_ids(&mut self, now: NonLogNow, component_ids: &[ComponentId]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_children`].
    fn rev_remove_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(
        &mut self,
        now: NonLogNow,
    ) -> &mut Self;

    // rev_remove_reflect
    // out of scope due complexity

    // rev_remove_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::remove_related`].
    fn rev_remove_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_with_requires`].
    fn rev_remove_with_requires<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_children`].
    fn rev_replace_children(&mut self, now: NonLogNow, related: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_children_with_difference`].
    fn rev_replace_children_with_difference(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_related`].
    fn rev_replace_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_related_with_difference`].
    fn rev_replace_related_with_difference<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::retain`].
    fn rev_retain<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::take`].
    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        now: NonLogNow,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out>;

    /// Reversible version of [`EntityWorldMut::with_child`].
    fn rev_with_child(&mut self, now: NonLogNow, bundle: impl Bundle) -> &mut Self;

    // rev_with_children
    // implemented via DespawnAtUndo

    /// Reversible version of [`EntityWorldMut::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle,
    ) -> &mut Self;

    // rev_with_related_entities
    // implemented via DespawnAtUndo
}

impl<'w> RevEntityWorldMut<'w> for EntityWorldMut<'w> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        self.world_scope(|world| world.redo_and_buffer(now, undo_redo))
    }

    fn buffer_bundle(
        &mut self,
        now: NonLogNow,
        at: BufferAt,
        after_now_before_undo: impl FnOnce(&mut World),
        bundle: BundleId,
        if_new: bool,
    ) -> Result<Option<Entity>, EntityRevDespawnedError> {
        let entity = self.id();
        let result = if at == BufferAt::Undo {
            unsafe {
                // SAFETY: No components of this entity are buffered now,
                // only resources are mutated and a bundle is registered.
                self.world_mut()
                    .buffer_bundle(now, entity, at, after_now_before_undo, bundle, if_new)
            }
        } else {
            self.world_scope(|world| {
                world.buffer_bundle(now, entity, at, after_now_before_undo, bundle, if_new)
            })
        };
        result.map_err(|err| match err {
            RevEntityError::EntityRevDespawnedError(err) => err,
            RevEntityError::EntityDoesNotExistError(_) => unreachable!("entity must exist"),
        })
    }

    fn rev_add_child(&mut self, now: NonLogNow, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(now, child)
    }

    fn rev_add_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self {
        self.rev_add_related::<ChildOf>(now, children)
    }

    fn rev_add_one_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entity: Entity,
    ) -> &mut Self {
        self.rev_add_related::<R>(now, &[entity])
    }

    fn rev_add_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        let id = self.id();
        self.world_scope(|world| {
            for related in related {
                world.entity_mut(*related).rev_insert(now, R::from(id));
            }
        });
        self
    }

    fn rev_clear(&mut self, now: NonLogNow) -> &mut Self {
        // todo: extend if_new by all variant
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_bundle(
            now,
            |_| (),
            unique_for_location!(archetype_id),
            |world| {
                let components: Vec<_> = world
                    .archetypes()
                    .get(archetype_id)
                    .unwrap()
                    .components()
                    .collect();
                (BufferAt::Now, components)
            },
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    fn rev_clear_children(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_clear_related::<ChildOf>(now)
    }

    fn rev_clear_related<R: Relationship>(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_remove::<R::RelationshipTarget>(now)
    }

    #[track_caller]
    fn rev_clone_and_spawn(&mut self, now: NonLogNow) -> Entity {
        self.rev_clone_and_spawn_with_opt_out(now, |_| {})
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity {
        let clone = self.clone_and_spawn_with_opt_in(config);
        let mut resource = self
            .world_scope(World::remove_resource::<RevRelationship>)
            .expect("todo");
        self.world_scope(|world| {
            let mut clone_mut = world.entity_mut(clone);
            let _ = resource.try_despawn(&mut clone_mut, now, false);
            world.insert_resource(resource);
        });
        clone
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity {
        let clone = self.clone_and_spawn_with_opt_out(config);
        let mut resource = self
            .world_scope(World::remove_resource::<RevRelationship>)
            .expect("todo");
        self.world_scope(|world| {
            let mut clone_mut = world.entity_mut(clone);
            let _ = resource.try_despawn(&mut clone_mut, now, false);
            world.insert_resource(resource);
        });
        clone
    }

    fn rev_clone_components<B: Bundle>(&mut self, now: NonLogNow, target: Entity) -> &mut Self {
        // if the target entity does not exist, let `clone_components` panic here
        let entity = self.id();
        if let Some(location) = self.world().entities().get(target) {
            let archetype_id = location.archetype_id;
            self.world_scope(|world| {
                let _ok = world.buffer_bundle(
                    now,
                    target,
                    |world| {
                        world.entity_mut(entity).clone_components::<B>(target);
                    },
                    unique_for_location!(archetype_id, TypeId::of::<B>()),
                    |world| {
                        let bundle_id = world.register_bundle::<B>().id();
                        pre_insert_maybe_overwrite(&world, bundle_id, archetype_id)
                    },
                );
            });
        }
        self
    }

    #[track_caller]
    fn rev_despawn(mut self, now: NonLogNow) {
        let entity = self.id();
        let mut resource = self
            .world_scope(World::remove_resource::<RevRelationship>)
            .expect("todo");
        let result = resource.try_despawn(&mut self, now, true);
        self.world_scope(|world| world.insert_resource(resource));
        if let Err(err) = result {
            panic!("entity {entity} could not be reversibly despawned: {err}")
        }
    }

    #[track_caller]
    fn rev_despawn_children(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_despawn_related::<Children>(now)
    }

    #[track_caller]
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self {
        if let Some(sources) = self.get::<S>() {
            let sources: Vec<_> = sources.iter().collect();
            self.world_scope(|world| {
                for entity in sources.into_iter() {
                    if let Ok(entity_mut) = world.get_entity_mut(entity) {
                        entity_mut.rev_despawn(now);
                    }
                }
            });
        }
        self
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

    fn rev_insert<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self {
        let entity = self.id();
        insert_inner(self, now, bundle, InsertMode::Replace)
            .unwrap_or_else(rev_despawned_panic(entity))
    }

    unsafe fn rev_insert_by_id(
        &mut self,
        now: NonLogNow,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        // todo: custom impl like insert_by_id?
        unsafe {
            // SAFETY: todo
            self.rev_insert_by_ids(now, &[component_id], [component].into_iter())
        }
    }

    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        now: NonLogNow,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let bundle_id = unsafe {
            // SAFETY: registering a bundle does not change the entity's location
            self.world_mut().register_dynamic_bundle(component_ids).id()
        };
        let entity = self.id();
        self.buffer_bundle(
            now,
            |world| {
                let mut entity_world_mut = world.entity_mut(entity);
                unsafe {
                    // SAFETY: todo
                    entity_world_mut.insert_by_ids(component_ids, iter_components);
                }
            },
            unique_for_location!(archetype_id, bundle_id),
            |world: &mut World| pre_insert_maybe_overwrite(world, bundle_id, archetype_id),
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    fn rev_insert_children(
        &mut self,
        now: NonLogNow,
        index: usize,
        children: &[Entity],
    ) -> &mut Self {
        self.rev_insert_related::<ChildOf>(now, index, children)
    }

    fn rev_insert_if_new<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self {
        let entity = self.id();
        insert_inner(self, now, bundle, InsertMode::Keep)
            .unwrap_or_else(rev_despawned_panic(entity))
    }

    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle + Clone,
    ) -> &mut Self {
        self.rev_insert(now, bundle.clone());
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.world_scope(|world| {
                    world
                        .entity_mut(related)
                        .rev_insert_recursive::<S>(now, bundle.clone());
                });
            }
        }

        self
    }

    fn rev_insert_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        index: usize,
        related: &[Entity],
    ) -> &mut Self
    where
        <R::RelationshipTarget as RelationshipTarget>::Collection:
            OrderedRelationshipSourceCollection,
    {
        let id = self.id();
        self.world_scope(|world| {
            for (offset, &related) in related.iter().enumerate() {
                let mut index = index + offset;
                if world
                    .get::<R>(related)
                    .is_some_and(|relationship| relationship.get() == id)
                {
                    let mut target = world
                        .get_mut::<R::RelationshipTarget>(id)
                        .expect("hooks should have added relationship target");
                    let collection = target.collection_mut_risky();
                    index = index.min(collection.len() - 1);
                    let old_index = collection.iter().position(|id| id == related).expect(
                        "hooks should have added the related entity to the relationship target",
                    );
                    if index != old_index {
                        collection.place(related, index);
                        world.buffer_undo_redo(
                            now,
                            InsertExistingRelated::<R> {
                                id,
                                related,
                                index,
                                old_index,
                                _marker: PhantomData,
                            },
                        );
                    }
                } else {
                    world.entity_mut(related).insert(R::from(id));
                    let mut target = world
                        .get_mut::<R::RelationshipTarget>(id)
                        .expect("hooks should have added relationship target");
                    let collection = target.collection_mut_risky();
                    index = index.min(collection.len());
                    collection.place_most_recent(index);
                    world.buffer_undo_redo(
                        now,
                        InsertNewRelated::<R> {
                            id,
                            related,
                            index,
                            _marker: PhantomData,
                        },
                    );
                }
            }
        });

        self
    }

    fn rev_move_components<B: Bundle>(&mut self, now: NonLogNow, target: Entity) -> &mut Self {
        // todo: when moving no longer requires Clone, replace this logic with non-Clone approach
        self.rev_clone_components::<B>(now, target)
            .rev_remove::<B>(now)
    }

    fn rev_remove<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_bundle(
            now,
            |_| (),
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                let bundle = world.bundles().get(bundle_id).unwrap();
                let archetype = world.archetypes().get(archetype_id).unwrap();
                let components: Vec<_> = bundle
                    .explicit_components()
                    .iter()
                    .filter(|component_id| archetype.contains(**component_id))
                    .copied()
                    .collect();
                (BufferAt::Now, components)
            },
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    fn rev_remove_by_id(&mut self, now: NonLogNow, component_id: ComponentId) -> &mut Self {
        // todo: custom impl like remove_by_id?
        self.rev_remove_by_ids(now, &[component_id])
    }

    fn rev_remove_by_ids(&mut self, now: NonLogNow, component_ids: &[ComponentId]) -> &mut Self {
        let archetype = self.archetype();
        let components: Vec<_> = component_ids
            .iter()
            .copied()
            .filter(|component_id| archetype.contains(*component_id))
            .collect();
        // must be Ok because self.archetype() did not panic
        let _ok = self.buffer_components(now, BufferAt::Now, |_| (), &components);
        self
    }

    fn rev_remove_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self {
        self.rev_remove_related::<ChildOf>(now, children)
    }

    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(
        &mut self,
        now: NonLogNow,
    ) -> &mut Self {
        self.rev_remove::<B>(now);
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.world_scope(|world| {
                    world.entity_mut(related).rev_remove_recursive::<S, B>(now);
                });
            }
        }

        self
    }

    fn rev_remove_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        let id = self.id();
        self.world_scope(|world| {
            for related in related {
                if world
                    .get::<R>(*related)
                    .is_some_and(|relationship| relationship.get() == id)
                {
                    world.entity_mut(*related).rev_remove::<R>(now);
                }
            }
        });

        self
    }

    fn rev_remove_with_requires<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_bundle(
            now,
            |_| (),
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                let bundle = world.bundles().get(bundle_id).unwrap();
                let archetype = world.archetypes().get(archetype_id).unwrap();
                let components: Vec<_> = bundle
                    .contributed_components()
                    .iter()
                    .filter(|component_id| archetype.contains(**component_id))
                    .copied()
                    .collect();
                (BufferAt::Now, components)
            },
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    fn rev_replace_children(&mut self, now: NonLogNow, related: &[Entity]) -> &mut Self {
        self.rev_replace_related::<ChildOf>(now, related)
    }

    fn rev_replace_children_with_difference(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self {
        self.rev_replace_related_with_difference::<ChildOf>(
            now,
            entities_to_unrelate,
            entities_to_relate,
            newly_related_entities,
        )
    }

    fn rev_replace_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        struct ReplaceRelated<R: Relationship> {
            entity: Entity,
            related: Box<[Entity]>,
            undo_at: usize,
            _p: PhantomData<R>,
        }

        impl<R: Relationship> UndoRedo for ReplaceRelated<R> {
            fn undo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related::<R>(&self.related[self.undo_at..]);
            }
            fn redo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related::<R>(&self.related[..self.undo_at]);
            }
        }

        if related.is_empty() {
            self.rev_remove::<R::RelationshipTarget>(now);
            return self;
        }

        let Some(existing_relations) = self.get::<R::RelationshipTarget>() else {
            return self.rev_add_related::<R>(now, related);
        };

        let mut removed_relations = EntityHashSet::from_iter(existing_relations.iter());
        let mut related = related
            .iter()
            .copied()
            .filter(|entity| !removed_relations.remove(*entity))
            .collect::<Vec<_>>();
        let undo_at = related.len();
        related.extend(removed_relations.into_iter());

        let entity = self.id();
        self.redo_and_buffer(
            now,
            ReplaceRelated {
                entity,
                related: related.into_boxed_slice(),
                undo_at,
                _p: PhantomData::<R>,
            },
        );

        self
    }

    fn rev_replace_related_with_difference<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self {
        struct ReplaceRelatedWithDifference<R: Relationship> {
            entity: Entity,
            related: Box<[Entity]>,        // [R, R, S, S, N, N]
            entities_to_unrelate: usize,   // ------^
            newly_related_entities: usize, // ------------^
            _p: PhantomData<R>,
        }

        impl<R: Relationship> UndoRedo for ReplaceRelatedWithDifference<R> {
            fn undo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related_with_difference::<R>(
                        &self.related[self.newly_related_entities..],
                        &self.related[self.entities_to_unrelate..],
                        &self.related[..self.entities_to_unrelate],
                    );
            }
            fn redo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related_with_difference::<R>(
                        &self.related[..self.entities_to_unrelate],
                        &self.related[self.entities_to_unrelate..],
                        &self.related[self.newly_related_entities..],
                    );
            }
        }

        if !self.contains::<R::RelationshipTarget>() {
            self.rev_add_related::<R>(now, entities_to_relate);
            return self;
        };

        let mut staying = EntityHashSet::from_iter(entities_to_relate.iter().copied());
        for &entity in newly_related_entities {
            staying.remove(entity);
        }

        let mut related = Vec::with_capacity(entities_to_unrelate.len() + entities_to_relate.len());
        related.extend_from_slice(entities_to_unrelate);
        related.extend(staying.into_iter());
        related.extend_from_slice(newly_related_entities);

        let entity = self.id();
        self.redo_and_buffer(
            now,
            ReplaceRelatedWithDifference {
                entity,
                related: related.into_boxed_slice(),
                entities_to_unrelate: entities_to_unrelate.len(),
                newly_related_entities: entities_to_unrelate.len() + entities_to_relate.len()
                    - newly_related_entities.len(),
                _p: PhantomData::<R>,
            },
        );

        self
    }

    fn rev_retain<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_bundle(
            now,
            |_| (),
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world| {
                let contributed_components: HashSet<_> = world
                    .register_bundle::<T>()
                    .contributed_components()
                    .iter()
                    .copied()
                    .collect();
                let components: Vec<_> = world
                    .archetypes()
                    .get(archetype_id)
                    .unwrap()
                    .components()
                    .filter(|component_id| !contributed_components.contains(component_id))
                    .collect();
                (BufferAt::Now, components)
            },
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        now: NonLogNow,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out> {
        self.take::<T>().map(|value| {
            let out = c(&value);
            let entity = self.id();
            self.buffer_undo_redo(
                now,
                Take {
                    bundle: Some(value),
                    entity,
                },
            );
            out
        })
    }

    fn rev_with_child(&mut self, now: NonLogNow, bundle: impl Bundle) -> &mut Self {
        self.rev_with_related::<ChildOf>(now, bundle)
    }

    fn rev_with_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle,
    ) -> &mut Self {
        let parent = self.id();
        self.world_scope(|world| {
            world.spawn((bundle, R::from(parent), DespawnAtUndo(now)));
        });
        self
    }
}

pub(super) fn insert_inner<'a, 'w, T: Bundle>(
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    now: NonLogNow,
    bundle: T,
    insert_mode: InsertMode,
) -> Result<&'a mut EntityWorldMut<'w>, RevEntityError> {
    let entity = entity_world_mut.id();
    let archetype_id = entity_world_mut.location().archetype_id;
    let marker = DisabledToDespawn::for_buffer(now.0);
    entity_world_mut.world_scope(|world| {
        buffer_pre_insert::<T>(
            world,
            now,
            entity,
            |world| {
                let mut entity_world_mut = world.entity_mut(entity);
                match insert_mode {
                    InsertMode::Replace => entity_world_mut.insert(bundle),
                    InsertMode::Keep => entity_world_mut.insert_if_new(bundle),
                };
            },
            archetype_id,
            InsertMode::Replace,
            marker,
        )
    })?;
    Ok(entity_world_mut)
}

fn rev_despawned_panic<Err: Error, Out>(entity: Entity) -> impl FnOnce(Err) -> Out {
    move |err| panic!("entity {entity} could not be mutated: {err}")
}

// todo: upstream public EntityWorldMut getter for vanilla types
pub enum RevComponentEntry<'w, 'a, T: Component> {
    /// An occupied entry.
    Occupied(RevOccupiedComponentEntry<'w, 'a, T>),
    /// A vacant entry.
    Vacant(RevVacantComponentEntry<'w, 'a, T>),
}

pub struct RevOccupiedComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

pub struct RevVacantComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

impl<'w, 'a, T: Component> RevComponentEntry<'w, 'a, T> {
    /// See [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    pub fn insert_entry(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.insert(component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.insert(component),
        }
    }

    /// Reversible version of [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    pub fn rev_insert_entry(
        self,
        now: NonLogNow,
        component: T,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.rev_insert(now, component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, component),
        }
    }

    /// See [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    pub fn or_insert(self, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Reversible version of [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    pub fn rev_or_insert(self, now: NonLogNow, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, default),
        }
    }

    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    pub fn or_insert_with<F: FnOnce() -> T>(
        self,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    pub fn rev_or_insert_with<F: FnOnce() -> T>(
        self,
        now: NonLogNow,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, default()),
        }
    }
}

impl<'w, 'a, T: Component + Default> RevComponentEntry<'w, 'a, T> {
    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    pub fn or_default(self) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(Default::default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    pub fn rev_or_default(self, now: NonLogNow) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, Default::default()),
        }
    }
}

impl<'w, 'a, T: Component> RevOccupiedComponentEntry<'w, 'a, T> {
    /// See [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    pub fn insert(&mut self, component: T) {
        self.entity_world_mut.insert(component);
    }

    /// Reversible version of [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    pub fn rev_insert(&mut self, now: NonLogNow, component: T) {
        self.entity_world_mut.rev_insert(now, component);
    }

    /// See [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    pub fn take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.take().unwrap()
    }

    /// Reversible version of [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    pub fn rev_take<Out>(self, now: NonLogNow, c: impl FnOnce(&T) -> Out) -> Out {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.rev_take::<T, Out>(now, c).unwrap()
    }
}

impl<'w, 'a, T: Component> RevVacantComponentEntry<'w, 'a, T> {
    /// See [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    pub fn insert(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.insert(component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }

    /// Reversible version of [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    pub fn rev_insert(self, now: NonLogNow, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.rev_insert(now, component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }
}
