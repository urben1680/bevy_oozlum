use std::{
    any::TypeId,
    marker::PhantomData,
    mem::{replace, take},
    sync::Arc,
};

use bevy::{
    ecs::{
        bundle::BundleId,
        change_detection::MaybeLocation,
        component::{Component, ComponentId},
        entity::{Entity, EntityHashMap, EntityHashSet},
        hierarchy::{ChildOf, Children},
        relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
        resource::Resource,
        world::{
            EntityRef, EntityWorldMut, Entry as EntityEntry, FromWorld, World,
            error::EntityMutableFetchError,
        },
    },
    platform::collections::{HashSet, hash_map::Entry as MapEntry, hash_set::Entry as SetEntry},
};

use crate::{
    meta::NonLogNow,
    prelude::{UndoRedo, UndoRedoSwap},
    undo_redo::{BuffersUndoRedo, DeferredUndoRedo, DisabledToDespawn, EntityRevDespawnedError},
};

use super::{BufferAt, RevEntitiesError, RevEntityError, RevIsDespawned, RevWorld};

struct BufferOneForManyEntities<T: Component> {
    values: Vec<T>,
    entities: Box<[Entity]>,
}

impl<T: Component> BufferOneForManyEntities<T> {
    pub fn new(entities: Vec<Entity>) -> Self {
        let entities = entities.into_boxed_slice();
        Self {
            values: Vec::with_capacity(entities.len()),
            entities,
        }
    }
    fn new_filter_empty(entities: Vec<Entity>) -> Option<Self> {
        if entities.is_empty() {
            None
        } else {
            Some(Self::new(entities))
        }
    }
    #[inline]
    fn common_undo(&mut self, world: &mut World) {
        world.insert_batch(self.entities.iter().copied().zip(self.values.drain(..)));
    }
}

impl<T: Component> UndoRedo for BufferOneForManyEntities<T> {
    fn undo(&mut self, world: &mut World) {
        self.common_undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        let values = self
            .entities
            .iter()
            .map(|entity| world.entity_mut(*entity).take::<T>().expect("todo"));
        self.values.extend(values);
    }
}

struct BufferBothForManyEntities<T: Relationship, const ONE_TO_ONE: bool> {
    relationship: BufferOneForManyEntities<T>,
    relationship_target: BufferOneForManyEntities<T::RelationshipTarget>,
}

impl<T: Relationship, const ONE_TO_ONE: bool> UndoRedo
    for BufferBothForManyEntities<T, ONE_TO_ONE>
{
    fn undo(&mut self, world: &mut World) {
        self.relationship_target.undo(world);
        self.relationship.undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        if ONE_TO_ONE {
            let iter = self.relationship_target.entities.iter().map(|entity| {
                // T::Collection is Entity
                let value = &mut *world
                    .get_mut::<T::RelationshipTarget>(*entity)
                    .expect("todo");
                let mut replacement =
                    <T::RelationshipTarget as RelationshipTarget>::Collection::new();
                replacement.add(value.collection().iter().next().expect("todo"));
                let replacement = T::RelationshipTarget::from_collection_risky(replacement);
                replace(value, replacement)
            });
            self.relationship_target.values.extend(iter);
            self.relationship.redo(world);
        } else {
            for entity in self.relationship_target.entities.iter() {
                world
                    .get_mut::<T::RelationshipTarget>(*entity)
                    .expect("todo")
                    .collection_mut_risky()
                    .add(Entity::PLACEHOLDER);
            }

            // buffering T::Relationship will now not remove T because with Entity::PLACEHOLDER it is not empty
            self.relationship.redo(world);

            let values = self.relationship_target.entities.iter().map(|entity| {
                let mut entity = world.entity_mut(*entity);
                let EntityEntry::Occupied(mut value) = entity.entry::<T::RelationshipTarget>()
                else {
                    panic!("todo");
                };
                // remove Entity::PLACEHOLDER before taking to make this trickery unnoticable by T's hooks
                value
                    .get_mut()
                    .collection_mut_risky()
                    .remove(Entity::PLACEHOLDER);
                value.take()
            });
            self.relationship_target.values.extend(values);
        }
    }
}

