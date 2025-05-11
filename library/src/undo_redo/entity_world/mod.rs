use std::{
    any::TypeId, borrow::BorrowMut, marker::PhantomData, ops::{Deref, DerefMut}
};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleFromComponents},
        component::{Component, ComponentId},
        entity::{Entity, EntityClonerBuilder},
        hierarchy::ChildOf,
        relationship::{
            OrderedRelationshipSourceCollection, Relationship, RelationshipSourceCollection,
            RelationshipTarget,
        },
    },
    ptr::OwningPtr,
};

use super::*;

#[cfg(test)]
mod test;

pub struct RevEntityWorldMut<'w, E: BorrowMut<EntityWorldMut<'w>>> {
    pub(super) entity_world_mut: E,
    pub(super) frame: u64,
    pub(super) _marker: PhantomData<&'w mut World>
}

impl<'w> TryFrom<EntityWorldMut<'w>> for RevEntityWorldMut<'w, EntityWorldMut<'w>> {
    type Error = RevMetaOrEntityError; // todo Error type without EntityNotFound
    fn try_from(entity_world_mut: EntityWorldMut<'w>) -> Result<Self, Self::Error> {
        let frame = non_log_frame(entity_world_mut.get_resource())?;
        match entity_world_mut.get::<DisabledToDespawn>() {
            Some(&marker) => Err(EntityRevDespawnedError {
                entity: entity_world_mut.id(),
                marker
            }.into()),
            None => Ok(Self {
                entity_world_mut,
                frame,
                _marker: PhantomData
            })
        }
    }
}

impl<'a, 'w> TryFrom<&'a mut EntityWorldMut<'w>> for RevEntityWorldMut<'w, &'a mut EntityWorldMut<'w>> {
    type Error = RevMetaOrEntityError; // todo Error type without EntityNotFound
    fn try_from(entity_world_mut: &'a mut EntityWorldMut<'w>) -> Result<Self, Self::Error> {
        let frame = non_log_frame(entity_world_mut.get_resource())?;
        match entity_world_mut.get::<DisabledToDespawn>() {
            Some(&marker) => Err(EntityRevDespawnedError {
                entity: entity_world_mut.id(),
                marker
            }.into()),
            None => Ok(Self {
                entity_world_mut,
                frame,
                _marker: PhantomData
            })
        }
    }
}

impl<'w, E: BorrowMut<EntityWorldMut<'w>>> Deref for RevEntityWorldMut<'w, E> {
    type Target = EntityWorldMut<'w>;
    fn deref(&self) -> &Self::Target {
        self.entity_world_mut.borrow()
    }
}

impl<'w, E: BorrowMut<EntityWorldMut<'w>>> DerefMut for RevEntityWorldMut<'w, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.entity_world_mut.borrow_mut()
    }
}

