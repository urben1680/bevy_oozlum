use std::{
    any::TypeId,
    marker::PhantomData,
    mem::{replace, take},
    sync::Arc,
};

use bevy::{
    ecs::{
        bundle::BundleId, change_detection::MaybeLocation, component::{Component, ComponentId}, entity::{Entity, EntityHashMap, EntityHashSet}, hierarchy::ChildOf, relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget}, resource::Resource, world::{
            error::EntityMutableFetchError, EntityRef, EntityWorldMut, Entry as EntityEntry, World
        }
    },
    platform::collections::{hash_map::Entry as MapEntry, hash_set::Entry as SetEntry},
};

use crate::{
    meta::NonLogNow,
    prelude::UndoRedo,
    undo_redo::{BuffersUndoRedo, DisabledToDespawn, EntityRevDespawnedError},
};

use super::{BufferAt, RevEntitiesError, RevEntityError, RevIsDespawned, RevWorld};

struct RelationshipBuffer<
    T: Send + 'static,
    Values: ExtendDrain<T>,
    const BOTH_EXTRA: bool, // is `true` if both relationship components have extra fields not relevant to relationships
    const ONE_TO_ONE: bool, // is `true` if T is part of a 1-to-1 relationship, has no effect if `BOTH_EXTRA` is false
> {
    values: Values,
    entities: Values::Entities,
    _p: PhantomData<T>,
}

trait ExtendDrain<T: 'static>: Send + 'static {
    type Entities: Send + 'static;
    fn extend_values(&mut self, values: impl IntoIterator<Item = T>);
    fn drain_values(&mut self) -> impl Iterator<Item = T>;
    fn iter_entities(entities: &Self::Entities) -> impl Iterator<Item = Entity>;
}

impl<T: Send + 'static> ExtendDrain<T> for Option<T> {
    type Entities = Entity;
    fn extend_values(&mut self, values: impl IntoIterator<Item = T>) {
        let mut values = values.into_iter();
        *self = values.next();
        debug_assert!(values.next().is_none())
    }
    fn drain_values(&mut self) -> impl Iterator<Item = T> {
        self.take().into_iter()
    }
    fn iter_entities(entity: &Self::Entities) -> impl Iterator<Item = Entity> {
        [*entity].into_iter()
    }
}

impl<T: Send + 'static> ExtendDrain<T> for Vec<T> {
    type Entities = Box<[Entity]>;
    fn extend_values(&mut self, values: impl IntoIterator<Item = T>) {
        self.extend(values)
    }
    fn drain_values(&mut self) -> impl Iterator<Item = T> {
        self.drain(..)
    }
    fn iter_entities(entities: &Self::Entities) -> impl Iterator<Item = Entity> {
        entities.iter().copied()
    }
}

impl<T: Component, Values: ExtendDrain<T>, const BOTH_EXTRA: bool, const ONE_TO_ONE: bool>
    RelationshipBuffer<T, Values, BOTH_EXTRA, ONE_TO_ONE>
{
    #[inline]
    fn common_undo(&mut self, world: &mut World) {
        world.insert_batch(Values::iter_entities(&self.entities).zip(self.values.drain_values()));
    }
}

impl<T: Component, const BOTH_EXTRA: bool, const ONE_TO_ONE: bool>
    RelationshipBuffer<T, Vec<T>, BOTH_EXTRA, ONE_TO_ONE>
where
    Self: UndoRedo,
{
    #[inline]
    fn construct_apply_store(world: &mut World, now: NonLogNow, entities: Vec<Entity>) {
        if entities.is_empty() {
            return;
        }
        let entities = entities.into_boxed_slice();
        let mut buffer = Self {
            values: Vec::with_capacity(entities.len()),
            entities,
            _p: PhantomData,
        };
        buffer.redo(world);
        world.buffer_undo_redo(now, buffer);
    }
}

impl<T: Component, Values: ExtendDrain<T>, const ONE_TO_ONE: bool> UndoRedo
    for RelationshipBuffer<T, Values, false, ONE_TO_ONE>
{
    fn undo(&mut self, world: &mut World) {
        self.common_undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        let values = Values::iter_entities(&self.entities).map(|entity| {
            let mut entity = world.entity_mut(entity);
            entity.take::<T>().expect("todo")
        });
        self.values.extend_values(values);
    }
}

