use std::{
    any::TypeId,
    mem::replace,
};

use bevy::{
    ecs::{
        component::{Component, ComponentId},
        entity::{Entity, EntityHashMap, EntityHashSet},
        hierarchy::{ChildOf, Children},
        relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
        resource::Resource,
        world::{EntityRef, EntityWorldMut, Entry as EntityEntry, FromWorld, World},
    },
    platform::collections::hash_map::Entry as MapEntry,
};

use crate::{
    meta::NonLogNow,
    prelude::{UndoRedo, UndoRedoSwap},
    undo_redo::{BuffersUndoRedo, DisabledToDespawn, EntityRevDespawnedError},
};

use super::{RevEntitiesError, RevEntityError, RevWorld, Spawn};

#[derive(Resource)]
pub(crate) struct RevRelationship {
    child_of: Ids,
    fns: Vec<Fns>,
    registered: Vec<TypeId>,
    cache_buffer: Vec<bool>,
    cache_despawn_results: DespawnResults,
    cache_despawn_visited: EntityHashSet,
}

struct Fns {
    ids: Ids,
    buffer: BufferFn,
    collect_despawn: Option<CollectDespawnFn>,
    buffer_despawn: BufferDespawnFn,
}

#[derive(Clone, Copy)]
struct Ids {
    relationship: ComponentId,
    relationship_target: ComponentId,
}

impl IntoIterator for Ids {
    type IntoIter = core::array::IntoIter<ComponentId, 2>;
    type Item = ComponentId;
    fn into_iter(self) -> Self::IntoIter {
        [self.relationship, self.relationship_target].into_iter()
    }
}

impl Ids {
    fn min_len(self) -> usize {
        self.relationship.index().max(self.relationship_target.index()) + 1
    }
}

impl FromWorld for RevRelationship {
    fn from_world(world: &mut World) -> Self {
        let relationship = world.register_component::<ChildOf>();
        let relationship_target = world.register_component::<Children>();
        let ids = Ids {
            relationship,
            relationship_target,
        };

        Self {
            child_of: ids,
            fns: Vec::new(),
            registered: Vec::new(),
            cache_buffer: vec![false; ids.min_len()],
            cache_despawn_results: EntityHashMap::new(),
            cache_despawn_visited: EntityHashSet::new()
        }
    }
}

type BufferFn = fn(&mut EntityWorldMut, Ids, &[bool], NonLogNow, bool) -> bool;
type CollectDespawnFn = fn(&Vec<Fns>, EntityRef, &World, &mut DespawnResults);
type BufferDespawnFn = fn(&mut World, NonLogNow, &DespawnResults, &mut EntityHashSet);
type DespawnResults = EntityHashMap<Result<(), RevEntityError>>;