struct BufferOneForSingleEntity<T: Component> {
    value: Option<T>,
    entity: Entity,
}

impl<T: Component> BufferOneForSingleEntity<T> {
    fn new(entity: Entity) -> Self {
        Self {
            value: None,
            entity,
        }
    }
}

impl<T: Component> UndoRedo for BufferOneForSingleEntity<T> {
    fn undo(&mut self, world: &mut World) {
        world
            .entity_mut(self.entity)
            .insert(self.value.take().unwrap());
    }
    fn redo(&mut self, world: &mut World) {
        self.value = world.entity_mut(self.entity).take::<T>();
    }
}

#[derive(Clone, Resource)]
pub(crate) struct RevRelationship(Arc<Inner>);

struct Inner {
    child_of: Ids,
    fns: Vec<Fns>,
    registered: Vec<TypeId>,
}

struct Fns {
    ids: Ids,
    backup: BackupFn,
    collect_despawn: Option<CollectDespawnFn>,
    buffer_despawn: BufferDespawn,
}

#[derive(Clone, Copy)]
struct Ids {
    relationship: ComponentId,
    relationship_target: ComponentId,
}

impl FromWorld for RevRelationship {
    fn from_world(world: &mut World) -> Self {
        let relationship = world.register_component::<ChildOf>();
        let relationship_target = world.register_component::<Children>();
        Self(Arc::new(Inner {
            child_of: Ids {
                relationship,
                relationship_target,
            },
            fns: Vec::new(),
            registered: Vec::new(),
        }))
    }
}

type BackupFn = fn(&mut EntityWorldMut, Ids, &HashSet<ComponentId>, NonLogNow, BufferAt);
type CollectDespawnFn = fn(&RevRelationship, EntityRef, &World, &mut DespawnResultsFn);
type BufferDespawn = fn(&mut World, NonLogNow, &DespawnResultsFn, &mut EntityHashSet);
type DespawnResultsFn = EntityHashMap<Result<(), RevEntityError>>;