impl<'w, E: BorrowMut<EntityWorldMut<'w>>> RevEntityWorldMut<'w, E> {
    pub unsafe fn rev_world_mut(&mut self) -> RevWorld {
        let frame = self.frame;
        // SAFETY: todo
        let world = unsafe { self.world_mut() };
        RevWorld { world, frame }
    }

    pub fn rev_world_scope<U>(&mut self, f: impl FnOnce(RevWorld) -> U) -> U {
        let frame = self.frame;
        self.world_scope(|world| f(RevWorld { world, frame }))
    }

    pub fn buffer_components(
        &mut self,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, EntityRevDespawnedError> {
        let entity = self.id();
        let result = if at == BufferAt::Undo {
            unsafe {
                // SAFETY: No components of this entity are buffered now,
                // only resources are mutated and a bundle is registered.
                self.rev_world_mut()
                    .buffer_components(entity, at, components)
            }
        } else {
            self.rev_world_scope(|mut world| world.buffer_components(entity, at, components))
        };
        result.map_err(|err| match err {
            RevEntityError::EntityRevDespawnedError(err) => err,
            RevEntityError::EntityDoesNotExistError(_) => unreachable!("entity must exist")
        })
    }

    pub fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, EntityRevDespawnedError> {
        let entity = self.id();
        let result = self.rev_world_scope(|mut world| world.buffer_components_cached(entity, key, components));
        result.map_err(|err| match err {
            RevEntityError::EntityRevDespawnedError(err) => err,
            RevEntityError::EntityDoesNotExistError(_) => unreachable!("entity must exist")
        })
    }

    pub fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, EntityRevDespawnedError> {
        let result = self.rev_world_scope(|mut world| world.buffer_bundle(entity, at, bundle));
        result.map_err(|err| match err {
            RevEntityError::EntityRevDespawnedError(err) => err,
            RevEntityError::EntityDoesNotExistError(_) => unreachable!("entity must exist")
        })
    }

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`EntityWorldMut::add_child`].
    pub fn rev_add_child(&mut self, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(child)
    }

    /// Reversible version of [`EntityWorldMut::add_children`].
    pub fn rev_add_children(&mut self, children: &[Entity]) -> &mut Self {
        self.rev_add_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    pub fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self {
        self.rev_add_related::<R>(&[entity])
    }

    /// Reversible version of [`EntityWorldMut::add_related`].
    pub fn rev_add_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self {
        let id = self.id();
        self.rev_world_scope(|mut world| {
            for related in related {
                world.rev_entity_mut(*related).rev_insert(R::from(id));
            }
        });
        self
    }

    /// Reversible version of [`EntityWorldMut::clear`].
    pub fn rev_clear(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(unique_for_location!(archetype_id), |world| {
            let components: Vec<_> = world
                .archetypes()
                .get(archetype_id)
                .unwrap()
                .components()
                .collect();
            (BufferAt::Now, components)
        })
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    pub fn rev_clone_and_spawn(&mut self) -> Entity {
        let marker = DisabledToDespawn::for_spawn_despawn(self.frame);
        let entity = self.clone_and_spawn();
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with`].
    pub fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity {
        let marker = DisabledToDespawn::for_spawn_despawn(self.frame);
        let entity = self.clone_and_spawn_with(config);
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    /// Reversible version of [`EntityWorldMut::clone_components`].
    pub fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        // if the target entity does not exist, let `clone_components` panic here
        if let Some(location) = self.world().entities().get(target) {
            let archetype_id = location.archetype_id;
            self.rev_world_scope(|mut world| {
                let _ok = world.buffer_components_cached(
                    target,
                    unique_for_location!(archetype_id, TypeId::of::<B>()),
                    |world| {
                        let bundle_id = world.register_bundle::<B>().id();
                        pre_insert_maybe_overwrite(&world, bundle_id, archetype_id)
                    },
                );
            });
        }
        self.clone_components::<B>(target);
        self
    }

    // rev_clone_with
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    pub fn rev_despawn(self) {
        let entity = self.id();
        rev_despawn_inner(self).unwrap_or_else(|err| {
            panic!("entity {entity} could not be reversibly despawned: {err}")
        });
    }

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    pub fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self {
        if let Some(sources) = self.get::<S>() {
            let sources: Vec<_> = sources.iter().collect();
            self.rev_world_scope(|mut world| {
                for entity in sources.into_iter() {
                    if let Ok(entity_mut) = world.rev_get_entity_mut(entity) {
                        entity_mut.rev_despawn();
                    }
                }
            });
        }
        self
    }

    /// Reversible version of [`EntityWorldMut::entry`].
    pub fn rev_entry<'a, T: Component>(&'a mut self) -> RevEntry<'w, 'a, T, E> {
        if self.contains::<T>() {
            RevEntry::Occupied(RevOccupiedEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        } else {
            RevEntry::Vacant(RevVacantEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        }
    }

    /// Reversible version of [`EntityWorldMut::insert`].
    pub fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        let entity = self.id();
        insert_inner(self, bundle, InsertMode::Replace).unwrap_or_else(rev_despawned_panic(entity))
    }

    /// Reversible version of [`EntityWorldMut::insert_by_id`].
    ///
    /// # Safety
    ///
    /// - [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    pub unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        unsafe {
            // SAFETY: todo
            self.rev_insert_by_ids(&[component_id], [component].into_iter())
        }
    }

    /// Reversible version of [`EntityWorldMut::insert_by_ids`].
    ///
    /// # Safety
    ///
    /// - Each [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - Each [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    pub unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let bundle_id = unsafe {
            // SAFETY: registering a bundle does not change the entity's location
            self.world_mut().register_dynamic_bundle(component_ids).id()
        };
        let entity = self.id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, bundle_id),
            |world: &mut World| pre_insert_maybe_overwrite(world, bundle_id, archetype_id),
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        unsafe {
            // SAFETY: todo
            self.insert_by_ids(component_ids, iter_components);
        }
        self
    }

    /// Reversible version of [`EntityWorldMut::insert_children`].
    pub fn rev_insert_children(&mut self, index: usize, children: &[Entity]) -> &mut Self {
        self.rev_insert_related::<ChildOf>(index, children)
    }

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    pub fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        let entity = self.id();
        insert_inner(self, bundle, InsertMode::Keep).unwrap_or_else(rev_despawned_panic(entity))
    }

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    pub fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self {
        self.rev_insert(bundle.clone());
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.rev_world_scope(|mut world| {
                    world
                        .rev_entity_mut(related)
                        .rev_insert_recursive::<S>(bundle.clone());
                });
            }
        }

        self
    }

    // rev_insert_reflect
    // out of scope due complexity

    // rev_insert_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::insert_related`].
    pub fn rev_insert_related<R: Relationship>(
        &mut self,
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
                        world.buffer_undo_redo(InsertExistingRelated::<R> {
                            id,
                            related,
                            index,
                            old_index,
                            _marker: PhantomData,
                        });
                    }
                } else {
                    world.entity_mut(related).insert(R::from(id));
                    let mut target = world
                        .get_mut::<R::RelationshipTarget>(id)
                        .expect("hooks should have added relationship target");
                    let collection = target.collection_mut_risky();
                    index = index.min(collection.len());
                    collection.place_most_recent(index);
                    world.buffer_undo_redo(InsertNewRelated::<R> {
                        id,
                        related,
                        index,
                        _marker: PhantomData,
                    });
                }
            }
        });

        self
    }

    // rev_insert_with_relationship_hook_mode
    // out of scope due complexity

    // rev_is_despawned
    // implemented via RevIsDespawned trait

    /// Reversible version of [`EntityWorldMut::move_components`].
    pub fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        // todo: when moving no longer requires Clone, replace this logic with non-Clone approach
        self.rev_clone_components::<B>(target).rev_remove::<B>()
    }

    /// Reversible version of [`EntityWorldMut::remove`].
    pub fn rev_remove<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(
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

    /// Reversible version of [`EntityWorldMut::remove_by_id`].
    pub fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        self.rev_remove_by_ids(&[component_id])
    }

    /// Reversible version of [`EntityWorldMut::remove_by_ids`].
    pub fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        let archetype = self.archetype();
        let components: Vec<_> = component_ids
            .iter()
            .copied()
            .filter(|component_id| archetype.contains(*component_id))
            .collect();
        // must be Ok because self.archetype() did not panic
        let _ok = self.buffer_components(BufferAt::Now, &components);
        self
    }

    /// Reversible version of [`EntityWorldMut::remove_children`].
    pub fn rev_remove_children(&mut self, children: &[Entity]) -> &mut Self {
        self.rev_remove_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    pub fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self {
        self.rev_remove::<B>();
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.rev_world_scope(|mut world| {
                    world.rev_entity_mut(related).rev_remove_recursive::<S, B>();
                });
            }
        }

        self
    }

    // rev_remove_reflect
    // out of scope due complexity

    // rev_remove_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::remove_related`].
    pub fn rev_remove_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self {
        let id = self.id();
        self.rev_world_scope(|mut world| {
            for related in related {
                if world
                    .get::<R>(*related)
                    .is_some_and(|relationship| relationship.get() == id)
                {
                    world.rev_entity_mut(*related).rev_remove::<R>();
                }
            }
        });

        self
    }

    /// Reversible version of [`EntityWorldMut::remove_with_requires`].
    pub fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(
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

    // rev_replace_children
    // out of scope due complexity

    // rev_replace_children_with_difference
    // out of scope due complexity

    // rev_replace_related
    // out of scope due complexity

    // rev_replace_related_with_difference
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::retain`].
    pub fn rev_retain<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(
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

    /// Reversible version of [`EntityWorldMut::take`].
    pub fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out> {
        self.take::<T>().map(|value| {
            let out = c(&value);
            let entity = self.id();
            self.buffer_undo_redo(Take {
                bundle: Some(value),
                entity,
            });
            out
        })
    }

    /// Reversible version of [`EntityWorldMut::with_child`].
    pub fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self {
        self.rev_with_related::<ChildOf>(bundle)
    }

    /// Reversible version of [`EntityWorldMut::with_children`].
    pub fn rev_with_children(&mut self, func: impl FnOnce(&mut RevRelatedSpawner<ChildOf>)) -> &mut Self {
        self.rev_with_related_entities::<ChildOf>(func)
    }

    /// Reversible version of [`EntityWorldMut::with_related`].
    pub fn rev_with_related<R: Relationship>(&mut self, bundle: impl Bundle) -> &mut Self {
        let parent = self.id();
        self.world_scope(|world| {
            world.spawn((bundle, R::from(parent), DespawnAtUndo));
        });
        self
    }

    /// Reversible version of [`EntityWorldMut::with_related_entities`].
    pub fn rev_with_related_entities<R: Relationship>(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawner<R>),
    ) -> &mut Self {
        todo!()
    }
}

/*
pub trait RevEntityWorldMut<'w> {
    // buffer methods

    fn buffer_components_in_progress(&self) -> Option<BufferInProgress>;

    fn buffer_components(
        &mut self,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevMetaOrEntityError>;

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevMetaOrEntityError>;

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevMetaOrEntityError>;

    // todo: fallible with_related etc

    fn rev_try_with_related<R: Relationship>(
        &mut self,
        bundle: impl Bundle,
    ) -> Result<&mut Self, RevMetaNotLogError>;

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`EntityWorldMut::add_child`].
    fn rev_add_child(&mut self, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(child)
    }

    /// Reversible version of [`EntityWorldMut::add_children`].
    fn rev_add_children(&mut self, children: &[Entity]) -> &mut Self {
        self.rev_add_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self {
        self.rev_add_related::<R>(&[entity])
    }

    /// Reversible version of [`EntityWorldMut::add_related`].
    fn rev_add_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear`].
    fn rev_clear(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    fn rev_clone_and_spawn(&mut self) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with`].
    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_components`].
    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;

    // rev_clone_with
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    fn rev_despawn(self);

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::entry`].
    fn rev_entry<'a, T: Component>(&'a mut self) -> RevEntry<'w, 'a, T>;

    /// Reversible version of [`EntityWorldMut::insert`].
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self;

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
    ) -> &mut Self {
        unsafe {
            // SAFETY: todo
            self.rev_insert_by_ids(&[component_id], [component].into_iter())
        }
    }

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

    /// Reversible version of [`EntityWorldMut::insert_children`].
    fn rev_insert_children(&mut self, index: usize, children: &[Entity]) -> &mut Self {
        self.rev_insert_related::<ChildOf>(index, children)
    }

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self;

    // rev_insert_reflect
    // out of scope due complexity

    // rev_insert_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::insert_related`].
    fn rev_insert_related<R: Relationship>(
        &mut self,
        index: usize,
        related: &[Entity],
    ) -> &mut Self
    where
        <R::RelationshipTarget as RelationshipTarget>::Collection:
            OrderedRelationshipSourceCollection;

    // rev_insert_with_relationship_hook_mode
    // out of scope due complexity

    // rev_is_despawned
    // implemented via RevIsDespawned trait

    /// Reversible version of [`EntityWorldMut::move_components`].
    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        // todo: when moving no longer requires Clone, replace this logic with non-Clone approach
        self.rev_clone_components::<B>(target).rev_remove::<B>()
    }

    /// Reversible version of [`EntityWorldMut::remove`].
    fn rev_remove<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_id`].
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        self.rev_remove_by_ids(&[component_id])
    }

    /// Reversible version of [`EntityWorldMut::remove_by_ids`].
    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_children`].
    fn rev_remove_children(&mut self, children: &[Entity]) -> &mut Self {
        self.rev_remove_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self;

    // rev_remove_reflect
    // out of scope due complexity

    // rev_remove_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::remove_related`].
    fn rev_remove_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_with_requires`].
    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self;

    // rev_replace_children
    // out of scope due complexity

    // rev_replace_children_with_difference
    // out of scope due complexity

    // rev_replace_related
    // out of scope due complexity

    // rev_replace_related_with_difference
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::retain`].
    fn rev_retain<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::take`].
    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out>;

    /// Reversible version of [`EntityWorldMut::with_child`].
    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self {
        self.rev_with_related::<ChildOf>(bundle)
    }

    /// Reversible version of [`EntityWorldMut::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        bundle: impl Bundle,
    ) -> &mut Self {
        self.rev_try_with_related::<R>(bundle).unwrap_or_else(|err| panic!("{err}"))
    }
}