impl RevRelationship {
    pub(crate) fn register<T: Relationship>(&mut self, world: &mut World) {
        let id = TypeId::of::<T>();
        if TypeId::of::<ChildOf>() == id || self.registered.contains(&id) {
            return;
        }

        let relationship = world.register_component::<T>();
        let relationship_target = world.register_component::<T::RelationshipTarget>();
        let ids = Ids {
            relationship,
            relationship_target,
        };

        if let Some(additional) = ids.min_len().checked_sub(self.cache_buffer.len()) {
            self.cache_buffer.reserve_exact(additional);
            self.cache_buffer.extend(core::iter::repeat_n(false, additional));
        }

        let relationship_extra = size_of::<T>() > size_of::<Entity>();
        let relationship_target_extra = size_of::<T::RelationshipTarget>()
            > size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let one_to_one = TypeId::of::<Entity>()
            == TypeId::of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let collect_despawn = <T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN
            .then_some(collect_despawn::<T> as CollectDespawnFn);

        let fns = match (relationship_extra, relationship_target_extra, one_to_one) {
            (_, false, _) => Fns {
                ids,
                buffer: buffer_relationship_extra::<T>,
                collect_despawn,
                buffer_despawn: buffer_despawn_relationship_extra::<T>,
            },
            (false, true, _) => Fns {
                ids,
                buffer: buffer_relationship_target_extra::<T>,
                collect_despawn,
                buffer_despawn: buffer_despawn_relationship_target_extra::<T>,
            },
            (true, true, false) => Fns {
                ids,
                buffer: buffer_both_extra::<T, false>,
                collect_despawn,
                buffer_despawn: buffer_despawn_both_extra::<T, false>,
            },
            (true, true, true) => Fns {
                ids,
                buffer: buffer_both_extra::<T, true>,
                collect_despawn,
                buffer_despawn: buffer_despawn_both_extra::<T, true>,
            },
        };

        self.fns.push(fns);
        self.registered.push(id);
    }
    pub(crate) fn registered(&self) -> impl Iterator<Item = ComponentId> {
        self.fns.iter().flat_map(|fns| fns.ids).chain(self.child_of)
    }
    /// backup these component ids if they are relevant to relationships
    pub(crate) fn buffer(
        &mut self,
        entity_mut: &mut EntityWorldMut,
        components: &[ComponentId],
        now: NonLogNow,
        buffer_at_now: bool,
    ) -> bool {
        for id in components {
            if let Some(contained) = self.cache_buffer.get_mut(id.index()) {
                *contained = true;
            }
        }

        let mut buffered_any = buffer_relationship_extra::<ChildOf>(
            entity_mut,
            self.child_of,
            &self.cache_buffer,
            now,
            buffer_at_now,
        );

        for f in self.fns.iter() {
            buffered_any = buffered_any
                | (f.buffer)(entity_mut, f.ids, &self.cache_buffer, now, buffer_at_now);
        }

        for id in components {
            if let Some(contained) = self.cache_buffer.get_mut(id.index()) {
                *contained = false;
            }
        }

        buffered_any
    }
    #[track_caller]
    pub(crate) fn try_despawn(
        &mut self,
        entity_mut: &mut EntityWorldMut,
        now: NonLogNow,
    ) -> Result<(), RevEntitiesError> {
        // todo: make &mut self, include cache in self

        let entity = entity_mut.id();

        if entity_mut.is_despawned() {
            let error = entity_mut.world().get_entity(entity).err().unwrap();
            return Err(error.into());
        }
        if let Some(&marker) = entity_mut.get::<DisabledToDespawn>() {
            let error = EntityRevDespawnedError { entity, marker };
            return Err(error.into());
        }

        self.cache_despawn_results.insert(entity, Ok(()));

        // collect entities that should be despawned and errors of entities that are already (reversibly) despawned
        recursive_collect_despawn(
            &self.fns,
            (&*entity_mut).into(),
            entity_mut.world(),
            &mut self.cache_despawn_results,
        );

        entity_mut.world_scope(|world| {
            // buffer relationship components of entities and their non-despawning parents/children if needed
            for f in self.fns.iter() {
                (f.buffer_despawn)(world, now, &self.cache_despawn_results, &mut self.cache_despawn_visited);
                self.cache_despawn_visited.clear();
            }

            let mut error = RevEntitiesError::empty();

            // add DisabledToDespawn to despawning entities
            let despawned_entities = self.cache_despawn_results
                .drain()
                .filter_map(|(entity, result)| match result {
                    Ok(()) => Some(entity),
                    Err(err) => {
                        error.push(err);
                        None
                    }
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();

            let despawned_entities = UndoRedoSwap(Spawn {
                spawned: despawned_entities,
                marker: DisabledToDespawn::for_spawn_despawn(now.0),
            });

            world.redo_and_buffer(now, despawned_entities);

            if !error.is_empty() {
                Err(error)
            } else {
                Ok(())
            }
        })
    }
}

fn buffer_relationship_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components_sparse: &[bool],
    now: NonLogNow,
    buffer_at_now: bool,
) -> bool {
    let mut relationship = None;
    if components_sparse[ids.relationship.index()] && entity_mut.contains_id(ids.relationship) {
        relationship = Some(BufferOneForSingleEntity::<T>::new(entity_mut.id()));
    }

    let mut relationship_target = None;
    if components_sparse[ids.relationship_target.index()] {
        if let Some(children) = entity_mut.get::<T::RelationshipTarget>() {
            let children = children.iter().collect::<Vec<_>>();
            relationship_target = BufferOneForManyEntities::<T>::new_if_not_empty(children);
        }
    }

    buffer_common(
        entity_mut,
        now,
        buffer_at_now,
        relationship,
        relationship_target,
    )
}

fn buffer_relationship_target_extra<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components_sparse: &[bool],
    now: NonLogNow,
    buffer_at_now: bool,
) -> bool {
    let mut relationship = None;
    if components_sparse[ids.relationship.index()] {
        if let Some(parent) = entity_mut.get::<T>().map(T::get) {
            if entity_mut
                .world()
                .get::<T::RelationshipTarget>(parent)
                .is_some_and(|siblings| siblings.len() == 1)
            {
                relationship = Some(BufferOneForSingleEntity::<T::RelationshipTarget>::new(
                    parent,
                ));
            }
        }
    }

    let mut relationship_target = None;
    if components_sparse[ids.relationship_target.index()]
        && entity_mut.contains_id(ids.relationship_target)
    {
        relationship_target = Some(BufferOneForSingleEntity::<T::RelationshipTarget>::new(
            entity_mut.id(),
        ));
    }

    buffer_common(
        entity_mut,
        now,
        buffer_at_now,
        relationship,
        relationship_target,
    )
}

