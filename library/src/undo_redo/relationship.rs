use std::{any::TypeId, marker::PhantomData, mem::replace, sync::Arc};

use bevy::{
    ecs::{
        change_detection::MaybeLocation,
        component::{Component, ComponentId},
        entity::{Entity, EntityHashMap},
        relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget},
        resource::Resource,
        world::{EntityWorldMut, World},
    },
    platform::collections::HashMap,
};

use crate::{meta::NonLogNow, prelude::UndoRedo, undo_redo::DisabledToDespawn};

use super::{BufferAt, RevEntitiesError, RevEntityError, RevWorld};

struct RelationshipBuffer<
    Values,
    Entities,
    T,
    const BOTH_EXTRA: bool, // is `true` if both relationship components have extra fields not relevant to relationships
    const ONE_TO_ONE: bool, // is `true` if T is part of a 1-to-1 relationship
> {
    values: Values,
    entities: Entities,
    _p: PhantomData<T>,
}

trait DrainExtend<T> {
    fn drain(&mut self) -> impl Iterator<Item = T>;
    fn extend(&mut self, values: impl IntoIterator<Item = T>);
}

impl<T> DrainExtend<T> for Option<T> {
    #[inline]
    fn drain(&mut self) -> impl Iterator<Item = T> {
        self.take().into_iter()
    }
    #[inline]
    fn extend(&mut self, values: impl IntoIterator<Item = T>) {
        let mut iter = values.into_iter();
        *self = iter.next();
        debug_assert!(iter.next().is_none())
    }
}

impl<T> DrainExtend<T> for Vec<T> {
    #[inline]
    fn drain(&mut self) -> impl Iterator<Item = T> {
        self.drain(..)
    }
    #[inline]
    fn extend(&mut self, values: impl IntoIterator<Item = T>) {
        <Vec<T> as Extend<T>>::extend(self, values);
    }
}

impl<Values, Entities, T, const BOTH_EXTRA: bool, const ONE_TO_ONE: bool>
    RelationshipBuffer<Values, Entities, T, BOTH_EXTRA, ONE_TO_ONE>
where
    Values: DrainExtend<T> + Send + 'static,
    Entities: Send + 'static,
    T: Component,
    for<'a> &'a Entities: Iterator<Item = &'a Entity>,
{
    fn common_undo(&mut self, world: &mut World) {
        world.insert_batch(self.entities.into_iter().copied().zip(self.values.drain()));
    }
}

impl<Values, Entities, T, const ONE_TO_ONE: bool> UndoRedo
    for RelationshipBuffer<Values, Entities, T, false, ONE_TO_ONE>
where
    Values: DrainExtend<T> + Send + 'static,
    Entities: Send + 'static,
    T: Component,
    for<'a> &'a Entities: Iterator<Item = &'a Entity>,
{
    fn undo(&mut self, world: &mut World) {
        self.common_undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        let values = self.entities.into_iter().copied().map(|entity| {
            let mut entity = world.entity_mut(entity);
            entity.take::<T>().expect("todo")
        });
        self.values.extend(values);
    }
}

impl<Values, Entities, T, Other> UndoRedo
    for (Other, RelationshipBuffer<Values, Entities, T, true, false>)
where
    Other: UndoRedo,
    Values: DrainExtend<T> + Send + 'static,
    Entities: Send + 'static,
    T: RelationshipTarget,
    for<'a> &'a Entities: Iterator<Item = &'a Entity>,
{
    fn undo(&mut self, world: &mut World) {
        self.1.common_undo(world);
        self.0.undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        for entity in self.1.entities.into_iter().copied() {
            world
                .get_mut::<T>(entity)
                .expect("todo")
                .collection_mut_risky()
                .add(Entity::PLACEHOLDER);
        }

        // buffering T::Relationship will now not remove T because with Entity::PLACEHOLDER it is not empty
        self.0.redo(world);

        let values = self.1.entities.into_iter().copied().map(|entity| {
            let mut entity = world.entity_mut(entity);
            let bevy::ecs::world::Entry::Occupied(mut value) = entity.entry::<T>() else {
                panic!("todo");
            };
            // remove Entity::PLACEHOLDER before taking to make this trickery unnoticable by T's hooks
            value
                .get_mut()
                .collection_mut_risky()
                .remove(Entity::PLACEHOLDER);
            value.take()
        });
        self.1.values.extend(values);
    }
}

impl<Values, Entities, T, Other> UndoRedo
    for (Other, RelationshipBuffer<Values, Entities, T, true, true>)
where
    Other: UndoRedo,
    Values: DrainExtend<T> + Send + 'static,
    Entities: Send + 'static,
    T: RelationshipTarget,
    for<'a> &'a Entities: Iterator<Item = &'a Entity>,
{
    fn undo(&mut self, world: &mut World) {
        self.1.common_undo(world);
        self.0.undo(world);
    }
    fn redo(&mut self, world: &mut World) {
        for entity in self.1.entities.into_iter().copied() {
            // T::Collection is Entity
            let mut entity_mut = world.entity_mut(entity);
            let value = &mut *entity_mut.get_mut::<T>().expect("todo");
            let mut replacement = T::Collection::new();
            replacement.add(value.collection().iter().next().expect("todo"));
            let replacement = T::from_collection_risky(replacement);
            self.1.values.extend([replace(value, replacement)]);
        }

        self.0.redo(world);
    }
}

#[derive(Default, Clone, Resource)]
pub(crate) struct RevRelationship {
    fns: Arc<HashMap<ComponentId, RelationshipFns>>,
}

