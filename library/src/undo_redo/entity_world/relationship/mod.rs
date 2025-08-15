use std::{any::TypeId, marker::PhantomData};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, BundleFromComponents, BundleId, InsertMode, NoBundleEffect},
        change_detection::MaybeLocation,
        component::{Component, ComponentId},
        entity::{Entity, EntityCloner, EntityClonerBuilder, EntityHashSet, OptIn, OptOut},
        hierarchy::{ChildOf, Children},
        relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
        resource::Resource,
        world::{error::EntityMutableFetchError, ComponentEntry, EntityRef, EntityWorldMut, World},
    },
    platform::collections::{HashMap, HashSet},
};

use crate::{
    meta::{NonLogNow, RevDirection},
    undo_redo::{
        assert_not_rev_despawned, rev_spawn_with_caller, rev_try_remove_with_caller, EntityRevDespawnedError, RevDespawned, RevEntityError, RevEntityWorldMut, RevIsDespawned, RevOpInProgress
    },
};

use super::{
    BuffersUndoRedo, RevDespawnCleaner, RevWorld, Spawn, Take, UndoRedo, UndoRedoSwap,
    rev_spawn_finish,
};

/* 
#[cfg(test)]
mod test;
*/

pub trait RevEntityWorldMutRelationship<'w> {
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

    /// Reversible version of [`EntityWorldMut::clear_children`].
    fn rev_clear_children(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear_related`].
    fn rev_clear_related<R: Relationship>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn_children`].
    fn rev_despawn_children(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self;

    // rev_insert_children
    // out of scope
    // todo: reevaluate

    fn rev_insert_parent(
        &mut self,
        now: NonLogNow,
        target: Entity
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle<Effect: NoBundleEffect> + Clone,
    ) -> &mut Self;

    // rev_insert_related
    // out of scope
    // todo: reevaluate

    fn rev_insert_target<R: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        target: Entity
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_children`].
    fn rev_remove_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self;

    fn remove_parent(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(
        &mut self,
        now: NonLogNow,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_related`].
    fn rev_remove_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self;

    fn remove_target<R: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self;

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

    /// Reversible version of [`EntityWorldMut::with_child`].
    fn rev_with_child(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle<Effect: NoBundleEffect>,
    ) -> &mut Self;

    // rev_with_children
    // implemented via DespawnAtUndo
    // todo: reevaluate

    /// Reversible version of [`EntityWorldMut::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle<Effect: NoBundleEffect>,
    ) -> &mut Self;

    // rev_with_related_entities
    // implemented via DespawnAtUndo
    // todo: reevaluate
}

impl<'w> RevEntityWorldMutRelationship<'w> for EntityWorldMut<'w> {
    #[track_caller]
    fn rev_add_child(&mut self, now: NonLogNow, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(now, child)
    }

    #[track_caller]
    fn rev_add_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self {
        self.rev_add_related::<ChildOf>(now, children)
    }

    #[track_caller]
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entity: Entity,
    ) -> &mut Self {
        self.rev_add_related::<R>(now, &[entity])
    }

    #[track_caller]
    fn rev_add_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        rev_try_add_related_with_caller::<R>(self, now, related, MaybeLocation::caller()).unwrap()
    }

    #[track_caller]
    fn rev_clear_children(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_clear_related::<ChildOf>(now)
    }

    #[track_caller]
    fn rev_clear_related<R: Relationship>(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_replace_related::<R>(now, &[])
    }

    #[track_caller]
    fn rev_despawn_children(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_despawn_related::<Children>(now)
    }

    #[track_caller]
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self {
        rev_try_despawn_related_with_caller::<S>(self, now, MaybeLocation::caller()).unwrap()
    }

    #[track_caller]
    fn rev_insert_parent(
        &mut self,
        now: NonLogNow,
        target: Entity
    ) -> &mut Self {
        self.rev_insert_target::<Children>(now, target)
    }

    #[track_caller]
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle<Effect: NoBundleEffect> + Clone,
    ) -> &mut Self {
        rev_try_insert_recursive_with_caller::<S>(self, now, bundle, MaybeLocation::caller())
            .unwrap()
    }

    #[track_caller]
    fn rev_insert_target<R: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        target: Entity
    ) -> &mut Self {
        todo!()
    }

    #[track_caller]
    fn rev_remove_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self {
        self.rev_remove_related::<ChildOf>(now, children)
    }

    #[track_caller]
    fn remove_parent(&mut self, now: NonLogNow) -> &mut Self {
        self.remove_target::<Children>(now)
    }

    #[track_caller]
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(
        &mut self,
        now: NonLogNow,
    ) -> &mut Self {
        rev_try_remove_recursive_with_caller::<S, B>(self, now, MaybeLocation::caller()).unwrap()
    }

    #[track_caller]
    fn rev_remove_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        rev_try_remove_related_with_caller::<R>(self, now, related, MaybeLocation::caller())
            .unwrap()
    }

    #[track_caller]
    fn remove_target<R: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self {
        rev_try_remove_target_with_caller::<R>(self, now, MaybeLocation::caller()).unwrap()
    }

    #[track_caller]
    fn rev_replace_children(&mut self, now: NonLogNow, related: &[Entity]) -> &mut Self {
        self.rev_replace_related::<ChildOf>(now, related)
    }

    #[track_caller]
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

    #[track_caller]
    fn rev_replace_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        rev_try_replace_related_with_caller::<R>(self, now, related, MaybeLocation::caller())
            .unwrap()
    }

    #[track_caller]
    fn rev_replace_related_with_difference<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self {
        rev_try_replace_related_with_difference_with_caller::<R>(
            self,
            now,
            entities_to_unrelate,
            entities_to_relate,
            newly_related_entities,
            MaybeLocation::caller(),
        )
        .unwrap()
    }

    #[track_caller]
    fn rev_with_child(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle<Effect: NoBundleEffect>,
    ) -> &mut Self {
        self.rev_with_related::<ChildOf>(now, bundle)
    }

    #[track_caller]
    fn rev_with_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle<Effect: NoBundleEffect>,
    ) -> &mut Self {
        rev_try_with_related_with_caller::<R>(self, now, bundle, MaybeLocation::caller()).unwrap()
    }
}