impl<'w> RevEntityWorldMut<'w> for EntityWorldMut<'w> {
    fn buffer_components_in_progress(&self) -> Option<BufferInProgress> {
        self.world().buffer_components_in_progress()
    }

    fn buffer_components(
        &mut self,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, RevMetaOrEntityError> {
        let entity = self.id();
        if at == BufferAt::Undo {
            unsafe {
                // SAFETY: No components of this entity are buffered now,
                // only resources are mutated and a bundle is registered.
                self.world_mut().buffer_components(entity, at, components)
            }
        } else {
            self.world_scope(|world| world.buffer_components(entity, at, components))
        }
    }

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, RevMetaOrEntityError> {
        let entity = self.id();
        self.world_scope(|world| world.buffer_components_cached(entity, key, components))
    }

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, RevMetaOrEntityError> {
        self.world_scope(|world| world.buffer_bundle(entity, at, bundle))
    }

    #[track_caller]
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        insert_inner(self, bundle, InsertMode::Replace).unwrap()
    }

    #[track_caller]
    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        insert_inner(self, bundle, InsertMode::Keep).unwrap()
    }

    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let bundle_id = unsafe {
            // SAFETY: registering a bundle does not change the entity's location
            self.world_mut().register_dynamic_bundle(component_ids).id()
        };
        let entity = self.id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, bundle_id),
            |world: &mut World| pre_insert_maybe_overwrite(world, bundle_id, archetype_id),
        )
        .unwrap_or_else(rev_despawned_panic(entity));
        unsafe {
            // SAFETY: todo
            self.insert_by_ids(component_ids, iter_components)
        }
    }

    fn rev_remove<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(
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

    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(
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

    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out> {
        self.take::<T>().map(|value| {
            let out = c(&value);
            let entity = self.id();
            self.buffer_undo_redo(Take {
                bundle: Some(value),
                entity,
            });
            out
        })
    }

    fn rev_retain<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(
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

    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        let archetype = self.archetype();
        let components: Vec<_> = component_ids
            .iter()
            .copied()
            .filter(|component_id| archetype.contains(*component_id))
            .collect();
        let _ok = self.buffer_components(BufferAt::Now, &components);
        self
    }

    fn rev_clear(&mut self) -> &mut Self {
        let archetype_id = self.location().archetype_id;
        let entity = self.id();
        self.buffer_components_cached(unique_for_location!(archetype_id), |world| {
            let components: Vec<_> = world
                .archetypes()
                .get(archetype_id)
                .unwrap()
                .components()
                .collect();
            (BufferAt::Now, components)
        })
        .unwrap_or_else(rev_despawned_panic(entity));
        self
    }

    fn rev_despawn(self) {
        let entity = self.id();
        rev_despawn_inner(self).unwrap_or_else(|err| {
            panic!("entity {entity} could not be reversibly despawned: {err}")
        });
    }

    fn rev_clone_and_spawn(&mut self) -> Entity {
        let marker = DisabledToDespawn::for_spawn_despawn(self.get_resource::<RevMeta>())
            .unwrap_or_else(|err| panic!("{err}"));
        let entity = self.clone_and_spawn();
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity {
        let marker = DisabledToDespawn::for_spawn_despawn(self.get_resource::<RevMeta>())
            .unwrap_or_else(|err| panic!("{err}"));
        let entity = self.clone_and_spawn_with(config);
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        let archetype_id = self
            .world()
            .entities()
            .get(target)
            .expect("todo")
            .archetype_id;
        self.world_scope(|world| {
            let _ok = world.buffer_components_cached(
                target,
                unique_for_location!(archetype_id, TypeId::of::<B>()),
                |world| {
                    let bundle_id = world.register_bundle::<B>().id();
                    pre_insert_maybe_overwrite(&world, bundle_id, archetype_id)
                },
            );
        });
        self.clone_components::<B>(target)
    }

    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        // todo: when moving no longer requires Clone, replace this logic with non-Clone approach
        self.rev_clone_components::<B>(target).rev_remove::<B>()
    }

    fn rev_try_with_related<R: Relationship>(
            &mut self,
            bundle: impl Bundle,
        ) -> Result<&mut Self, RevMetaNotLogError> {
        let marker = DisabledToDespawn::for_spawn_despawn(self.get_resource())?;
        let parent = self.id();
        self.world_scope(|world| {
            world.spawn((bundle, R::from(parent), PrecomputedDespawnAtUndo(marker)));
        });
        Ok(self)
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

    fn rev_insert_related<R>(&mut self, index: usize, related: &[Entity]) -> &mut EntityWorldMut<'w>
    where
        R: Relationship,
        <<R as Relationship>::RelationshipTarget as RelationshipTarget>::Collection:
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
                        world.buffer_undo_redo(InsertExistingRelated::<R> {
                            id,
                            related,
                            index,
                            old_index,
                            _marker: PhantomData,
                        });
                    }
                } else {
                    world.entity_mut(related).insert(R::from(id));
                    let mut target = world
                        .get_mut::<R::RelationshipTarget>(id)
                        .expect("hooks should have added relationship target");
                    let collection = target.collection_mut_risky();
                    index = index.min(collection.len());
                    collection.place_most_recent(index);
                    world.buffer_undo_redo(InsertNewRelated::<R> {
                        id,
                        related,
                        index,
                        _marker: PhantomData,
                    });
                }
            }
        });

        self
    }

    fn rev_remove_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self {
        let id = self.id();
        self.world_scope(|world| {
            for related in related {
                if world
                    .get::<R>(*related)
                    .is_some_and(|relationship| relationship.get() == id)
                {
                    world.entity_mut(*related).rev_remove::<R>();
                }
            }
        });

        self
    }

    fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self {
        if let Some(sources) = self.get::<S>() {
            let sources: Vec<_> = sources.iter().collect();
            self.world_scope(|world| {
                for entity in sources.into_iter() {
                    if let Ok(entity_mut) = world.get_entity_mut(entity) {
                        entity_mut.rev_despawn();
                    }
                }
            });
        }
        self
    }

    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self {
        self.rev_insert(bundle.clone());
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.world_scope(|world| {
                    world
                        .entity_mut(related)
                        .rev_insert_recursive::<S>(bundle.clone());
                });
            }
        }

        self
    }

    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self {
        self.rev_remove::<B>();
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.world_scope(|world| {
                    world.entity_mut(related).rev_remove_recursive::<S, B>();
                });
            }
        }

        self
    }

    fn rev_entry<'a, T: Component>(&'a mut self) -> RevEntry<'w, 'a, T> {
        // todo: try to wrap type that check for changed component on drop

        if self.contains::<T>() {
            RevEntry::Occupied(RevOccupiedEntry {
                entity_world: self,
                _marker: PhantomData,
            })
        } else {
            RevEntry::Vacant(RevVacantEntry {
                entity_world: self,
                _marker: PhantomData,
            })
        }
    }
}
*/