impl RevRelationship {
    pub(crate) fn register<T: Relationship>(
        &mut self,
        relationship: ComponentId,
        relationship_target: ComponentId,
    ) {
        let id = TypeId::of::<T>();
        if TypeId::of::<ChildOf>() == id || self.0.registered.contains(&id) {
            return;
        }

        let relationship_extra = size_of::<T>() > size_of::<Entity>();
        let relationship_target_extra = size_of::<T::RelationshipTarget>()
            > size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let one_to_one = TypeId::of::<Entity>()
            == TypeId::of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let ids = Ids {
            relationship,
            relationship_target,
        };
        let collect_despawn = <T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN
            .then_some(Self::collect_despawn::<T> as CollectDespawnFn);

        let fns = match (relationship_extra, relationship_target_extra, one_to_one) {
            (_, false, _) => Fns {
                ids,
                backup: backup_relationship_extra::<T>,
                collect_despawn,
                buffer_despawn: buffer_despawn_relationship_extra::<T>,
            },
            (false, true, _) => Fns {
                ids,
                backup: backup_relationship_target_extra::<T>,
                collect_despawn,
                buffer_despawn: buffer_despawn_relationship_target_extra::<T>,
            },
            (true, true, false) => Fns {
                ids,
                backup: backup_both_extra::<T, false>,
                collect_despawn,
                buffer_despawn: buffer_despawn_both_extra::<T, false>,
            },
            (true, true, true) => Fns {
                ids,
                backup: backup_both_extra::<T, true>,
                collect_despawn,
                buffer_despawn: buffer_despawn_both_extra::<T, true>,
            },
        };

        let inner = Arc::get_mut(&mut self.0).expect("todo");
        inner.fns.push(fns);
        inner.registered.push(id);
    }
    /// backup these component ids if they are relevant to relationships
    pub(crate) fn backup(
        &self,
        entity: &mut EntityWorldMut,
        components: &HashSet<ComponentId>,
        now: NonLogNow,
        at: BufferAt,
    ) {
        backup_relationship_extra::<ChildOf>(entity, self.0.child_of, components, now, at);
        for f in self.0.fns.iter() {
            (f.backup)(entity, f.ids, components, now, at)
        }
    }
    #[track_caller]
    pub(crate) fn try_despawn(
        &self,
        entity_mut: &mut EntityWorldMut,
        now: NonLogNow,
    ) -> Result<(), RevEntitiesError> {
        #[derive(Default, Resource)]
        struct Cache {
            results: DespawnResultsFn,
            visited: EntityHashSet,
            errors: Vec<RevEntityError>,
        }

        let entity = entity_mut.id();

        if entity_mut.is_despawned() {
            let error = entity_mut.world().get_entity(entity).err().unwrap();
            return Err(error.into());
        }
        if let Some(&marker) = entity_mut.get::<DisabledToDespawn>() {
            let error = EntityRevDespawnedError { entity, marker };
            return Err(error.into());
        }

        let mut cache = entity_mut
            .get_resource_mut::<Cache>()
            .map(|cache| take(cache.into_inner()))
            .unwrap_or_default();

        cache.results.insert(entity, Ok(()));

        // collect entities that should be despawned and errors of entities that are already (reversibly) despawned
        self.recursive_collect_despawn(
            (&*entity_mut).into(),
            entity_mut.world(),
            &mut cache.results,
        );

        entity_mut.world_scope(|world| {
            let Cache {
                results,
                visited,
                errors,
            } = &mut cache;

            // buffer relationship components of entities and their non-despawning parents/children if needed
            for f in self.0.fns.iter() {
                (f.buffer_despawn)(world, now, &results, visited);
                visited.clear();
            }

            // add DisabledToDespawn to despawning entities
            let marker = DisabledToDespawn::for_spawn_despawn(now.0);
            let mut result = world.rev_try_insert_batch(
                now,
                results.drain().filter_map(|(entity, result)| match result {
                    Ok(()) => Some((entity, marker)),
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }),
            );

            // collect errors
            if !errors.is_empty() {
                let mut entities_error = result.err().unwrap_or_else(|| RevEntitiesError::empty());
                errors
                    .drain(..)
                    .for_each(|entity_error| entities_error.push(entity_error));
                result = Err(entities_error);
            }

            world.insert_resource(cache);

            result
        })
    }
    fn recursive_collect_despawn(
        &self,
        entity_ref: EntityRef,
        world: &World,
        results: &mut DespawnResultsFn,
    ) {
        self.collect_despawn::<ChildOf>(entity_ref, world, results);
        self.0
            .fns
            .iter()
            .flat_map(|fns| fns.collect_despawn)
            .for_each(|collect_despawn| collect_despawn(self, entity_ref, world, results));
    }
    fn collect_despawn<T: Relationship>(
        &self,
        entity_ref: EntityRef,
        world: &World,
        results: &mut DespawnResultsFn,
    ) {
        let Some(children) = entity_ref.get::<T::RelationshipTarget>() else {
            return;
        };
        for child in children.iter() {
            let MapEntry::Vacant(vacant) = results.entry(child) else {
                continue;
            };
            match world.get_entity(child) {
                Ok(entity_ref) => match entity_ref.get::<DisabledToDespawn>() {
                    None => {
                        vacant.insert(Ok(()));
                        self.recursive_collect_despawn(entity_ref, world, results);
                    }
                    Some(&marker) => {
                        let error = EntityRevDespawnedError {
                            entity: child,
                            marker,
                        };
                        vacant.insert(Err(error.into()));
                    }
                },
                Err(err) => {
                    vacant.insert(Err(err.into()));
                }
            }
        }
    }
}