impl<T: RelationshipTarget, Values: ExtendDrain<T>, Other: UndoRedo> UndoRedo
    for (Other, RelationshipBuffer<T, Values, true, false>)
{
    fn undo(&mut self, world: &mut World) {
        self.1.common_undo(world);
        self.0.undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        for entity in Values::iter_entities(&self.1.entities) {
            world
                .get_mut::<T>(entity)
                .expect("todo")
                .collection_mut_risky()
                .add(Entity::PLACEHOLDER);
        }

        // buffering T::Relationship will now not remove T because with Entity::PLACEHOLDER it is not empty
        self.0.redo(world);

        let values = Values::iter_entities(&self.1.entities).map(|entity| {
            let mut entity = world.entity_mut(entity);
            let EntityEntry::Occupied(mut value) = entity.entry::<T>() else {
                panic!("todo");
            };
            // remove Entity::PLACEHOLDER before taking to make this trickery unnoticable by T's hooks
            value
                .get_mut()
                .collection_mut_risky()
                .remove(Entity::PLACEHOLDER);
            value.take()
        });
        self.1.values.extend_values(values);
    }
}

impl<T: RelationshipTarget, Values: ExtendDrain<T>, Other: UndoRedo> UndoRedo
    for (Other, RelationshipBuffer<T, Values, true, true>)
{
    fn undo(&mut self, world: &mut World) {
        self.1.common_undo(world);
        self.0.undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        let iter = Values::iter_entities(&self.1.entities).map(|entity| {
            // T::Collection is Entity
            let value = &mut *world.get_mut::<T>(entity).expect("todo");
            let mut replacement = T::Collection::new();
            replacement.add(value.collection().iter().next().expect("todo"));
            let replacement = T::from_collection_risky(replacement);
            replace(value, replacement)
        });
        self.1.values.extend_values(iter);
        self.0.redo(world); // todo: correct order? add comment explaining this 
    }
}

/*
- sammle rekursiv die entities die DisabledToDespawn erhalten sollen
- alle relationship types checken ob dieses entity hier einen parent hat und in dem fall buffert T
  und/oder, wenn es keine geschwister hat und target gespeichert werden muss, auch RelationshipTarget vom parent
- für LINKED_SPAWN wird nichts weiter unternommen da es keine Kinder gibt die nicht despawnen sollen
- für nicht LINKED_SPAWN werden nicht zu despawnenden kindern aller despawned entities das T und/oder von diesem
  zu despawnenden entity das RelationshipTarget gebuffert

- fn für pre-insert
- Option(fn) für linked despawn-entities
- fn für despawn-buffer (top)
- Option(fn) für non-linked despawn-buffer (bottoms)

ChildOf bekommt extra logik außerhalb Fns sodass diese nicht registriert werden muss und performanter ist

braucht es eine EntityHashSet?

 */

#[derive(Default, Clone, Resource)]
pub(crate) struct RevRelationship {
    fns: Arc<RevRelationshipInner>,
    registered: Vec<TypeId>,
}

#[derive(Default)]
struct RevRelationshipInner {
    linked: Vec<LinkedFns>,
    not_linked: Vec<NotLinkedFns>,
}

struct LinkedFns {
    backup: BackupFn,
    collect_despawn: CollectDespawnFn,
    buffer_top: BufferTopFn,
}

struct NotLinkedFns {
    backup: BackupFn,
    buffer_top: BufferTopFn,
    buffer_bottom: BufferBottomFn,
}

type BackupFn = fn(&mut EntityWorldMut, BundleId, NonLogNow, BufferAt);
type CollectDespawnFn = fn(&RevRelationship, EntityRef, &World, &mut DespawnResultsFn);
type BufferTopFn = fn(&mut EntityWorldMut, NonLogNow, &DespawnResultsFn);
type BufferBottomFn = fn(&mut World, Entity, NonLogNow, &DespawnResultsFn, &mut EntityHashSet);
type DespawnResultsFn = EntityHashMap<Result<(), RevEntityError>>;