#[track_caller]
fn insert_inner<'a, 'w, T: Bundle, E: BorrowMut<EntityWorldMut<'w>>>(
    entity_world_mut: &'a mut RevEntityWorldMut<'w, E>,
    bundle: T,
    insert_mode: InsertMode,
) -> Result<&'a mut RevEntityWorldMut<'w, E>, RevEntityError> {
    let entity = entity_world_mut.id();
    let archetype_id = entity_world_mut.location().archetype_id;
    let marker = DisabledToDespawn::for_buffer(entity_world_mut.frame);
    entity_world_mut.world_scope(|world| {
        pre_insert::<T>(world, entity, archetype_id, InsertMode::Replace, marker)
    })?;
    match insert_mode {
        InsertMode::Replace => entity_world_mut.insert(bundle),
        InsertMode::Keep => entity_world_mut.insert_if_new(bundle),
    };
    Ok(entity_world_mut)
}

fn rev_despawned_panic<Err: Error, Out>(entity: Entity) -> impl FnOnce(Err) -> Out {
    move |err| panic!("entity {entity} could not be mutated: {err}")
}

pub struct RevRelatedSpawner<'w, R: Relationship> {
    target: Entity,
    world: RevWorld<'w>,
    _marker: PhantomData<R>,
}

impl<'w, R: Relationship> RevRelatedSpawner<'w, R> {
    /// See [`RelatedSpawner::new`](bevy::ecs::relationship::RelatedSpawner::new).
    pub fn new(world: RevWorld<'w>, target: Entity) -> Self {
        Self {
            world,
            target,
            _marker: PhantomData,
        }
    }