// backup implementierung falsch!
// bei at != Undo: wie bisher umsetzen
// bei at == Undo: anhand von `components` festlegen ob ein uninitalisierter buffer gespeichert werden soll
// es benötigt beide für NowAndUndo, vielleicht als swap?

fn backup_relationship_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components: &HashSet<ComponentId>,
    now: NonLogNow,
    at: BufferAt,
) {
    let entity = entity_mut.id();

    match at {
        BufferAt::Now => {
            let mut buffer_relationship = None;
            if components.contains(&ids.relationship) && entity_mut.contains_id(ids.relationship) {
                buffer_relationship = Some(BufferOneForSingleEntity::<T>::new(entity_mut.id()));
            }

            let mut buffer_relationship_target = None;
            if components.contains(&ids.relationship_target) {
                if let Some(children) = entity_mut.get::<T::RelationshipTarget>() {
                    let children = children.iter().collect::<Vec<_>>();
                    buffer_relationship_target =
                        BufferOneForManyEntities::<T>::new_filter_empty(children);
                }
            }

            // SAFETY: will call update_locations if components are taken here
            let world = unsafe { entity_mut.world_mut() };

            match (buffer_relationship, buffer_relationship_target) {
                (None, None) => return,
                (Some(buffer), None) => world.redo_and_buffer(now, buffer),
                (None, Some(buffer)) => world.redo_and_buffer(now, buffer),
                (Some(relationship), Some(relationship_target)) => {
                    world.redo_and_buffer(now, relationship);
                    world.redo_and_buffer(now, relationship_target);
                }
            }

            entity_mut.update_location();
        }
        BufferAt::Undo => {
            let mut buffer_relationship = None;
            if components.contains(&ids.relationship) {
                buffer_relationship = Some(BufferOneForSingleEntity::<T>::new(entity_mut.id()));
            }

            let mut buffer_relationship_target = None;
            if components.contains(&ids.relationship_target) {
                buffer_relationship_target = Some(DeferredUndoRedo::new(move |world| {
                    let children = world
                        .get::<T::RelationshipTarget>(entity)
                        .into_iter()
                        .flat_map(RelationshipTarget::iter)
                        .collect();
                    let mut buffer = UndoRedoSwap(BufferOneForManyEntities::<T>::new(children));
                    buffer.undo(world);
                    buffer
                }))
            }

            // SAFETY: will call update_locations if components are taken here
            let world = unsafe { entity_mut.world_mut() };

            match (buffer_relationship, buffer_relationship_target) {
                (None, None) => return,
                (Some(buffer), None) => world.redo_and_buffer(now, buffer),
                (None, Some(buffer)) => world.redo_and_buffer(now, buffer),
                (Some(relationship), Some(relationship_target)) => {
                    world.redo_and_buffer(now, relationship);
                    world.redo_and_buffer(now, relationship_target);
                }
            }

            entity_mut.update_location();
        }
        BufferAt::NowAndUndo => {
            todo!()
        }
    }
}

fn backup_relationship_target_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components: &HashSet<ComponentId>,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

fn backup_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components: &HashSet<ComponentId>,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

fn buffer_despawn_relationship_extra<T: Relationship>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResultsFn,
    visited: &mut EntityHashSet,
) {
    let mut relationship_entities = Vec::new();
    let entities = results
        .iter()
        .filter_map(|(entity, result)| result.is_ok().then_some(*entity));
    for entity in entities {
        if !visited.insert(entity) {
            continue;
        }
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };

        if !<T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            if let Some(children) = entity_ref.get::<T::RelationshipTarget>() {
                let children = children
                    .iter()
                    .filter(|child| !results.contains_key(child) && visited.insert(*child));
                relationship_entities.extend(children);
            }
        }

        if entity_ref
            .get::<T>()
            .map(T::get)
            .is_some_and(|parent| !results.contains_key(&parent))
        {
            relationship_entities.push(entity);
        }
    }
    if let Some(buffer) = BufferOneForManyEntities::<T>::new_filter_empty(relationship_entities) {
        world.redo_and_buffer(now, buffer);
    }
}

