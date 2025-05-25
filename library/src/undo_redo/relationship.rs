use std::any::type_name;

use bevy::{ecs::{
    component::Component,
    entity::{Entity, EntityHashMap},
    relationship::{Relationship, RelationshipTarget},
    world::{EntityWorldMut, World},
}, log::error};

use crate::{
    meta::NonLogNow,
    prelude::UndoRedo,
    undo_redo::{BuffersUndoRedo, DisabledToDespawn},
};

// todo: test cyclic relationships

fn despawn_specialized<T: Relationship>() -> fn(&mut EntityWorldMut, DisabledToDespawn) {
    if !T::RelationshipTarget::LINKED_SPAWN {
        return despawn_not_linked::<T>;
    }
    if size_of::<T::RelationshipTarget>()
        == size_of::<<T::RelationshipTarget as RelationshipTarget>::Collection>()
    {
        return despawn_linked_store_relationship::<T>;
    }
    if size_of::<T>() == size_of::<Entity>() {
        return despawn_linked_store_relationship_target::<T>;
    }
    error!(
        "both {0} and {1} contain extra fields, reversibly adding and removing only supports up to
        one of such types, falling back to storing only {0} in non-entity buffers and restoring {1}
        with the default values, potentially losing different, previous values",
        type_name::<T>(), type_name::<T::RelationshipTarget>()
    );
    despawn_linked_store_relationship::<T>
}

// todo: buffer T at child or Target at parent (if needed)
fn despawn_not_linked<T: Relationship>(entity: &mut EntityWorldMut, marker: DisabledToDespawn) {
    let now = NonLogNow(marker.added_frame());
    let id = entity.id();

    if !entity.contains::<DisabledToDespawn>() {
        entity.insert(marker);
    }

    if let Some(relationship) = entity.take::<T>() {
        entity.buffer_undo_redo(
            now,
            BufferSingle {
                entity: id,
                value: Some(relationship),
                marker,
            },
        );
    }

    if let Some(target) = entity.take::<T::RelationshipTarget>() {
        entity.buffer_undo_redo(
            now,
            BufferSingle {
                entity: id,
                value: Some(target),
                marker,
            },
        );
    }
}

fn despawn_linked_store_relationship<T: Relationship>(
    entity: &mut EntityWorldMut,
    marker: DisabledToDespawn,
) {
    fn recursive<T: Relationship>(
        world: &mut World,
        entity: Entity,
        marker: DisabledToDespawn,
        values: &mut EntityHashMap<T>,
        collection_buffer: &mut Vec<Entity>,
    ) {
        let mut entity_mut = world.entity_mut(entity);
        if !entity_mut.contains::<DisabledToDespawn>() {
            entity_mut.insert(marker);
        }
        let Some(relationship) = entity_mut.take::<T>() else {
            return;
        };
        values.insert(entity, relationship);
        let Some(target) = entity_mut.get::<T::RelationshipTarget>() else {
            return;
        };
        collection_buffer.extend(target.iter());
        while let Some(entity) = collection_buffer.pop() {
            recursive(world, entity, marker, values, collection_buffer);
        }
    }

    let now = NonLogNow(marker.added_frame());
    let id = entity.id();
    let mut values = EntityHashMap::new();
    let mut collection_buffer = Vec::new();
    entity.world_scope(|world| {
        recursive::<T>(world, id, marker, &mut values, &mut collection_buffer)
    });
    let (entities, values) = values.drain().unzip::<_, _, Vec<_>, _>();
    entity.buffer_undo_redo(
        now,
        BufferMany {
            entities: entities.into(),
            values,
            marker,
        },
    );
}

