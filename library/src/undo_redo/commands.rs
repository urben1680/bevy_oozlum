use std::{any::TypeId, hash::Hash, mem::take, ops::Deref, sync::Arc};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, InsertMode},
        change_detection::Mut,
        component::{Component, ComponentId},
        entity::{Entity, EntityCloneBuilder},
        resource::Resource,
        result::Result as CommandResult,
        system::{Command, Commands},
        world::{error::TryInsertBatchError, FromWorld, World},
    },
    platform_support::{
        collections::{hash_map::Entry, HashMap},
        hash::FixedHasher,
    },
};

use super::{
    bundle_buffer::{get_bundle_id, BundleBuffers, InsertReplace},
    BuffersRev, Finalize, UndoRedo,
};

pub trait RevCommands {
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);
    fn rev_remove_resource<R: Resource>(&mut self);
}

impl RevCommands for Commands<'_, '_> {
    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        self.queue(rev_init_resource::<R>())
    }
    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        self.queue(rev_insert_resource(resource))
    }
    fn rev_remove_resource<R: Resource>(&mut self) {
        self.queue(rev_remove_resource::<R>())
    }
}

/// todo: replace with first party Disabled when bevy main branch can be used
#[derive(Component)]
struct Disabled;

/// Reversible version of [`spawn_batch`](bevy::ecs::system::command::spawn_batch).
///
/// If the entities are spawned with [`Disabled`], undoing this will do nothing though exiting the log when this command is undone still despawns the entities.
#[track_caller]
pub fn rev_spawn_batch<I>(bundles_iter: I) -> impl Command
where
    I: IntoIterator + Send + Sync + 'static,
    I::Item: Bundle,
{
    #[derive(Clone)]
    struct SpawnBatch<T: Send + Deref<Target = [Entity]>>(T);

    impl UndoRedo for SpawnBatch<Arc<[Entity]>> {
        fn undo(&mut self, world: &mut World) {
            world.insert_batch(self.0.iter().cloned().map(|entity| (entity, Disabled)));
        }
        fn redo(&mut self, world: &mut World) {
            let component_id = world
                .component_id::<Disabled>()
                .expect("undo should have registered Disabled");
            let mut commands = world.commands();
            for &entity in &*self.0 {
                commands.entity(entity).remove_by_id(component_id);
            }
            world.flush();
        }
    }

    impl<T: Send + 'static + Deref<Target = [Entity]>> Finalize for SpawnBatch<T> {
        fn finalize_redone(self: Box<Self>, _: &mut World) {}
        fn finalize_undone(self: Box<Self>, world: &mut World) {
            for &entity in &*self.0 {
                // todo: despawn recursively?
                world.despawn(entity);
            }
        }
    }

    |world: &mut World| {
        let entities: Box<[Entity]> = world.spawn_batch(bundles_iter).collect();

        if let Some(disabled_id) = world.component_id::<Disabled>() {
            const BUNDLE_EXPECT: &'static str =
                "iterated SpawnBatchIter should have registered bundle";
            let bundles = world.bundles();
            let bundle_id = bundles
                .get_id(TypeId::of::<I::Item>())
                .expect(BUNDLE_EXPECT);
            let bundle_info = bundles.get(bundle_id).expect(BUNDLE_EXPECT);

            if bundle_info.contributed_components().contains(&disabled_id) {
                world.buffer_finalize(SpawnBatch(entities));
                return;
            }
        }

        world.buffer_undo_redo_finalize(SpawnBatch(Arc::from(entities)));
    }
}