fn buffer_despawn_relationship_target_extra<T: Relationship>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResultsFn,
    visited: &mut EntityHashSet,
) {
    let mut relationship_entities = Vec::new();
    let mut relationship_target_entities = Vec::new();
    let entities = results
        .iter()
        .filter_map(|(entity, result)| result.is_ok().then_some(*entity));
    for entity in entities {
        if !visited.insert(entity) {
            continue;
        }
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };

        if !<T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            if let Some(children) = entity_ref.get::<T::RelationshipTarget>() {
                if children.iter().any(|child| !results.contains_key(&child)) {
                    relationship_target_entities.push(entity);
                }
            }
        }

        let parent = entity_ref
            .get::<T>()
            .map(T::get)
            .filter(|parent| !results.contains_key(parent) && visited.insert(*parent));
        let Some(parent) = parent else {
            continue;
        };
        let previous_relationship_len = relationship_entities.len();
        let mut all_siblings_despawn = true;
        let siblings = world
            .get::<T::RelationshipTarget>(parent)
            .into_iter()
            .flat_map(RelationshipTarget::iter);
        for sibling in siblings {
            if results.contains_key(&sibling) {
                relationship_entities.push(sibling);
            } else {
                all_siblings_despawn = false;
            }
        }
        if all_siblings_despawn {
            relationship_entities.truncate(previous_relationship_len);
            relationship_target_entities.push(parent);
        }
    }
    if let Some(buffer) = BufferOneForManyEntities::<T>::new_filter_empty(relationship_entities) {
        world.redo_and_buffer(now, buffer);
    }
    if let Some(buffer) = BufferOneForManyEntities::<T::RelationshipTarget>::new_filter_empty(
        relationship_target_entities,
    ) {
        world.redo_and_buffer(now, buffer);
    }
}

fn buffer_despawn_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResultsFn,
    visited: &mut EntityHashSet,
) {
    let mut relationship_entities = Vec::new();
    let mut relationship_target_entities = Vec::new();
    for entity in results
        .iter()
        .filter_map(|(entity, result)| result.is_ok().then_some(*entity))
    {
        if !visited.insert(entity) {
            continue;
        }
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };

        if !<T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            if let Some(children) = entity_ref.get::<T::RelationshipTarget>() {
                let previous_relationship_len = relationship_entities.len();
                let mut any_children_despawn = false;
                for child in children.iter() {
                    if results.contains_key(&child) {
                        any_children_despawn = true;
                    } else {
                        relationship_entities.push(child);
                    }
                }
                if relationship_entities.len() > previous_relationship_len && any_children_despawn {
                    relationship_target_entities.push(entity);
                }
            }
        }

        let parent = entity_ref
            .get::<T>()
            .map(T::get)
            .filter(|parent| !results.contains_key(parent) && visited.insert(*parent));
        let Some(parent) = parent else {
            continue;
        };

        let mut all_siblings_despawn = true;
        let siblings = world
            .get::<T::RelationshipTarget>(parent)
            .into_iter()
            .flat_map(RelationshipTarget::iter);
        for sibling in siblings {
            if results.contains_key(&sibling) {
                relationship_entities.push(sibling);
            } else {
                all_siblings_despawn = false;
            }
        }
        if all_siblings_despawn {
            relationship_target_entities.push(parent);
        }
    }

    match (
        BufferOneForManyEntities::<T>::new_filter_empty(relationship_entities),
        BufferOneForManyEntities::<T::RelationshipTarget>::new_filter_empty(
            relationship_target_entities,
        ),
    ) {
        (None, None) => return,
        (Some(buffer), None) => world.redo_and_buffer(now, buffer),
        (None, Some(buffer)) => world.redo_and_buffer(now, buffer),
        (Some(relationship), Some(relationship_target)) => {
            let buffer = BufferBothForManyEntities::<T, ONE_TO_ONE> {
                relationship,
                relationship_target,
            };
            world.redo_and_buffer(now, buffer)
        }
    }
}