impl RevRelationship {
    pub(crate) fn register<T: Relationship>(
        &mut self,
        relationship: ComponentId,
        relationship_target: ComponentId,
    ) {
        let linked_spawn = <T::RelationshipTarget as RelationshipTarget>::LINKED_SPAWN;
        let relationship_extra = size_of::<T>() > size_of::<Entity>();
        let relationship_target_extra = size_of::<T::RelationshipTarget>()
            > size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let one_to_one = TypeId::of::<Entity>()
            == TypeId::of::<<T::RelationshipTarget as RelationshipTarget>::Collection>();
        let (relationship_fns, relationship_target_fns) = match (
            linked_spawn,
            relationship_extra,
            relationship_target_extra,
            one_to_one,
        ) {
            (false, _, false, _) => RelationshipFns::relationship_extra::<T>(),
            (false, false, true, _) => RelationshipFns::relationship_target_extra::<T>(),
            (false, true, true, false) => RelationshipFns::both_extra::<T, false>(),
            (false, true, true, true) => RelationshipFns::both_extra::<T, true>(),
            (true, _, false, _) => RelationshipFns::linked_spawn_relationship_extra::<T>(),
            (true, false, true, _) => {
                RelationshipFns::linked_spawn_relationship_target_extra::<T>()
            }
            (true, true, true, false) => RelationshipFns::linked_spawn_both_extra::<T, false>(),
            (true, true, true, true) => RelationshipFns::linked_spawn_both_extra::<T, true>(),
        };
        let fns = Arc::get_mut(&mut self.fns).expect("todo");
        fns.insert(relationship, relationship_fns);
        fns.insert(relationship_target, relationship_target_fns);
    }
    /// backup these component ids if they are relevant to relationships
    pub(crate) fn pre_insert(
        &self,
        entity: &mut EntityWorldMut,
        components: &[ComponentId],
        now: NonLogNow,
        at: BufferAt,
    ) {
        for component_id in components {
            if let Some(fns) = self.fns.get(component_id) {
                (fns.pre_insert)(entity, now, at);
            }
        }
    }
    #[track_caller]
    pub(crate) fn try_despawn(
        &self,
        entity: &mut EntityWorldMut,
        now: NonLogNow,
    ) -> Result<(), RevEntitiesError> {
        let components = entity.archetype().components().collect::<Box<_>>();
        let mut entities = EntityHashMap::from([(entity.id(), Ok(true))]);
        let mut ok = true;
        for component_id in components {
            if let Some(fns) = self.fns.get(&component_id) {
                ok = ok & (fns.despawn)(entity, now, &mut entities);
            }
        }
        if !ok {
            let mut error = RevEntitiesError {
                invalid: Vec::new(),
                rev_despawned: Vec::new(),
                rev_despawned_buffers: MaybeLocation::new_with(|| Vec::new()),
            };
            for (_, result) in entities.into_iter() {
                if let Err(err) = result {
                    error.push(err)
                }
            }
            return Err(error);
        }

        let marker = DisabledToDespawn::for_spawn_despawn(now.0);
        entity.world_scope(|world| {
            // rev_try_insert_batch_if_new is not needed as rev_try_insert_batch already skips entities that contain DisabledToDespawn
            world.rev_try_insert_batch(
                now,
                entities.into_iter().filter_map(|(entity, result)| {
                    (result == Ok(true)).then_some((entity, marker))
                }),
            )
        })
    }
}

type PreInsert = fn(&mut EntityWorldMut, NonLogNow, BufferAt);
type Despawn =
    fn(&mut EntityWorldMut, NonLogNow, &mut EntityHashMap<Result<bool, RevEntityError>>) -> bool;

struct RelationshipFns {
    pre_insert: PreInsert,
    despawn: Despawn,
}

impl RelationshipFns {
    fn relationship_extra<T: Relationship>() -> (Self, Self) {
        let (relationship_pre_insert, relationship_target_pre_insert) =
            Self::pre_insert_relationship_extra::<T>();
        todo!()
    }
    fn relationship_target_extra<T: Relationship>() -> (Self, Self) {
        let (relationship_pre_insert, relationship_target_pre_insert) =
            Self::pre_insert_relationship_target_extra::<T>();
        todo!()
    }
    fn both_extra<T: Relationship, const ONE_TO_ONE: bool>() -> (Self, Self) {
        let (relationship_pre_insert, relationship_target_pre_insert) =
            Self::pre_insert_both_extra::<T, ONE_TO_ONE>();
        todo!()
    }
    fn linked_spawn_relationship_extra<T: Relationship>() -> (Self, Self) {
        let (relationship_pre_insert, relationship_target_pre_insert) =
            Self::pre_insert_relationship_extra::<T>();
        todo!()
    }
    fn linked_spawn_relationship_target_extra<T: Relationship>() -> (Self, Self) {
        let (relationship_pre_insert, relationship_target_pre_insert) =
            Self::pre_insert_relationship_target_extra::<T>();
        todo!()
    }
    fn linked_spawn_both_extra<T: Relationship, const ONE_TO_ONE: bool>() -> (Self, Self) {
        let (relationship_pre_insert, relationship_target_pre_insert) =
            Self::pre_insert_both_extra::<T, ONE_TO_ONE>();
        todo!()
    }
    fn pre_insert_relationship_extra<T: Relationship>() -> (PreInsert, PreInsert) {
        todo!()
    }
    fn pre_insert_relationship_target_extra<T: Relationship>() -> (PreInsert, PreInsert) {
        todo!()
    }
    fn pre_insert_both_extra<T: Relationship, const ONE_TO_ONE: bool>() -> (PreInsert, PreInsert) {
        todo!()
    }
}