fn despawn_linked_store_relationship_target<T: Relationship>(
    entity: &mut EntityWorldMut,
    marker: DisabledToDespawn,
) {
    fn recursive<T: Relationship>(
        world: &mut World,
        entity: Entity,
        marker: DisabledToDespawn,
        values: &mut EntityHashMap<T::RelationshipTarget>,
    ) {
        let mut entity_mut = world.entity_mut(entity);
        if !entity_mut.contains::<DisabledToDespawn>() {
            entity_mut.insert(marker);
        }
        let Some(target) = entity_mut.take::<T::RelationshipTarget>() else {
            return;
        };
        for entity in target.iter() {
            recursive::<T>(world, entity, marker, values);
        }
        values.insert(entity, target);
    }

    let now = NonLogNow(marker.added_frame());
    let id = entity.id();

    if let Some(relationship) = entity.take::<T>() {
        entity.buffer_undo_redo(
            now,
            BufferSingle {
                entity: id,
                value: Some(relationship),
                marker,
            },
        );
    }

    let mut values = EntityHashMap::new();
    entity.world_scope(|world| recursive::<T>(world, id, marker, &mut values));
    let (entities, values) = values.drain().unzip::<_, _, Vec<_>, _>();
    entity.buffer_undo_redo(
        now,
        BufferMany {
            entities: entities.into(),
            values,
            marker,
        },
    );
}

struct BufferSingle<T> {
    entity: Entity,
    value: Option<T>,
    marker: DisabledToDespawn,
}

impl<T: Component> UndoRedo for BufferSingle<T> {
    fn undo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        self.value = entity.take();
        entity.insert(self.marker);
    }
    fn redo(&mut self, world: &mut World) {
        let mut entity = world.entity_mut(self.entity);
        entity.insert(self.value.take().expect("todo"));
        entity.remove::<DisabledToDespawn>();
    }
}

struct BufferMany<T> {
    entities: Box<[Entity]>,
    values: Vec<T>,
    marker: DisabledToDespawn,
}

impl<T: Component> UndoRedo for BufferMany<T> {
    fn undo(&mut self, world: &mut World) {
        world.insert_batch(self.entities.iter().copied().zip(self.values.drain(..)));
        for entity in self.entities.iter() {
            world.entity_mut(*entity).remove::<DisabledToDespawn>();
        }
    }
    fn redo(&mut self, world: &mut World) {
        world.insert_batch(
            self.entities
                .iter()
                .copied()
                .map(|entity| (entity, self.marker)),
        );
        self.values.extend(
            self.entities
                .iter()
                .map(|entity| world.entity_mut(*entity).take::<T>().expect("todo")),
        );
    }
}

#[cfg(test)]
mod test {
    use bevy::ecs::{component::Component, entity::Entity, world::World};

    use crate::{meta::{NonLogNow, RevDirection}, panic_on_error_events, prelude::UndoRedoBuffer, undo_redo::DisabledToDespawn};

    use super::despawn_not_linked;

    #[derive(Component)]
    #[relationship(relationship_target = ChildrenExtra)]
    struct ChildOfExtra {
        #[relationship]
        child_of: Entity,
        data: u8
    }

    #[derive(Component)]
    #[relationship_target(relationship = ChildOfExtra)]
    struct ChildrenExtra {
        #[relationship]
        children: Vec<Entity>,
        data: u8
    }

    struct Setup {
        world: World,
        marker: DisabledToDespawn,
        parent: Entity,
        entity: Entity,
        child: Entity,
    }

    impl Setup {
        fn new(relationship_loop: bool) -> Self {
            panic_on_error_events();
            let mut world = World::new();
            world.init_resource::<UndoRedoBuffer>();
            world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
            let marker = DisabledToDespawn::for_spawn_despawn(1);

            let parent = world.spawn_empty().id();
            let entity = world.spawn(ChildOfExtra { child_of: parent, data: 42 }).id();
            let child = world.spawn(ChildOfExtra { child_of: entity, data: 42 }).id();

            world.get_mut::<ChildrenExtra>(parent).unwrap().data = 42;
            world.get_mut::<ChildrenExtra>(entity).unwrap().data = 42;

            if relationship_loop {
                world.entity_mut(parent).insert(ChildOfExtra { child_of: child, data: 42 });
                world.get_mut::<ChildrenExtra>(child).unwrap().data = 42;
            }

            Setup {
                world,
                marker,
                parent,
                entity,
                child
            }
        }
    }
}