// todo: buffer top does not need to check if parents are (reversibly) despawned if relationship is linked

// problem: not only top entity can have non-despawned parent
// unify buffer top/bottom again

/*
fn buffer_top_relationship_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    results: &DespawnResultsFn,
) {
    todo!()
}

fn buffer_top_relationship_target_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    results: &DespawnResultsFn,
) {
    todo!()
}

fn buffer_top_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    results: &DespawnResultsFn,
) {
    todo!()
}

fn buffer_bottom_relationship_extra<T: Relationship>(
    world: &mut World,
    top: Entity,
    now: NonLogNow,
    results: &DespawnResultsFn,
    visited: &mut EntityHashSet,
) {
    todo!()
}

fn buffer_bottom_relationship_target_extra<T: Relationship>(
    world: &mut World,
    top: Entity,
    now: NonLogNow,
    results: &DespawnResultsFn,
    visited: &mut EntityHashSet,
) {
    todo!()
}

fn buffer_bottom_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    world: &mut World,
    top: Entity,
    now: NonLogNow,
    results: &DespawnResultsFn,
    visited: &mut EntityHashSet,
) {
    todo!()
}

#[derive(Default, Clone, Resource)]
pub(crate) struct RevRelationship {
    fns: Arc<Vec<RelationshipFns>>,
}

impl RevRelationship {
    pub(crate) fn register<T: Relationship>(&mut self) {
        let relationship_extra = size_of::<T>() > size_of::<Entity>();
        let relationship_target_extra = size_of::<T::RelationshipTarget>()
            > size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let one_to_one = TypeId::of::<Entity>()
            == TypeId::of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let despawn_entities = <T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN
            .then_some(Self::despawn_entities_linked::<T> as DespawnEntities);
        let fns = match (relationship_extra, relationship_target_extra, one_to_one) {
            (_, false, _) => RelationshipFns {
                backup: backup_relationship_extra::<T>,
                despawn_entities,
                despawn_buffers: despawn_relationship_extra::<T>,
            },
            (false, true, _) => RelationshipFns {
                backup: backup_relationship_target_extra::<T>,
                despawn_entities,
                despawn_buffers: despawn_relationship_target_extra::<T>,
            },
            (true, true, false) => RelationshipFns {
                backup: backup_both_extra::<T, false>,
                despawn_entities,
                despawn_buffers: despawn_both_extra::<T, false>,
            },
            (true, true, true) => RelationshipFns {
                backup: backup_both_extra::<T, true>,
                despawn_entities,
                despawn_buffers: despawn_both_extra::<T, true>,
            },
        };
        let fns_mut = Arc::get_mut(&mut self.fns).expect("todo");
        if !fns_mut.contains(&fns) {
            fns_mut.push(fns);
        }
    }
    fn iter_pre_despawn_entities(&self) -> impl Iterator<Item = DespawnEntities> + Clone {
        self.fns.iter().filter_map(|fns| fns.despawn_entities)
    }
    /// backup these component ids if they are relevant to relationships
    pub(crate) fn backup(&self, entity: &mut EntityWorldMut, now: NonLogNow, at: BufferAt) {
        for f in self.fns.iter() {
            (f.backup)(entity, now, at)
        }
    }
    #[track_caller]
    pub(crate) fn try_despawn(
        &self,
        entity_mut: &mut EntityWorldMut,
        now: NonLogNow,
    ) -> Result<(), RevEntitiesError> {
        #[derive(Default, Resource)]
        struct Cache {
            results: DespawnResults,
            visited: EntityHashSet,
            errors: Vec<RevEntityError>,
        }

        let entity = entity_mut.id();

        if let Some(&marker) = entity_mut.get::<DisabledToDespawn>() {
            return Err(EntityRevDespawnedError { entity, marker }.into());
        }

        let mut cache = entity_mut
            .get_resource_mut::<Cache>()
            .map(|cache| take(cache.into_inner()))
            .unwrap_or_default();

        // false because
        cache.results.insert(entity, Ok(false));

        // collect entities that should be despawned and errors of entities that are already (reversibly) despawned
        let entity_ref = (&*entity_mut).into();
        self.for_each_pre_despawn_entities(|f| {
            f(self, entity_ref, entity_mut.world(), &mut cache.results)
        });

        entity_mut.world_scope(|world| {
            let Cache {
                results,
                visited,
                errors,
            } = &mut cache;

            // buffer relevant relationship components, their hooks/observers may further invalidate other entities so results gets updated
            for fns in self.fns.iter() {
                (fns.despawn_buffers)(world, now, results, visited);
                visited.clear();
            }

            // rev_try_insert_batch_if_new is not needed as rev_try_insert_batch already skips entities that contain DisabledToDespawn
            let marker = DisabledToDespawn::for_spawn_despawn(now.0);
            let mut result = world.rev_try_insert_batch(
                now,
                results.drain().filter_map(|(entity, result)| match result {
                    Ok(()) => Some((entity, marker)),
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }),
            );

            // collect errors
            if !errors.is_empty() {
                let mut entities_error = result.err().unwrap_or_else(|| RevEntitiesError::empty());
                errors
                    .drain(..)
                    .for_each(|entity_error| entities_error.push(entity_error));
                result = Err(entities_error);
            }

            world.insert_resource(cache);

            result
        })
    }
    fn for_each_pre_despawn_entities(&self, for_each: impl FnMut(DespawnEntities)) {
        self.fns
            .iter()
            .flat_map(|fns| fns.despawn_entities)
            .for_each(for_each);
    }
    fn despawn_entities_linked<T: Relationship>(
        &self,
        entity_ref: EntityRef,
        world: &World,
        results: &mut DespawnResults,
    ) {
        let Some(children) = entity_ref.get::<T::RelationshipTarget>() else {
            return;
        };
        for child in children.iter() {
            let MapEntry::Vacant(vacant) = results.entry(child) else {
                continue;
            };
            match world.get_entity(child) {
                Ok(entity_ref) => match entity_ref.get::<DisabledToDespawn>() {
                    None => {
                        vacant.insert(Ok(()));
                        self.for_each_pre_despawn_entities(|f| f(self, entity_ref, world, results));
                    }
                    Some(&marker) => {
                        vacant.insert(Err(EntityRevDespawnedError {
                            entity: child,
                            marker,
                        }
                        .into()));
                    }
                },
                Err(err) => {
                    vacant.insert(Err(err.into()));
                }
            }
        }
    }
}

type Backup = fn(&mut EntityWorldMut, NonLogNow, BufferAt);
type DespawnResults = EntityHashMap<Result<bool, RevEntityError>>;
type DespawnEntities = fn(&RevRelationship, EntityRef, &World, &mut DespawnResults);
type DespawnBuffers = fn(&mut World, NonLogNow, &DespawnResults, &mut EntityHashSet);

#[derive(PartialEq)]
struct RelationshipFns {
    backup: Backup,
    despawn_entities: Option<DespawnEntities>,
    despawn_buffers: DespawnBuffers,
}

fn backup_relationship_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

fn backup_relationship_target_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

fn backup_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

// optimization: only buffer relationships if they are related to a non-despawning entity
// dann ist der DespawnResults Ansatz ineffizient, die info welche entities die unterste hierarchie sind ging verloren

/*

- hat zu despawnendes einen parent? dann T buffern
- hat kind von entity

*/