    /// See [`RelatedSpawner::spawn`](bevy::ecs::relationship::RelatedSpawner::spawn).
    pub fn spawn(&mut self, bundle: impl Bundle) -> EntityWorldMut<'_> {
        self.world.spawn((R::from(self.target), bundle))
    }

    /// Reversible version of [`RelatedSpawner::spawn`](bevy::ecs::relationship::RelatedSpawner::spawn).
    pub fn rev_spawn(&mut self, bundle: impl Bundle) -> RevEntityWorldMut<EntityWorldMut> {
        let mut entity_mut = self.world.rev_spawn((R::from(self.target), bundle));
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(InsertRelationship::<R, _> {
            entity,
            target: [self.target],
            _marker: PhantomData,
        });
        entity_mut
    }

    /// See [`RelatedSpawner::spawn_empty`](bevy::ecs::relationship::RelatedSpawner::spawn_empty).
    pub fn spawn_empty(&mut self) -> RevEntityWorldMut<EntityWorldMut> {
        self.world.spawn(R::from(self.target))
    }

    /// Reversible version of [`RelatedSpawner::spawn_empty`](bevy::ecs::relationship::RelatedSpawner::spawn_empty).
    pub fn rev_spawn_empty(&mut self) -> RevEntityWorldMut<EntityWorldMut> {
        // todo: spawn empty variant without empty tuple bundle
        self.rev_spawn(())
    }

    /// See [`RelatedSpawner::target_entity`](bevy::ecs::relationship::RelatedSpawner::target_entity).
    pub fn target_entity(&self) -> Entity {
        self.target
    }
}