impl RevRelationship {
    pub(crate) fn register<T: Relationship>(&mut self) {
        let id = TypeId::of::<T>();
        if TypeId::of::<ChildOf>() == id || self.registered.contains(&id) {
            return;
        }

        let fns_mut = Arc::get_mut(&mut self.fns).expect("todo");
        self.registered.push(id);

        let relationship_extra = size_of::<T>() > size_of::<Entity>();
        let relationship_target_extra = size_of::<T::RelationshipTarget>()
            > size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let one_to_one = TypeId::of::<Entity>()
            == TypeId::of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();

        let backup: BackupFn;
        let buffer_top: BufferTopFn;
        let buffer_bottom: BufferBottomFn;

        match (relationship_extra, relationship_target_extra, one_to_one) {
            (_, false, _) => {
                backup = backup_relationship_extra::<T>;
                buffer_top = buffer_top_relationship_extra::<T>;
                buffer_bottom = buffer_bottom_relationship_extra::<T>;
            }
            (false, true, _) => {
                backup = backup_relationship_target_extra::<T>;
                buffer_top = buffer_top_relationship_target_extra::<T>;
                buffer_bottom = buffer_bottom_relationship_target_extra::<T>;
            }
            (true, true, false) => {
                backup = backup_both_extra::<T, false>;
                buffer_top = buffer_top_both_extra::<T, false>;
                buffer_bottom = buffer_bottom_both_extra::<T, false>;
            }
            (true, true, true) => {
                backup = backup_both_extra::<T, true>;
                buffer_top = buffer_top_both_extra::<T, true>;
                buffer_bottom = buffer_bottom_both_extra::<T, true>;
            }
        }

        if <T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            fns_mut.linked.push(LinkedFns {
                backup,
                collect_despawn: Self::collect_despawn::<T>,
                buffer_top,
            });
        } else {
            fns_mut.not_linked.push(NotLinkedFns {
                backup,
                buffer_top,
                buffer_bottom,
            })
        }
    }
    /// backup these component ids if they are relevant to relationships
    pub(crate) fn backup(&self, entity: &mut EntityWorldMut, bundle: BundleId, now: NonLogNow, at: BufferAt) {
        backup_relationship_extra::<ChildOf>(entity, bundle, now, at);
        for f in self.fns.linked.iter() {
            (f.backup)(entity, bundle, now, at)
        }
        for f in self.fns.not_linked.iter() {
            (f.backup)(entity, bundle, now, at)
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

        // buffer relationship components of entity and it's non-despawning parents
        buffer_top_relationship_extra::<ChildOf>(entity_mut, now, &cache.results);
        for f in self.fns.linked.iter() {
            (f.buffer_top)(entity_mut, now, &cache.results);
        }
        for f in self.fns.not_linked.iter() {
            (f.buffer_top)(entity_mut, now, &cache.results);
        }

        entity_mut.world_scope(|world| {
            let Cache {
                results,
                visited,
                errors,
            } = &mut cache;

            // buffer relationship components of entities and their non-despawning children
            for f in self.fns.not_linked.iter() {
                (f.buffer_bottom)(world, entity, now, &results, visited);
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
        self.fns
            .linked
            .iter()
            .for_each(|fns| (fns.collect_despawn)(self, entity_ref, world, results));
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

fn backup_relationship_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    bundle: BundleId,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

fn backup_relationship_target_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    bundle: BundleId,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

fn backup_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    entity_mut: &mut EntityWorldMut,
    bundle: BundleId,
    now: NonLogNow,
    at: BufferAt,
) {
    todo!()
}

// todo: buffer top does not need to check if parents are (reversibly) despawned if relationship is linked

fn buffer_top_relationship_extra<T: Relationship>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    results: &DespawnResultsFn,
) {
    todo!()
}

fn buffer_top_relationship_target_extra<T: Relationship>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    results: &DespawnResultsFn,
) {
    todo!()
}

fn buffer_top_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    entity: &mut EntityWorldMut,
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

/*
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