fn despawn_relationship_extra<T: Relationship>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResults,
    visited: &mut EntityHashSet,
) {
    let mut relationship_entities = Vec::new();
    let entities = results
        .iter()
        .filter_map(|(entity, result)| result.is_ok().then_some(*entity));
    for entity in entities {
        if !visited.insert(entity) {
            continue;
        }
        let Ok(entity) = world.get_entity(entity) else {
            continue;
        };
        if entity
            .get::<T>()
            .map(T::get)
            .is_some_and(|parent| !results.contains_key(&parent))
        {
            relationship_entities.push(entity.id());
        }
        if !<T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            // children are already part of `results`
            continue;
        }
        if let Some(children) = entity.get::<T::RelationshipTarget>() {
            let children = children
                .iter()
                .filter(|child| !results.contains_key(child) && visited.insert(*child));
            relationship_entities.extend(children);
        }
    }
    RelationshipBuffer::<T, false, false>::construct_apply_store(world, now, relationship_entities);
}

fn despawn_relationship_target_extra<T: Relationship>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResults,
    visited: &mut EntityHashSet,
) {
    let mut relationship_entities = Vec::new();
    let mut relationship_target_entities = Vec::new();
    let entities = results
        .iter()
        .filter_map(|(entity, result)| result.is_ok().then_some(*entity));
    for entity in entities {
        if !visited.insert(entity) {
            continue;
        }
        let Ok(entity) = world.get_entity(entity) else {
            continue;
        };
        if entity.contains::<T::RelationshipTarget>() {
            relationship_target_entities.push(entity.id());
        }
        let parent = entity
            .get::<T>()
            .map(T::get)
            .filter(|parent| !results.contains_key(parent) && visited.insert(*parent));
        let Some(parent) = parent else {
            continue;
        };
        let previous_relationship_len = relationship_entities.len();
        let mut all_siblings_despawn = true;
        let siblings = world
            .get::<T::RelationshipTarget>(parent)
            .into_iter()
            .flat_map(RelationshipTarget::iter);
        for sibling in siblings {
            if results.contains_key(&sibling) {
                relationship_entities.push(sibling);
            } else {
                all_siblings_despawn = false;
            }
        }
        if all_siblings_despawn {
            relationship_entities.truncate(previous_relationship_len);
            relationship_target_entities.push(parent);
        }
    }
    RelationshipBuffer::<T, false, false>::construct_apply_store(world, now, relationship_entities);
    RelationshipBuffer::<T::RelationshipTarget, false, false>::construct_apply_store(
        world,
        now,
        relationship_target_entities,
    );
}