pub(crate) fn rev_try_add_related_with_caller<'a, 'b, R: Relationship>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    related: &[Entity],
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, RevEntityError> {
    assert_not_rev_despawned(&*entity_mut)?;
    let target = entity_mut.id();
    entity_mut.world_scope(|world| {
        let related = related.into_iter()
            .flat_map(|&entity| {
                match world.get_entity_mut(entity) {
                    Ok(mut entity_mut) => match assert_not_rev_despawned(&entity_mut) {
                        Ok(_) => match entity_mut.entry::<R>() {
                            ComponentEntry::Vacant(vacant) => {
                                vacant.insert(R::from(target));
                                Some(Ok(entity))
                            },
                            ComponentEntry::Occupied(occupied) => {
                                if occupied.get().get() != target {
                                    rev_remove_target_with_caller::<R::RelationshipTarget>(&mut entity_mut, now, caller);
                                    Some(Ok(entity))
                                } else {
                                    None
                                }
                            }
                        },
                        Err(err) => Some(Err(RevEntityError::from(err))),
                    },
                    Err(EntityMutableFetchError::EntityDoesNotExist(err)) => Some(Err(RevEntityError::from(err))),
                    Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!() // get_entity_mut called per entity
                }
            })
            .collect::<Result<Box<[_]>, _>>()?;
        world.buffer_undo_redo(now, AddRelated {
            target,
            related,
            caller,
            _marker: PhantomData::<R>
        });
        Ok(())
    }).map(|()| entity_mut)
}

pub(crate) fn rev_try_insert_recursive_with_caller<'a, 'b, S: RelationshipTarget>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    bundle: impl Bundle<Effect: NoBundleEffect> + Clone,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    // todo, anders umsetzen
    // rev_try_insert_with_caller(entity_mut, bundle.clone(), InsertMode::Replace, now, caller)?;
    if let Some(relationship_target) = entity_mut.get::<S>() {
        let related_vec: Vec<Entity> = relationship_target.iter().collect();
        for related in related_vec {
            entity_mut.world_scope(|world| {
                let mut related = world.entity_mut(related);
                rev_try_insert_recursive_with_caller::<S>(&mut related, now, bundle.clone(), caller)
                    .map(|_| ())
            })?;
        }
    }
    Ok(entity_mut)
}

pub(crate) fn rev_try_despawn_related_with_caller<'a, 'b, S: RelationshipTarget>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    if let Some(sources) = entity_mut.get::<S>() {
        let sources: Vec<_> = sources.iter().collect();
        entity_mut.world_scope(|world| {
            for entity in sources.into_iter() {
                if let Ok(mut related) = world.get_entity_mut(entity) {
                    if !related.is_rev_despawned() {
                        related.redo_and_buffer(
                            now,
                            UndoRedoSwap(Spawn {
                                entity,
                                location: caller,
                            }),
                        );
                        related
                            .resource_mut::<RevDespawnCleaner>()
                            .log_despawn(entity, caller, now);
                    }
                }
            }
        });
    }
    Ok(entity_mut)
}

pub(crate) fn rev_try_remove_recursive_with_caller<'a, 'b, S: RelationshipTarget, B: Bundle>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    rev_try_remove_with_caller::<B, false>(entity_mut, now, caller)?;
    if let Some(relationship_target) = entity_mut.get::<S>() {
        let related_vec: Vec<Entity> = relationship_target.iter().collect();
        for related in related_vec {
            entity_mut.world_scope(|world| {
                let mut related = world.entity_mut(related);
                rev_try_remove_recursive_with_caller::<S, B>(&mut related, now, caller).map(|_| ())
            })?;
        }
    }
    Ok(entity_mut)
}