fn buffer_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components_sparse: &[bool],
    now: NonLogNow,
    buffer_at_now: bool,
) -> bool {
    let mut relationship_entities = Vec::new();
    let mut relationship_target_entities = Vec::new();

    if components_sparse[ids.relationship.index()] {
        if let Some(parent) = entity_mut.get::<T>().map(T::get) {
            if entity_mut
                .world()
                .get::<T::RelationshipTarget>(parent)
                .is_some_and(|siblings| siblings.len() == 1)
            {
                relationship_target_entities.push(parent);
            }
            relationship_entities.push(entity_mut.id());
        }
    }

    if components_sparse[ids.relationship_target.index()] {
        if let Some(children) = entity_mut.get::<T::RelationshipTarget>() {
            relationship_entities.extend(children.iter());
            relationship_target_entities.push(entity_mut.id());
        }
    }

    let relationship = BufferOneForManyEntities::<T>::new_if_not_empty(relationship_entities);
    let relationship_target = BufferOneForManyEntities::<T::RelationshipTarget>::new_if_not_empty(
        relationship_target_entities,
    );

    match (relationship, relationship_target) {
        (Some(relationship), Some(relationship_target)) => {
            let buffer = BufferBothForManyEntities::<T, ONE_TO_ONE> {
                relationship,
                relationship_target,
            };
            if buffer_at_now {
                entity_mut.world_scope(|world| world.redo_and_buffer(now, buffer))
            } else {
                entity_mut.buffer_undo_redo(now, UndoRedoSwap(buffer));
            }
            true
        }
        (relationship, relationship_target) => buffer_common(
            entity_mut,
            now,
            buffer_at_now,
            relationship,
            relationship_target,
        ),
    }
}

fn buffer_common(
    entity_mut: &mut EntityWorldMut,
    now: NonLogNow,
    buffer_at_now: bool,
    relationship: Option<impl UndoRedo>,
    relationship_target: Option<impl UndoRedo>,
) -> bool {
    if buffer_at_now {
        // SAFETY: will call update_locations if components are taken here
        let world = unsafe { entity_mut.world_mut() };

        match (relationship, relationship_target) {
            (None, None) => return false,
            (Some(buffer), None) => world.redo_and_buffer(now, buffer),
            (None, Some(buffer)) => world.redo_and_buffer(now, buffer),
            (Some(relationship), Some(relationship_target)) => {
                world.redo_and_buffer(now, relationship);
                world.redo_and_buffer(now, relationship_target);
            }
        }

        entity_mut.update_location();
    } else {
        match (
            relationship.map(UndoRedoSwap),
            relationship_target.map(UndoRedoSwap),
        ) {
            (None, None) => return false,
            (Some(buffer), None) => entity_mut.buffer_undo_redo(now, buffer),
            (None, Some(buffer)) => entity_mut.buffer_undo_redo(now, buffer),
            (Some(relationship), Some(relationship_target)) => {
                entity_mut.buffer_undo_redo(now, relationship);
                entity_mut.buffer_undo_redo(now, relationship_target);
            }
        }
    }

    true
}