/// Reversible version of [`insert_batch`](bevy::ecs::system::command::insert_batch).
///
/// Currently requires all components in `B` to be clonable.
#[track_caller]
pub fn rev_insert_batch<I, B>(batch: I, insert_mode: InsertMode) -> impl Command<CommandResult>
where
    I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
    B: Bundle,
{
    struct InsertBatchKeep {
        entity_buffer_pairs: Box<[(Entity, Entity)]>,
        inserts: Arc<[ComponentId]>,
    }

    impl InsertBatchKeep {
        fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
            for &(entity, buffer) in self.entity_buffer_pairs.iter() {
                let mut builder = EntityCloneBuilder::new(world);
                builder
                    .deny_all()
                    .allow_by_ids(self.inserts.iter().cloned())
                    .move_components(true);
                if UNDO {
                    builder.clone_entity(entity, buffer);
                } else {
                    builder.clone_entity(buffer, entity);
                }
            }
        }
    }

    impl UndoRedo for InsertBatchKeep {
        fn undo(&mut self, world: &mut World) {
            self.undo_redo::<true>(world);
        }
        fn redo(&mut self, world: &mut World) {
            self.undo_redo::<false>(world);
        }
    }

    struct InsertBatchReplace {
        /// First half (rounded down) are target entities, second half (rounded up) are buffer entities.
        ///
        /// First buffer entity is empty, unless the state is undone, then the last buffer entity is empty.
        ///
        /// Undoing/Redoing this makes the empty state bubble through the buffer entities.
        entity_buffers: Box<[Entity]>,
        components: InsertReplace,
    }

    impl InsertBatchReplace {
        fn undo_redo<const UNDO: bool, const SKIP_MOVE_INTO_TARGET: bool>(
            &self,
            world: &mut World,
        ) {
            let (components1, components2) = if UNDO {
                (&self.components.adds, &self.components.replaces)
            } else {
                (&self.components.replaces, &self.components.adds)
            };
            let entities_len = self.entity_buffers.len() / 2;
            let (entities, buffers) = self.entity_buffers.split_at(entities_len);
            let mut buffers = buffers.into_iter();
            let mut next_buffer = || {
                if UNDO {
                    *buffers.next().unwrap()
                } else {
                    *buffers.next_back().unwrap()
                }
            };
            let mut empty_buffer = next_buffer();
            for &entity in entities {
                let mut builder = EntityCloneBuilder::new(world);
                builder
                    .deny_all()
                    .allow_by_ids(components1.iter().cloned())
                    .move_components(true);
                builder.clone_entity(entity, empty_buffer);
                empty_buffer = next_buffer();
                if !SKIP_MOVE_INTO_TARGET {
                    let mut builder = EntityCloneBuilder::new(world);
                    builder
                        .deny_all()
                        .allow_by_ids(components2.iter().cloned())
                        .move_components(true);
                    builder.clone_entity(empty_buffer, entity);
                }
            }
        }
    }

    impl UndoRedo for InsertBatchReplace {
        fn undo(&mut self, world: &mut World) {
            self.undo_redo::<true, false>(world);
        }
        fn redo(&mut self, world: &mut World) {
            self.undo_redo::<false, false>(world);
        }
    }

    struct InsertBatchBuffers(Box<[Entity]>);

    impl Finalize for InsertBatchBuffers {
        fn finalize_undone(self: Box<Self>, world: &mut World) {
            for entity in self.0 {
                world.despawn(entity);
            }
        }
        fn finalize_redone(self: Box<Self>, world: &mut World) {
            self.finalize_undone(world);
        }
    }

    fn update_entry<K: Hash>(
        entry: Entry<K, Vec<Entity>, FixedHasher>,
        entities: &mut Vec<Entity>,
    ) {
        match entry {
            Entry::Occupied(mut occupied) => occupied.get_mut().append(entities),
            Entry::Vacant(vacant) => {
                vacant.insert(take(entities));
            }
        }
    }

    move |world: &mut World| {
        let mut entities_per_archetype: HashMap<ArchetypeId, Vec<Entity>> = HashMap::default();
        let mut invalid_entities = Vec::new();
        let batch: Vec<_> = batch
            .into_iter()
            .inspect(|&(entity, _)| match world.entities().get(entity) {
                Some(location) => entities_per_archetype
                    .entry(location.archetype_id)
                    .or_insert_with(|| Vec::with_capacity(1))
                    .push(entity),
                None => invalid_entities.push(entity),
            })
            .collect();

        if !invalid_entities.is_empty() {
            Err(TryInsertBatchError {
                bundle_type: core::any::type_name::<B>(),
                entities: invalid_entities,
            })?;
        }

        let mut buffer_entities: Box<[Entity]> = Box::new([]);
        let mut replace_buffers = 0;
        let mut keeps: HashMap<Arc<[ComponentId]>, Vec<Entity>> = HashMap::default();
        world.init_resource::<BundleBuffers>();
        world.resource_scope(|world, mut bundle_buffers: Mut<BundleBuffers>| {
            let bundle_id = get_bundle_id::<B>(world);
            let bundle_info = world.bundles().get(bundle_id).expect("todo");
            match insert_mode {
                InsertMode::Keep => {
                    for (archetype_id, entities) in entities_per_archetype.iter_mut() {
                        let archetype = world.archetypes().get(*archetype_id).expect("todo");
                        let components = bundle_buffers.insert_keep(bundle_info, archetype);
                        update_entry(keeps.entry(components), entities)
                    }
                    buffer_entities = world
                        .entities()
                        .reserve_entities(batch.len() as u32)
                        .collect();
                }
                InsertMode::Replace => {
                    let mut replaces: HashMap<InsertReplace, Vec<Entity>> = HashMap::default();
                    for (archetype_id, entities) in entities_per_archetype.iter_mut() {
                        let archetype = world.archetypes().get(*archetype_id).expect("todo");
                        let insert_replace = bundle_buffers.insert_replace(bundle_info, archetype);
                        if insert_replace.replaces.is_empty() {
                            update_entry(keeps.entry(insert_replace.adds), entities)
                        } else {
                            replace_buffers += entities.len() + 1;
                            update_entry(replaces.entry(insert_replace), entities)
                        }
                    }
                    buffer_entities = world
                        .entities()
                        .reserve_entities((batch.len() + replaces.len()) as u32)
                        .collect();

                    if !replaces.is_empty() {
                        world.flush(); // buffer entities are already needed in the `inspect` below
                        let mut replaces_buffers = buffer_entities.iter().copied();
                        let replaces: Box<[InsertBatchReplace]> = replaces
                            .into_iter()
                            .map(|(components, entities)| {
                                let buffers_len = entities.len() + 1;
                                InsertBatchReplace {
                                    entity_buffers: entities
                                        .into_iter()
                                        .chain(replaces_buffers.by_ref().take(buffers_len))
                                        .collect(),
                                    components,
                                }
                            })
                            .inspect(|replaces| replaces.undo_redo::<false, true>(world))
                            .collect();
                        world.buffer_undo_redo(replaces);
                    }
                }
            }
        });

        if !keeps.is_empty() {
            let mut keeps_buffers = buffer_entities[replace_buffers..].into_iter().copied();
            let keeps: Box<[InsertBatchKeep]> = keeps
                .into_iter()
                .map(|(inserts, entities)| InsertBatchKeep {
                    entity_buffer_pairs: entities.into_iter().zip(keeps_buffers.by_ref()).collect(),
                    inserts,
                })
                .collect();
            world.buffer_undo_redo(keeps);
        }

        world.insert_batch(batch);
        world.buffer_finalize(InsertBatchBuffers(buffer_entities));
        Ok(())
    }
}

struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> UndoRedo for ResourceSwap<R> {
    fn undo(&mut self, world: &mut World) {
        match world.get_resource_mut::<R>() {
            Some(mut r1) => match self.0.as_mut() {
                Some(r2) => core::mem::swap(&mut *r1, r2),
                None => self.0 = world.remove_resource::<R>(),
            },
            None => {
                if let Some(r2) = self.0.take() {
                    world.insert_resource(r2)
                }
            }
        }
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world)
    }
}

/// Reversible version of [`init_resource`](bevy::ecs::system::command::init_resource).
#[track_caller]
pub fn rev_init_resource<R: Resource + FromWorld>() -> impl Command {
    |world: &mut World| {
        if !world.contains_resource::<R>() {
            world.init_resource::<R>();
            world.buffer_undo_redo(ResourceSwap::<R>(None));
        }
    }
}

/// Reversible version of [`insert_resource`](bevy::ecs::system::command::insert_resource).
#[track_caller]
pub fn rev_insert_resource<R: Resource>(resource: R) -> impl Command {
    |world: &mut World| {
        let swap = ResourceSwap(world.remove_resource::<R>());
        world.insert_resource(resource);
        world.buffer_undo_redo(swap);
    }
}

/// Reversible version of [`remove_resource`](bevy::ecs::system::command::remove_resource).
pub fn rev_remove_resource<R: Resource>() -> impl Command {
    |world: &mut World| {
        if let Some(resource) = world.remove_resource::<R>() {
            world.buffer_undo_redo(ResourceSwap(Some(resource)));
        }
    }
}

#[cfg(test)]
mod test {
    use crate::prelude::RevBuffers;

    use super::*;

    #[derive(Resource, PartialEq, Debug, Default)]
    struct TestRes(u8);

    #[test]
    fn rev_init_resource_works() {
        let mut world = World::new();
        world.init_resource::<RevBuffers>();

        rev_init_resource::<TestRes>().apply(&mut world);
        let mut undo_redo = world.resource_mut::<RevBuffers>().pop_undo_redo().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(0)));
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(0)));

        rev_init_resource::<TestRes>().apply(&mut world);
        let undo_redo = world.resource_mut::<RevBuffers>().pop_undo_redo();
        assert!(undo_redo.is_none());
    }

    #[test]
    fn rev_insert_resource_works() {
        let mut world = World::new();
        world.init_resource::<RevBuffers>();

        rev_insert_resource(TestRes(10)).apply(&mut world);
        let mut undo_redo = world.resource_mut::<RevBuffers>().pop_undo_redo().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));

        rev_insert_resource(TestRes(20)).apply(&mut world);
        let mut undo_redo = world.resource_mut::<RevBuffers>().pop_undo_redo().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(20)));
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(20)));
    }

    #[test]
    fn rev_remove_resource_works() {
        let mut world = World::new();
        world.init_resource::<RevBuffers>();

        world.insert_resource(TestRes(10));
        rev_remove_resource::<TestRes>().apply(&mut world);
        let mut undo_redo = world.resource_mut::<RevBuffers>().pop_undo_redo().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), None);
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);

        rev_remove_resource::<TestRes>().apply(&mut world);
        let undo_redo = world.resource_mut::<RevBuffers>().pop_undo_redo();
        assert!(undo_redo.is_none());
    }
}