pub(crate) fn rev_try_remove_related_with_caller<'a, 'b, R: Relationship>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    related: &[Entity],
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    let id = entity_mut.id();
    entity_mut.world_scope(|world| {
        for related in related {
            if world
                .get::<R>(*related)
                .is_some_and(|relationship| relationship.get() == id)
            {
                let mut related = world.entity_mut(*related);
                rev_try_remove_with_caller::<R, false>(&mut related, now, caller)?;
            }
        }
        Result::<_, EntityRevDespawnedError>::Ok(())
    })?;
    Ok(entity_mut)
}

pub(crate) fn rev_try_replace_related_with_caller<'a, 'b, R: Relationship>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    related: &[Entity],
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, RevEntityError> {

    assert_not_rev_despawned(&*entity_mut)?;

    if related.is_empty() {
        if entity_mut.contains::<R::RelationshipTarget>() {

        }
        // todo: anders umsetzen
        rev_try_remove_with_caller::<R::RelationshipTarget, false>(entity_mut, now, caller)?;
        return Ok(entity_mut);
    }

    let Some(existing_relations) = entity_mut.get::<R::RelationshipTarget>() else {
        return rev_try_add_related_with_caller::<R>(entity_mut, now, related, caller);
    };

    let mut removed_relations = EntityHashSet::from_iter(existing_relations.iter());
    let mut related = related
        .iter()
        .copied()
        .filter(|entity| !removed_relations.remove(*entity))
        .collect::<Vec<_>>();
    let undo_at = related.len();
    related.extend(removed_relations.into_iter());

    let entity = entity_mut.id();
    entity_mut.redo_and_buffer(
        now,
        ReplaceRelated {
            entity,
            related: related.into_boxed_slice(),
            undo_at,
            _p: PhantomData::<R>,
        },
    );

    Ok(entity_mut)
}

pub(crate) fn rev_try_remove_target_with_caller<'a, 'b, R: RelationshipTarget>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;
    Ok(rev_remove_target_with_caller::<R>(entity_mut, now, caller))
}

fn rev_remove_target_with_caller<'a, 'b, R: RelationshipTarget>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    caller: MaybeLocation,
) -> &'a mut EntityWorldMut<'b> {
    todo!()
}

pub(crate) fn rev_try_replace_related_with_difference_with_caller<'a, 'b, R: Relationship>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    entities_to_unrelate: &[Entity],
    entities_to_relate: &[Entity],
    newly_related_entities: &[Entity],
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, RevEntityError> {
    assert_not_rev_despawned(&*entity_mut)?;

    if !entity_mut.contains::<R::RelationshipTarget>() {
        return rev_try_add_related_with_caller::<R>(entity_mut, now, entities_to_relate, caller);
    };

    let mut staying = EntityHashSet::from_iter(entities_to_relate.iter().copied());
    for &entity in newly_related_entities {
        staying.remove(entity);
    }

    let mut related = Vec::with_capacity(entities_to_unrelate.len() + entities_to_relate.len());
    related.extend_from_slice(entities_to_unrelate);
    related.extend(staying.into_iter());
    related.extend_from_slice(newly_related_entities);

    let entity = entity_mut.id();
    entity_mut.redo_and_buffer(
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

    Ok(entity_mut)
}

pub(crate) fn rev_try_with_related_with_caller<'a, 'b, R: Relationship>(
    entity_mut: &'a mut EntityWorldMut<'b>,
    now: NonLogNow,
    bundle: impl Bundle<Effect: NoBundleEffect>,
    caller: MaybeLocation,
) -> Result<&'a mut EntityWorldMut<'b>, EntityRevDespawnedError> {
    assert_not_rev_despawned(&*entity_mut)?;

    let parent = entity_mut.id();
    entity_mut.world_scope(|world| {
        // todo: R Teil anders umsetzen
        rev_spawn_with_caller(world, now, (bundle, R::from(parent)), caller);
    });

    Ok(entity_mut)
}

struct AddRelated<R, Target>
where
    R: Relationship,
    Target: AsRef<[Entity]> + Send + Sync + 'static,
{
    target: Entity,
    related: Target,
    caller: MaybeLocation,
    _marker: PhantomData<R>,
}

impl<R, Target> UndoRedo for AddRelated<R, Target>
where
    R: Relationship,
    Target: AsRef<[Entity]> + Send + Sync + 'static,
{
    fn undo(&mut self, world: &mut World) {
        let component_id = world.component_id::<R>().unwrap();
        for &target in self.related.as_ref().into_iter() {
            world.entity_mut(target).remove_by_id(component_id);
        }
    }
    fn redo(&mut self, world: &mut World) {
        world.insert_batch(
            self.related
                .as_ref()
                .into_iter()
                .copied()
                .map(|target| (target, R::from(self.target))),
        );
    }
}

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

struct ReplaceRelatedWithDifference<R: Relationship> {
    entity: Entity,
    related: Box<[Entity]>,        // [R, R, S, S, N, N]
    entities_to_unrelate: usize,   // ^-----^
    newly_related_entities: usize, // ^-----------^
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