// todo: upstream public EntityWorldMut getter for vanilla types
pub enum RevEntry<'w, 'a, T: Component, E: BorrowMut<EntityWorldMut<'w>>> {
    /// An occupied entry.
    Occupied(RevOccupiedEntry<'w, 'a, T, E>),
    /// A vacant entry.
    Vacant(RevVacantEntry<'w, 'a, T, E>),
}

pub struct RevOccupiedEntry<'w, 'a, T, E: BorrowMut<EntityWorldMut<'w>>> {
    entity_world_mut: &'a mut RevEntityWorldMut<'w, E>,
    _marker: PhantomData<T>,
}

pub struct RevVacantEntry<'w, 'a, T, E: BorrowMut<EntityWorldMut<'w>>> {
    entity_world_mut: &'a mut RevEntityWorldMut<'w, E>,
    _marker: PhantomData<T>,
}

impl<'w, 'a, T: Component, E: BorrowMut<EntityWorldMut<'w>>> RevEntry<'w, 'a, T, E> {
    /// See [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    pub fn insert_entry(self, component: T) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(mut entry) => {
                entry.insert(component);
                entry
            }
            RevEntry::Vacant(entry) => entry.insert(component),
        }
    }

    /// Reversible version of [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    pub fn rev_insert_entry(self, component: T) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(mut entry) => {
                entry.rev_insert(component);
                entry
            }
            RevEntry::Vacant(entry) => entry.rev_insert(component),
        }
    }

    /// See [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    pub fn or_insert(self, default: T) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Reversible version of [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    pub fn rev_or_insert(self, default: T) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.rev_insert(default),
        }
    }

    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    pub fn or_insert_with<F: FnOnce() -> T>(self, default: F) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    pub fn rev_or_insert_with<F: FnOnce() -> T>(self, default: F) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.rev_insert(default()),
        }
    }
}