fn despawn_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResults,
    visited: &mut EntityHashSet,
) {
    let mut relationship_entities = Vec::new();
    let mut relationship_target_entities = Vec::new();
    for entity in results
        .iter()
        .filter_map(|(entity, result)| result.is_ok().then_some(*entity))
    {
        if !visited.insert(entity) {
            continue;
        }
        let Ok(entity) = world.get_entity(entity) else {
            continue;
        };

        // T
        if entity.contains::<T>() {
            relationship_entities.push(entity.id());
        }
        if !<T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            // children are already part of `results`
            continue;
        }
        if let Some(children) = entity.get::<T::RelationshipTarget>() {
            let children = children
                .iter()
                .filter(|child| !results.contains_key(child) && visited.insert(*child));
            relationship_entities.extend(children);
        }

        // T::RelationshipTarget
        if entity.contains::<T::RelationshipTarget>() {
            relationship_target_entities.push(entity.id());
        }
        let parent = entity
            .get::<T>()
            .map(T::get)
            .filter(|parent| !results.contains_key(parent) && visited.insert(*parent));
        let Some(parent) = parent else {
            continue;
        };
        let previous_relationship_len = relationship_entities.len();
        let mut all_siblings_despawn = true;
        let siblings = world
            .get::<T::RelationshipTarget>(parent)
            .into_iter()
            .flat_map(RelationshipTarget::iter);
        for sibling in siblings {
            if results.contains_key(&sibling) {
                relationship_entities.push(sibling);
            } else {
                all_siblings_despawn = false;
            }
        }
        if all_siblings_despawn {
            relationship_entities.truncate(previous_relationship_len);
            relationship_target_entities.push(parent);
        }

        // todo: visited must not block non-despawning entities to be checked as both children nor parents of despawning entities
    }
    todo!()
}
*/