fn recursive_collect_despawn(
    fns: &Vec<Fns>,
    entity_ref: EntityRef,
    world: &World,
    results: &mut DespawnResults,
) {
    collect_despawn::<ChildOf>(fns, entity_ref, world, results);
    fns
        .iter()
        .flat_map(|fns| fns.collect_despawn)
        .for_each(|collect_despawn| collect_despawn(fns, entity_ref, world, results));
}

fn collect_despawn<T: Relationship>(
    fns: &Vec<Fns>,
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
                    recursive_collect_despawn(fns, entity_ref, world, results);
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

fn buffer_despawn_relationship_extra<T: Relationship>(
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

    if let Some(buffer) = BufferOneForManyEntities::<T>::new_if_not_empty(relationship_entities) {
        world.redo_and_buffer(now, buffer);
    }
}

fn buffer_despawn_relationship_target_extra<T: Relationship>(
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

    if let Some(buffer) = BufferOneForManyEntities::<T>::new_if_not_empty(relationship_entities) {
        world.redo_and_buffer(now, buffer);
    }

    if let Some(buffer) = BufferOneForManyEntities::<T::RelationshipTarget>::new_if_not_empty(
        relationship_target_entities,
    ) {
        world.redo_and_buffer(now, buffer);
    }
}

fn buffer_despawn_both_extra<T: Relationship, const ONE_TO_ONE: bool>(
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
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };

        if !<T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN {
            if let Some(children) = entity_ref.get::<T::RelationshipTarget>() {
                let previous_relationship_len = relationship_entities.len();
                for child in children.iter() {
                    if !results.contains_key(&child) {
                        relationship_entities.push(child);
                    }
                }
                if relationship_entities.len() - previous_relationship_len == children.len() {
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
                relationship_entities.push(sibling); // entity_ref is part of this
            } else {
                all_siblings_despawn = false;
            }
        }
        if all_siblings_despawn {
            relationship_target_entities.push(parent);
        }
    }

    match (
        BufferOneForManyEntities::<T>::new_if_not_empty(relationship_entities),
        BufferOneForManyEntities::<T::RelationshipTarget>::new_if_not_empty(
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

struct BufferOneForManyEntities<T: Component> {
    values: Vec<T>,
    entities: Box<[Entity]>,
}

impl<T: Component> BufferOneForManyEntities<T> {
    fn new_if_not_empty(entities: Vec<Entity>) -> Option<Self> {
        if entities.is_empty() {
            None
        } else {
            let entities = entities.into_boxed_slice();
            Some(Self {
                values: Vec::with_capacity(entities.len()),
                entities,
            })
        }
    }
    #[inline]
    fn common_undo(&mut self, world: &mut World) {
        // use insert_batch when fixed: https://github.com/bevyengine/bevy/issues/19275
        // world.insert_batch(self.entities.iter().copied().zip(self.values.drain(..)));
        let iter = self.entities.iter().copied().zip(self.values.drain(..));
        for (entity, value) in iter {
            world.entity_mut(entity).insert(value);
        }
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
                // RelationshipTarget::Collection is Entity
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
        assert!(self.value.is_some());
    }
}

#[cfg(test)]
mod test {
    use std::marker::PhantomData;

    use bevy::prelude::*;

    use crate::{panic_on_error_events, prelude::*};

    use super::*;

    #[derive(Component, PartialEq, Debug)]
    #[relationship(relationship_target = ParentMany<Data, DataParent>)]
    struct ChildMany<Data: DSSS, DataParent: DSSS> {
        #[relationship]
        parent: Entity,
        data: Data,
        data_parent: PhantomData<DataParent>,
    }

    #[derive(Component, PartialEq, Debug)]
    #[relationship_target(relationship = ChildMany<DataChild, Data>)]
    struct ParentMany<DataChild: DSSS, Data: DSSS> {
        #[relationship] // EntityHashSet instead of Vec<Entity> so order does not matter for PartialEq
        children: EntityHashSet,
        data: Data,
        data_children: PhantomData<DataChild>,
    }

    #[derive(Component, PartialEq, Debug)]
    #[relationship(relationship_target = ParentSingleBothExtra)]
    struct ChildSingleBothExtra {
        #[relationship]
        parent: Entity,
        data: ExtraData,
    }

    #[derive(Component, PartialEq, Debug)]
    #[relationship_target(relationship = ChildSingleBothExtra)]
    struct ParentSingleBothExtra {
        #[relationship]
        child: Entity,
        data: ExtraData,
    }

    #[derive(Debug, Default, PartialEq)]
    enum ExtraData {
        #[default] // removing Default derive should reveal dead_code lint for this variant
        Default,
        NotDefault,
    }

    trait DSSS: Default + Send + Sync + 'static {}
    impl<T: Default + Send + Sync + 'static> DSSS for T {}

    impl ChildMany<ExtraData, ()> {
        fn relationship_extra(parent: Entity) -> Self {
            Self {
                parent,
                data: ExtraData::NotDefault,
                data_parent: PhantomData,
            }
        }
    }

    impl ChildMany<(), ExtraData> {
        fn relationship_target_extra(parent: Entity) -> Self {
            Self {
                parent,
                data: (),
                data_parent: PhantomData,
            }
        }
    }

    impl ChildMany<ExtraData, ExtraData> {
        fn both_extra(parent: Entity) -> Self {
            Self {
                parent,
                data: ExtraData::NotDefault,
                data_parent: PhantomData,
            }
        }
    }

    impl ChildSingleBothExtra {
        fn new(parent: Entity) -> Self {
            Self { 
                parent, 
                data: ExtraData::NotDefault
            }
        }
    }

    impl ParentMany<ExtraData, ()> {
        fn relationship_extra(children: impl Into<EntityHashSet>) -> Self {
            Self {
                children: children.into(),
                data: (),
                data_children: PhantomData,
            }
        }
    }

    impl ParentMany<(), ExtraData> {
        fn relationship_target_extra(children: impl Into<EntityHashSet>) -> Self {
            Self {
                children: children.into(),
                data: ExtraData::NotDefault,
                data_children: PhantomData,
            }
        }
    }

    impl ParentMany<ExtraData, ExtraData> {
        fn both_extra(children: impl Into<EntityHashSet>) -> Self {
            Self {
                children: children.into(),
                data: ExtraData::NotDefault,
                data_children: PhantomData,
            }
        }
    }

    impl ParentSingleBothExtra {
        fn new(child: Entity) -> Self {
            Self { 
                child, 
                data: ExtraData::NotDefault
            }
        }
    }

    fn setup() -> World {
        panic_on_error_events();

        let mut world = World::new();
        let mut resource = RevRelationship::from_world(&mut world);

        resource.register::<ChildMany<ExtraData, ()>>(&mut world);
        resource.register::<ChildMany<(), ExtraData>>(&mut world);
        resource.register::<ChildMany<ExtraData, ExtraData>>(&mut world);
        resource.register::<ChildSingleBothExtra>(&mut world);

        world.insert_resource(resource);
        world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
        world.init_resource::<UndoRedoBuffer>();

        world
    }

    mod despawn {
        use crate::undo_redo::RevIsDespawned;

        use super::*;

        #[test]
        fn relationship_extra() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let gen1 = world.spawn_empty().id();
            let gen2 = world
                .spawn((ChildMany::relationship_extra(gen1), ChildOf(gen1)))
                .id();
            let gen3 = world
                .spawn((ChildMany::relationship_extra(gen2), ChildOf(gen2)))
                .id();
            let gen4 = world.spawn(ChildMany::relationship_extra(gen3)).id();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(gen2);
                let result = resource.try_despawn(&mut entity, now);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [gen1_ref, gen2_ref, gen3_ref, gen4_ref] = world.entity([gen1, gen2, gen3, gen4]);

            assert_eq!(gen1_ref.rev_is_despawned(), false);
            assert_eq!(gen1_ref.get::<ParentMany<ExtraData, ()>>(), None);

            assert_eq!(gen2_ref.rev_is_despawned(), true);

            assert_eq!(gen3_ref.rev_is_despawned(), true);

            assert_eq!(gen4_ref.rev_is_despawned(), false);
            assert_eq!(gen4_ref.get::<ChildMany<ExtraData, ()>>(), None);

            buffer.undo(&mut world);
            let [gen1_ref, gen2_ref, gen3_ref, gen4_ref] = world.entity([gen1, gen2, gen3, gen4]);

            assert_eq!(gen1_ref.rev_is_despawned(), false);
            assert_eq!(
                gen1_ref.get::<ParentMany<ExtraData, ()>>(),
                Some(&ParentMany::relationship_extra([gen2]))
            );

            assert_eq!(gen2_ref.rev_is_despawned(), false);
            assert_eq!(
                gen2_ref.get::<ChildMany<ExtraData, ()>>(),
                Some(&ChildMany::relationship_extra(gen1))
            );
            assert_eq!(
                gen2_ref.get::<ParentMany<ExtraData, ()>>(),
                Some(&ParentMany::relationship_extra([gen3]))
            );

            assert_eq!(gen3_ref.rev_is_despawned(), false);
            assert_eq!(
                gen3_ref.get::<ChildMany<ExtraData, ()>>(),
                Some(&ChildMany::relationship_extra(gen2))
            );
            assert_eq!(
                gen3_ref.get::<ParentMany<ExtraData, ()>>(),
                Some(&ParentMany::relationship_extra([gen4]))
            );

            assert_eq!(gen4_ref.rev_is_despawned(), false);
            assert_eq!(
                gen4_ref.get::<ChildMany<ExtraData, ()>>(),
                Some(&ChildMany::relationship_extra(gen3))
            );

            buffer.redo(&mut world);
            let [gen1_ref, gen2_ref, gen3_ref, gen4_ref] = world.entity([gen1, gen2, gen3, gen4]);

            assert_eq!(gen1_ref.rev_is_despawned(), false);
            assert_eq!(gen1_ref.get::<ParentMany<ExtraData, ()>>(), None);

            assert_eq!(gen2_ref.rev_is_despawned(), true);

            assert_eq!(gen3_ref.rev_is_despawned(), true);

            assert_eq!(gen4_ref.rev_is_despawned(), false);
            assert_eq!(gen4_ref.get::<ChildMany<ExtraData, ()>>(), None);
        }
    }

    /*
    generelle varianten:
    - Relationship extra
    - RelationshipTarget extra
    - both extra
    - both extra (one-to-one)

    tests:
    - buffer noop (1)
    - buffer
    -- je variant, je buffer_at_now (16)
    - despawn mit ChildOf relationships das sich auf despawn entities beschränkt und nur die varianten testen soll
    -- je variant, je linked spawn (16)

    buffer_despawn_relationship_extra
    - !LINKED_SPAWN: buffer T of non-despawning children
    - buffer T of despawning entity if it has non-despawning parent

    buffer_despawn_relationship_target_extra
    - !LINKED_SPAWN: buffer Target of despawning entity if any child is not despawning
    - buffer T of despawning entity if parent is not despawning and not all siblings despawn
    - buffer Target of not despawning parent if entity and all siblings despawn

    buffer_despawn_both_extra
    - !LINKED_SPAWN: buffer T of children that are not despawning
    - !LINKED_SPAWN: buffer Target of despawning entity if no child is despawning
    - buffer T of despawning entity if parent is not despawning
    - buffer Target of not despawning parent if all children despawn
     */
}