impl<'w, 'a, T: Component + Default, E: BorrowMut<EntityWorldMut<'w>>> RevEntry<'w, 'a, T, E> {
    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    pub fn or_default(self) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.insert(Default::default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    pub fn rev_or_default(self) -> RevOccupiedEntry<'w, 'a, T, E> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.rev_insert(Default::default()),
        }
    }
}

impl<'w, 'a, T: Component, E: BorrowMut<EntityWorldMut<'w>>> RevOccupiedEntry<'w, 'a, T, E> {
    /// See [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    pub fn insert(&mut self, component: T) {
        self.entity_world_mut.insert(component);
    }

    /// Reversible version of [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    pub fn rev_insert(&mut self, component: T) {
        self.entity_world_mut.rev_insert(component);
    }

    /// See [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    pub fn take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.take().unwrap()
    }

    /// Reversible version of [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    pub fn rev_take<Out>(self, c: impl FnOnce(&T) -> Out) -> Out {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.rev_take::<T, Out>(c).unwrap()
    }
}

impl<'w, 'a, T: Component, E: BorrowMut<EntityWorldMut<'w>>> RevVacantEntry<'w, 'a, T, E> {
    /// See [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    pub fn insert(self, component: T) -> RevOccupiedEntry<'w, 'a, T, E> {
        self.entity_world_mut.insert(component);
        RevOccupiedEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }

    /// Reversible version of [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    pub fn rev_insert(self, component: T) -> RevOccupiedEntry<'w, 'a, T, E> {
        self.entity_world_mut.rev_insert(component);
        RevOccupiedEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }
}
