use std::{
    any::{TypeId, type_name},
    mem::replace,
};

use bevy::{
    ecs::{
        component::{Component, ComponentId},
        entity::{Entity, EntityHashMap, EntityHashSet},
        hierarchy::{ChildOf, Children},
        name::Name,
        relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
        resource::Resource,
        world::{EntityRef, EntityWorldMut, Entry as ComponentEntry, FromWorld, World},
    },
    log::error,
    platform::collections::hash_map::Entry as MapEntry,
};

use crate::{
    meta::NonLogNow,
    prelude::{UndoRedo, UndoRedoSwap},
    undo_redo::{BuffersUndoRedo, DisabledToDespawn, EntityRevDespawnedError, RevEntityWorldMut},
};

use super::{RevEntitiesError, RevEntityError, RevWorld};

// todo: make this sensitive to upcoming changes by https://github.com/bevyengine/bevy/issues/19589

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

type BufferFn = fn(&mut EntityWorldMut, Ids, &[bool], NonLogNow, bool) -> bool;
type CollectDespawnFn = fn(&Vec<Fns>, EntityRef, &World, &mut DespawnResults);
type BufferDespawnFn = fn(&mut World, NonLogNow, &DespawnResults, &mut EntityHashSet, bool);
type DespawnResults = EntityHashMap<Result<(), RevEntityError>>;

impl IntoIterator for Ids {
    type IntoIter = core::array::IntoIter<ComponentId, 2>;
    type Item = ComponentId;
    fn into_iter(self) -> Self::IntoIter {
        [self.relationship, self.relationship_target].into_iter()
    }
}

impl Ids {
    fn min_len(self) -> usize {
        self.relationship
            .index()
            .max(self.relationship_target.index())
            + 1
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
            cache_despawn_visited: EntityHashSet::new(),
        }
    }
}

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
            self.cache_buffer
                .extend(core::iter::repeat_n(false, additional));
        }

        let relationship_target_extra = size_of::<T::RelationshipTarget>()
            > size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let one_to_one = TypeId::of::<Entity>()
            == TypeId::of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let collect_despawn = <T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN
            .then_some(collect_despawn::<T> as CollectDespawnFn);

        let fns = match (relationship_target_extra, one_to_one) {
            (false, _) => Fns {
                ids,
                buffer: buffer_relationship::<T>,
                collect_despawn,
                buffer_despawn: despawn_relationship::<T>,
            },
            (true, false) => Fns {
                ids,
                buffer: buffer_both::<T, false>,
                collect_despawn,
                buffer_despawn: despawn_both::<T, false>,
            },
            (true, true) => Fns {
                ids,
                buffer: buffer_both::<T, true>,
                collect_despawn,
                buffer_despawn: despawn_both::<T, true>,
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
        components: Option<&[ComponentId]>,
        now: NonLogNow,
        at_now: bool,
    ) -> bool {
        self.set_sparse(entity_mut, components, true);

        let mut buffered_any = buffer_relationship::<ChildOf>(
            entity_mut,
            self.child_of,
            &self.cache_buffer,
            now,
            at_now,
        );

        for f in self.fns.iter() {
            buffered_any =
                buffered_any | (f.buffer)(entity_mut, f.ids, &self.cache_buffer, now, at_now);
        }

        self.set_sparse(entity_mut, components, false);

        buffered_any
    }
    fn set_sparse(
        &mut self,
        entity_mut: &mut EntityWorldMut,
        components: Option<&[ComponentId]>,
        value: bool,
    ) {
        match components {
            Some(components) => {
                for id in components {
                    if let Some(contained) = self.cache_buffer.get_mut(id.index()) {
                        *contained = value;
                    }
                }
            }
            None => {
                for id in entity_mut.archetype().components() {
                    if let Some(contained) = self.cache_buffer.get_mut(id.index()) {
                        *contained = value;
                    }
                }
            }
        }
    }
    #[track_caller]
    pub(crate) fn try_despawn(
        &mut self,
        entity_mut: &mut EntityWorldMut,
        now: NonLogNow,
        at_now: bool,
    ) -> Result<(), RevEntitiesError> {
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
            despawn_relationship::<ChildOf>(
                world,
                now,
                &self.cache_despawn_results,
                &mut self.cache_despawn_visited,
                at_now,
            );
            self.cache_despawn_visited.clear();
            for f in self.fns.iter() {
                (f.buffer_despawn)(
                    world,
                    now,
                    &self.cache_despawn_results,
                    &mut self.cache_despawn_visited,
                    at_now,
                );
                self.cache_despawn_visited.clear();
            }

            let mut error = RevEntitiesError::empty();

            // add DisabledToDespawn to despawning entities
            let despawned_entities = self
                .cache_despawn_results
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

            let despawned_entities = Despawn {
                entities: despawned_entities,
                marker: DisabledToDespawn::for_spawn_despawn(now.0),
            };

            if at_now {
                world.redo_and_buffer(now, despawned_entities);
            } else {
                world.buffer_undo_redo(now, UndoRedoSwap(despawned_entities));
            }

            if !error.is_empty() {
                Err(error)
            } else {
                Ok(())
            }
        })
    }
}

fn buffer_relationship<T: Relationship>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components_sparse: &[bool],
    now: NonLogNow,
    at_now: bool,
) -> bool {
    let mut entities = Vec::new();

    if components_sparse[ids.relationship.index()] && entity_mut.contains_id(ids.relationship) {
        entities.push(entity_mut.id());
    }

    if components_sparse[ids.relationship_target.index()] {
        if let Some(children) = entity_mut.get::<T::RelationshipTarget>() {
            entities.extend(children.iter());
        }
    }

    let Some(relationship) = BufferComponent::<T>::new_if_not_empty(entities) else {
        return false;
    };

    if at_now {
        entity_mut.redo_and_buffer(now, relationship);
    } else {
        entity_mut.buffer_undo_redo(now, UndoRedoSwap(relationship));
    }

    true
}

fn buffer_both<T: Relationship, const ONE_TO_ONE: bool>(
    entity_mut: &mut EntityWorldMut,
    ids: Ids,
    components_sparse: &[bool],
    now: NonLogNow,
    at_now: bool,
) -> bool {
    let mut relationship_entities = Vec::new();
    let mut relationship_target_entities = Vec::new();

    if components_sparse[ids.relationship.index()] {
        // todo: this needs T to exist, why could buffer_relationship not see it?
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

    let relationship = BufferComponent::<T>::new_if_not_empty(relationship_entities);
    let relationship_target =
        BufferComponent::<T::RelationshipTarget>::new_if_not_empty(relationship_target_entities);

    let Some(relationship) = relationship else {
        debug_assert!(relationship_target.is_none());
        return false;
    };

    match relationship_target {
        Some(relationship_target) => {
            let buffer = BufferBoth::<T, ONE_TO_ONE> {
                relationship,
                relationship_target,
            };
            if at_now {
                entity_mut.redo_and_buffer(now, buffer);
            } else {
                entity_mut.buffer_undo_redo(now, UndoRedoSwap(buffer));
            }
        }
        None => {
            if at_now {
                entity_mut.redo_and_buffer(now, relationship);
            } else {
                entity_mut.buffer_undo_redo(now, UndoRedoSwap(relationship));
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
    fns.iter()
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
        get_collect_despawn(fns, child, world, results)
    }
}

fn get_collect_despawn(
    fns: &Vec<Fns>,
    entity: Entity,
    world: &World,
    results: &mut DespawnResults,
) {
    let MapEntry::Vacant(vacant) = results.entry(entity) else {
        return;
    };
    match world.get_entity(entity) {
        Ok(entity_ref) => match entity_ref.get::<DisabledToDespawn>() {
            None => {
                vacant.insert(Ok(()));
                recursive_collect_despawn(fns, entity_ref, world, results);
            }
            Some(&marker) => {
                let error = EntityRevDespawnedError {
                    entity: entity,
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

fn despawn_relationship<T: Relationship>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResults,
    visited: &mut EntityHashSet,
    at_now: bool,
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

    if let Some(buffer) = BufferComponent::<T>::new_if_not_empty(relationship_entities) {
        if at_now {
            world.redo_and_buffer(now, buffer);
        } else {
            world.buffer_undo_redo(now, UndoRedoSwap(buffer));
        }
    }
}

fn despawn_both<T: Relationship, const ONE_TO_ONE: bool>(
    world: &mut World,
    now: NonLogNow,
    results: &DespawnResults,
    visited: &mut EntityHashSet,
    at_now: bool,
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

        if ONE_TO_ONE {
            relationship_entities.push(entity);
            relationship_target_entities.push(parent);
        } else {
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
    }

    let Some(relationship) = BufferComponent::<T>::new_if_not_empty(relationship_entities) else {
        debug_assert!(relationship_target_entities.is_empty());
        return;
    };

    match BufferComponent::<T::RelationshipTarget>::new_if_not_empty(relationship_target_entities) {
        None if at_now => world.redo_and_buffer(now, relationship),
        None => world.buffer_undo_redo(now, UndoRedoSwap(relationship)),
        Some(relationship_target) => {
            let buffer = BufferBoth::<T, ONE_TO_ONE> {
                relationship,
                relationship_target,
            };
            if at_now {
                world.redo_and_buffer(now, buffer)
            } else {
                world.buffer_undo_redo(now, UndoRedoSwap(buffer));
            }
        }
    }
}

struct BufferComponent<T: Component> {
    values: Vec<T>,
    entities: Box<[Entity]>,
}

impl<T: Component> BufferComponent<T> {
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
}

impl<T: Component> UndoRedo for BufferComponent<T> {
    fn undo(&mut self, world: &mut World) {
        let iter = self
            .entities
            .iter()
            .copied()
            .zip(self.values.drain(..))
            .rev();
        for (entity, value) in iter {
            match world.get_entity_mut(entity) {
                Ok(mut entity_mut) => match entity_mut.entry::<T>() {
                    ComponentEntry::Vacant(vacant) => {
                        vacant.insert(value);
                    }
                    ComponentEntry::Occupied(mut occupied) => {
                        occupied.insert(value);
                        match entity_mut.get::<Name>() {
                            Some(name) => error!(
                                "inserted relationship component {} into \"{name}\" ({entity}) but this overwrote an existing value, this is irreversible, make sure to only insert this value with reversible commands",
                                type_name::<T>()
                            ),
                            None => error!(
                                "inserted relationship component {} into {entity} but this overwrote an existing value, this is irreversible, make sure to only insert this value with reversible commands",
                                type_name::<T>()
                            ),
                        }
                    }
                },
                Err(err) => {
                    panic!(
                        "could not insert relationship component {}: {err}",
                        type_name::<T>()
                    )
                }
            }
        }
    }
    fn redo(&mut self, world: &mut World) {
        let values = self
            .entities
            .iter()
            .map(|&entity| {
                match world.get_entity_mut(entity) {
                    Ok(mut entity_mut) => match entity_mut.take::<T>() {
                        Some(component) => component,
                        None => {
                            match entity_mut.get::<Name>() {
                                Some(name) => panic!(
                                    "could not take relationship component {}: entity \"{name}\" ({entity}) does not contain it anymore",
                                    type_name::<T>()
                                ),
                                None => panic!(
                                    "could not take relationship component {}: entity {entity} does not contain it anymore",
                                    type_name::<T>()
                                )
                            }
                        }
                    }
                    Err(err) => {
                        panic!("could not take relationship component {}: {err}", type_name::<T>())
                    },
                }
            });
        self.values.extend(values);
    }
}

struct BufferBoth<T: Relationship, const ONE_TO_ONE: bool> {
    relationship: BufferComponent<T>,
    relationship_target: BufferComponent<T::RelationshipTarget>,
}

impl<T: Relationship, const ONE_TO_ONE: bool> UndoRedo for BufferBoth<T, ONE_TO_ONE> {
    fn undo(&mut self, world: &mut World) {
        self.relationship_target.undo(world);
        self.relationship.undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        // todo: more informative panics with Name/Entity
        if ONE_TO_ONE {
            let iter = self.relationship_target.entities.iter().map(|entity| {
                // RelationshipTarget::Collection is Entity
                let value = &mut *world
                    .get_mut::<T::RelationshipTarget>(*entity)
                    .expect("todo");
                let collection = value.collection_mut_risky();
                let mut replacement =
                    <T::RelationshipTarget as RelationshipTarget>::Collection::new();
                let child = collection.iter().next().expect("todo");
                replacement.add(child);
                collection.clear();
                let replacement = T::RelationshipTarget::from_collection_risky(replacement);
                replace(value, replacement) //T::Relationship remove hooks/observers can no longer see data -> todo: open issue
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

            // buffering T will now not remove T::Relationship because with Entity::PLACEHOLDER it is not empty
            self.relationship.redo(world);

            let values = self.relationship_target.entities.iter().map(|entity| {
                let mut entity = world.entity_mut(*entity);
                let ComponentEntry::Occupied(mut value) = entity.entry::<T::RelationshipTarget>()
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

pub(super) struct Despawn {
    pub(super) entities: Box<[Entity]>,
    pub(super) marker: DisabledToDespawn,
}

impl UndoRedo for Despawn {
    fn undo(&mut self, world: &mut World) {
        let id = world.component_id::<DisabledToDespawn>().expect("todo");
        for entity in self.entities.iter().copied() {
            world.entity_mut(entity).remove_by_id(id);
        }
    }
    fn redo(&mut self, world: &mut World) {
        world.insert_batch(
            self.entities
                .iter()
                .copied()
                .rev()
                .map(|entity| (entity, self.marker)),
        );
    }
}

#[cfg(test)]
mod test {
    use std::{fmt::Debug, marker::PhantomData};

    use bevy::ecs::component::Mutable;
    use bevy::prelude::*;

    use crate::{panic_on_error_events, prelude::*};

    use super::*;

    #[derive(Component, PartialEq, Debug)]
    #[relationship(relationship_target = Parent)]
    struct Child<Parent: RelationshipTarget<Relationship = Self>> {
        #[relationship]
        parent: Entity,
        data: ExtraData,
        parent_type: PhantomData<Parent>,
    }

    #[derive(Component, PartialEq, Debug)]
    #[relationship_target(relationship = Child<Self>)]
    struct PlainParent {
        #[relationship]
        // EntityHashSet instead of Vec<Entity> so order does not matter for PartialEq
        children: EntityHashSet,
    }

    #[derive(Component, PartialEq, Debug)]
    #[relationship_target(relationship = Child<Self>)]
    struct DataParent {
        #[relationship]
        // EntityHashSet instead of Vec<Entity> so order does not matter for PartialEq
        children: EntityHashSet,
        data: ExtraData,
    }

    #[derive(Component, PartialEq, Debug)]
    #[relationship_target(relationship = Child<Self>)]
    struct DataSingleParent {
        #[relationship]
        child: Entity,
        data: ExtraData,
    }

    trait RelationshipSetup: Component + Debug + PartialEq {
        fn new_single(entity: Entity) -> Self;
        fn set_non_default(&mut self);
    }

    impl<Parent: RelationshipTarget<Relationship = Self> + Debug + PartialEq> RelationshipSetup
        for Child<Parent>
    {
        fn new_single(parent: Entity) -> Self {
            Self {
                parent,
                data: ExtraData::NotDefault,
                parent_type: PhantomData,
            }
        }
        fn set_non_default(&mut self) {
            self.data = ExtraData::NotDefault;
        }
    }

    impl PlainParent {
        fn new_many(children: impl Into<EntityHashSet>) -> Self {
            Self {
                children: children.into(),
            }
        }
    }

    impl RelationshipSetup for PlainParent {
        fn new_single(entity: Entity) -> Self {
            Self::new_many([entity])
        }
        fn set_non_default(&mut self) {}
    }

    impl DataParent {
        fn new_many(children: impl Into<EntityHashSet>) -> Self {
            Self {
                children: children.into(),
                data: ExtraData::NotDefault,
            }
        }
    }

    impl RelationshipSetup for DataParent {
        fn new_single(entity: Entity) -> Self {
            Self::new_many([entity])
        }
        fn set_non_default(&mut self) {
            self.data = ExtraData::NotDefault;
        }
    }

    impl RelationshipSetup for DataSingleParent {
        fn new_single(child: Entity) -> Self {
            Self {
                child,
                data: ExtraData::NotDefault,
            }
        }
        fn set_non_default(&mut self) {
            self.data = ExtraData::NotDefault;
        }
    }

    impl RelationshipSetup for ChildOf {
        fn new_single(entity: Entity) -> Self {
            ChildOf(entity)
        }
        fn set_non_default(&mut self) {}
    }

    impl RelationshipSetup for Children {
        fn new_single(entity: Entity) -> Self {
            Children::from_collection_risky(vec![entity])
        }
        fn set_non_default(&mut self) {}
    }

    #[derive(Debug, Default, PartialEq)]
    enum ExtraData {
        /// The tests are based on asserting this variant is not constructed by Bevy and
        /// instead the original NotDefault value is preserved.
        /// Removing the Default derive should result in the dead_code lint for this variant.
        #[default]
        Default,
        NotDefault,
    }

    fn setup() -> World {
        panic_on_error_events();

        let mut world = World::new();
        let mut resource = RevRelationship::from_world(&mut world);

        resource.register::<Child<PlainParent>>(&mut world);
        resource.register::<Child<DataParent>>(&mut world);
        resource.register::<Child<DataSingleParent>>(&mut world);

        world.insert_resource(resource);
        world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
        world.init_resource::<UndoRedoBuffer>();

        world
    }

    mod buffer {
        use super::*;

        #[test]
        fn none() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let entity = world.spawn_empty().id();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity_mut = world.entity_mut(entity);

                let buffered_any = resource.buffer(&mut entity_mut, Some(&[]), now, true);
                assert_eq!(buffered_any, false);

                let buffered_any = resource.buffer(&mut entity_mut, Some(&[]), now, false);
                assert_eq!(buffered_any, false);
            });

            let buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            assert!(buffer.is_empty());
        }

        #[test]
        fn relationship() {
            generic::<Child<PlainParent>, PlainParent>();
        }

        #[test]
        fn child_of_and_children() {
            generic::<ChildOf, Children>();
        }

        #[test]
        fn both() {
            generic::<Child<DataParent>, DataParent>();
        }

        #[test]
        fn both_one_to_one() {
            generic::<Child<DataSingleParent>, DataSingleParent>();
        }

        fn generic<Child: RelationshipSetup, Parent: RelationshipSetup<Mutability = Mutable>>() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let old_parent = world.spawn(Name::new("old_parent")).id();
            let new_parent = world.spawn(Name::new("new_parent")).id();
            let entity = world
                .spawn((Name::new("entity"), Child::new_single(old_parent)))
                .id();
            let old_child = world
                .spawn((Name::new("old_child"), Child::new_single(entity)))
                .id();

            let [mut old_parent_mut, mut entity_mut] = world.entity_mut([old_parent, entity]);

            old_parent_mut
                .get_mut::<Parent>()
                .unwrap()
                .set_non_default();
            entity_mut.get_mut::<Parent>().unwrap().set_non_default();

            let relationship = world.register_component::<Child>();
            let relationship_target = world.register_component::<Parent>();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity_mut = world.entity_mut(entity);

                let buffered_any = resource.buffer(
                    &mut entity_mut,
                    Some(&[
                        relationship,        // buffer `Child::new_single(old_parent)` of "entity"
                        relationship_target, // indirectly buffer `Child::new_single(entity)` of "old_child"
                    ]),
                    now,
                    true,
                );
                assert_eq!(buffered_any, true);

                entity_mut.insert(Child::new_single(new_parent));
                entity_mut.world_scope(|world| {
                    world
                        .entity_mut(new_parent)
                        .get_mut::<Parent>()
                        .unwrap()
                        .set_non_default()
                });

                let buffered_any = resource.buffer(
                    &mut entity_mut,
                    Some(&[
                        relationship, // buffer `Child::new_single(new_parent)` of "entity" at undo
                    ]),
                    now,
                    false,
                );
                assert_eq!(buffered_any, true);
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [old_parent_ref, new_parent_ref, entity_ref, old_child_ref] =
                world.entity([old_parent, new_parent, entity, old_child]);

            assert_eq!(old_parent_ref.get::<Parent>(), None);
            assert_eq!(
                new_parent_ref.get::<Parent>(),
                Some(&Parent::new_single(entity))
            );
            assert_eq!(
                entity_ref.get::<Child>(),
                Some(&Child::new_single(new_parent))
            );
            assert_eq!(entity_ref.get::<Parent>(), None);
            assert_eq!(old_child_ref.get::<Child>(), None);

            buffer.undo(&mut world);
            let [old_parent_ref, new_parent_ref, entity_ref, old_child_ref] =
                world.entity([old_parent, new_parent, entity, old_child]);

            assert_eq!(
                old_parent_ref.get::<Parent>(),
                Some(&Parent::new_single(entity))
            );
            assert_eq!(new_parent_ref.get::<Parent>(), None);
            assert_eq!(
                entity_ref.get::<Child>(),
                Some(&Child::new_single(old_parent))
            );
            assert_eq!(
                entity_ref.get::<Parent>(),
                Some(&Parent::new_single(old_child))
            );
            assert_eq!(
                old_child_ref.get::<Child>(),
                Some(&Child::new_single(entity))
            );

            buffer.redo(&mut world);
            let [old_parent_ref, new_parent_ref, entity_ref, old_child_ref] =
                world.entity([old_parent, new_parent, entity, old_child]);

            assert_eq!(old_parent_ref.get::<Parent>(), None);
            assert_eq!(
                new_parent_ref.get::<Parent>(),
                Some(&Parent::new_single(entity))
            );
            assert_eq!(
                entity_ref.get::<Child>(),
                Some(&Child::new_single(new_parent))
            );
            assert_eq!(entity_ref.get::<Parent>(), None);
            assert_eq!(old_child_ref.get::<Child>(), None);
        }
    }

    mod despawn {
        use super::*;

        #[test]
        fn non_linked_relationship() {
            non_linked_generic::<Child<PlainParent>, PlainParent>();
        }

        #[test]
        fn child_of_and_children() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let parent = world.spawn(Name::new("parent")).id();
            let entity = world.spawn((Name::new("entity"), ChildOf(parent))).id();
            let child = world.spawn((Name::new("child"), ChildOf(entity))).id();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(entity);
                let result = resource.try_despawn(&mut entity, now, true);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(parent_ref.get::<Children>(), None);

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(child_ref.rev_is_despawned(), true);

            buffer.undo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<Children>(),
                Some(&Children::from_collection_risky(vec![entity]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(entity_ref.get::<ChildOf>(), Some(&ChildOf(parent)));
            assert_eq!(
                entity_ref.get::<Children>(),
                Some(&Children::from_collection_risky(vec![child]))
            );

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<ChildOf>(), Some(&ChildOf(entity)));

            buffer.redo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(parent_ref.get::<Children>(), None);

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(child_ref.rev_is_despawned(), true);
        }

        #[test]
        fn non_linked_both() {
            non_linked_generic::<Child<DataParent>, DataParent>();
        }

        #[test]
        fn non_linked_both_one_to_one() {
            non_linked_generic::<Child<DataSingleParent>, DataSingleParent>();
        }

        fn non_linked_generic<
            Child: RelationshipSetup,
            Parent: RelationshipSetup<Mutability = Mutable>,
        >() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let parent = world.spawn(Name::new("parent")).id();
            let entity = world
                .spawn((Name::new("entity"), Child::new_single(parent)))
                .id();
            let child = world
                .spawn((Name::new("child"), Child::new_single(entity)))
                .id();

            let [mut parent_mut, mut entity_mut] = world.entity_mut([parent, entity]);

            parent_mut.get_mut::<Parent>().unwrap().set_non_default();
            entity_mut.get_mut::<Parent>().unwrap().set_non_default();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(entity);
                let result = resource.try_despawn(&mut entity, now, true);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(parent_ref.get::<Parent>(), None);

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<Child>(), None);

            buffer.undo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<Parent>(),
                Some(&Parent::new_single(entity))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(entity_ref.get::<Child>(), Some(&Child::new_single(parent)));
            assert_eq!(entity_ref.get::<Parent>(), Some(&Parent::new_single(child)));

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<Child>(), Some(&Child::new_single(entity)));

            buffer.redo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(parent_ref.get::<Parent>(), None);

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<Child>(), None);
        }

        #[test]
        fn non_linked_both_staying_sibling() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();

            let parent = world.spawn(Name::new("parent")).id();
            let entity = world
                .spawn((Name::new("entity"), Child::<DataParent>::new_single(parent)))
                .id();
            let sibling = world
                .spawn((
                    Name::new("sibling"),
                    Child::<DataParent>::new_single(parent),
                ))
                .id();

            world
                .entity_mut(parent)
                .get_mut::<DataParent>()
                .unwrap()
                .data = ExtraData::NotDefault;

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(entity);
                let result = resource.try_despawn(&mut entity, now, true);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [parent_ref, entity_ref, sibling_ref] = world.entity([parent, entity, sibling]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<DataParent>(),
                Some(&DataParent::new_many([sibling]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(sibling_ref.rev_is_despawned(), false);
            assert_eq!(
                sibling_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );

            buffer.undo(&mut world);
            let [parent_ref, entity_ref, sibling_ref] = world.entity([parent, entity, sibling]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<DataParent>(),
                Some(&DataParent::new_many([entity, sibling]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(
                entity_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );

            assert_eq!(sibling_ref.rev_is_despawned(), false);
            assert_eq!(
                sibling_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );

            buffer.redo(&mut world);
            let [parent_ref, entity_ref, sibling_ref] = world.entity([parent, entity, sibling]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<DataParent>(),
                Some(&DataParent::new_many([sibling]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(sibling_ref.rev_is_despawned(), false);
            assert_eq!(
                sibling_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );
        }
    }

    mod spawn {
        use super::*;

        #[test]
        fn non_linked_relationship() {
            non_linked_generic::<Child<PlainParent>, PlainParent>();
        }

        #[test]
        fn child_of_and_children() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let parent = world.spawn(Name::new("parent")).id();
            let entity = world.spawn((Name::new("entity"), ChildOf(parent))).id();
            let child = world.spawn((Name::new("child"), ChildOf(entity))).id();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(entity);
                let result = resource.try_despawn(&mut entity, now, false);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<Children>(),
                Some(&Children::from_collection_risky(vec![entity]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(entity_ref.get::<ChildOf>(), Some(&ChildOf(parent)));
            assert_eq!(
                entity_ref.get::<Children>(),
                Some(&Children::from_collection_risky(vec![child]))
            );

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<ChildOf>(), Some(&ChildOf(entity)));

            buffer.undo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(parent_ref.get::<Children>(), None);

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(child_ref.rev_is_despawned(), true);

            buffer.redo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<Children>(),
                Some(&Children::from_collection_risky(vec![entity]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(entity_ref.get::<ChildOf>(), Some(&ChildOf(parent)));
            assert_eq!(
                entity_ref.get::<Children>(),
                Some(&Children::from_collection_risky(vec![child]))
            );

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<ChildOf>(), Some(&ChildOf(entity)));
        }

        #[test]
        fn non_linked_both() {
            non_linked_generic::<Child<DataParent>, DataParent>();
        }

        #[test]
        fn non_linked_both_one_to_one() {
            non_linked_generic::<Child<DataSingleParent>, DataSingleParent>();
        }

        fn non_linked_generic<
            Child: RelationshipSetup,
            Parent: RelationshipSetup<Mutability = Mutable>,
        >() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();
            let parent = world.spawn(Name::new("parent")).id();
            let entity = world
                .spawn((Name::new("entity"), Child::new_single(parent)))
                .id();
            let child = world
                .spawn((Name::new("child"), Child::new_single(entity)))
                .id();

            let [mut parent_mut, mut entity_mut] = world.entity_mut([parent, entity]);

            parent_mut.get_mut::<Parent>().unwrap().set_non_default();
            entity_mut.get_mut::<Parent>().unwrap().set_non_default();

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(entity);
                let result = resource.try_despawn(&mut entity, now, false);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<Parent>(),
                Some(&Parent::new_single(entity))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(entity_ref.get::<Child>(), Some(&Child::new_single(parent)));
            assert_eq!(entity_ref.get::<Parent>(), Some(&Parent::new_single(child)));

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<Child>(), Some(&Child::new_single(entity)));

            buffer.undo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(parent_ref.get::<Parent>(), None);

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<Child>(), None);

            buffer.redo(&mut world);
            let [parent_ref, entity_ref, child_ref] = world.entity([parent, entity, child]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<Parent>(),
                Some(&Parent::new_single(entity))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(entity_ref.get::<Child>(), Some(&Child::new_single(parent)));
            assert_eq!(entity_ref.get::<Parent>(), Some(&Parent::new_single(child)));

            assert_eq!(child_ref.rev_is_despawned(), false);
            assert_eq!(child_ref.get::<Child>(), Some(&Child::new_single(entity)));
        }

        #[test]
        fn non_linked_both_staying_sibling() {
            let mut world = setup();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();

            let parent = world.spawn(Name::new("parent")).id();
            let entity = world
                .spawn((Name::new("entity"), Child::<DataParent>::new_single(parent)))
                .id();
            let sibling = world
                .spawn((
                    Name::new("sibling"),
                    Child::<DataParent>::new_single(parent),
                ))
                .id();

            world
                .entity_mut(parent)
                .get_mut::<DataParent>()
                .unwrap()
                .data = ExtraData::NotDefault;

            world.resource_scope::<RevRelationship, _>(|world, mut resource| {
                let mut entity = world.entity_mut(entity);
                let result = resource.try_despawn(&mut entity, now, true);
                assert_eq!(result, Ok(()));
            });

            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            let [parent_ref, entity_ref, sibling_ref] = world.entity([parent, entity, sibling]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<DataParent>(),
                Some(&DataParent::new_many([sibling]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(sibling_ref.rev_is_despawned(), false);
            assert_eq!(
                sibling_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );

            buffer.undo(&mut world);
            let [parent_ref, entity_ref, sibling_ref] = world.entity([parent, entity, sibling]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<DataParent>(),
                Some(&DataParent::new_many([entity, sibling]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), false);
            assert_eq!(
                entity_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );

            assert_eq!(sibling_ref.rev_is_despawned(), false);
            assert_eq!(
                sibling_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );

            buffer.redo(&mut world);
            let [parent_ref, entity_ref, sibling_ref] = world.entity([parent, entity, sibling]);

            assert_eq!(parent_ref.rev_is_despawned(), false);
            assert_eq!(
                parent_ref.get::<DataParent>(),
                Some(&DataParent::new_many([sibling]))
            );

            assert_eq!(entity_ref.rev_is_despawned(), true);

            assert_eq!(sibling_ref.rev_is_despawned(), false);
            assert_eq!(
                sibling_ref.get::<Child<DataParent>>(),
                Some(&Child::<DataParent>::new_single(parent))
            );
        }
    }
}
