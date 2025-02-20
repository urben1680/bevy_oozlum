use std::{
    any::TypeId,
    hash::Hash,
    mem::take,
};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, BundleEffect, InsertMode, NoBundleEffect},
        component::ComponentId,
        entity::Entity,
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
    archetype_insert_if_new, archetype_insert_replace, archetype_insert_replace_backup,
    empty_entity_scope, get_bundle_id, move_with_despawn_at_out_of_log, BuffersUndoRedo,
    RevDisabled, UndoRedo,
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

/// Reversible version of [`spawn_batch`](bevy::ecs::system::command::spawn_batch).
///
/// If the entities are spawned with [`Disabled`], undoing this will do nothing though exiting the log when this command is undone still despawns the entities.
#[track_caller]
pub fn rev_spawn_batch<I>(bundles_iter: I) -> impl Command
where
    I: IntoIterator + Send + Sync + 'static,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    #[derive(Clone)]
    struct SpawnBatch(Box<[Entity]>);

    impl UndoRedo for SpawnBatch {
        fn undo(&mut self, world: &mut World) {
            world.insert_batch(self.0.iter().cloned().map(|entity| (entity, RevDisabled)));
        }
        fn redo(&mut self, world: &mut World) {
            let component_id = world
                .component_id::<RevDisabled>()
                .expect("undo should have registered Disabled");
            let mut commands = world.commands();
            for &entity in &*self.0 {
                commands.entity(entity).remove_by_id(component_id);
            }
            world.flush();
        }
    }

    |world: &mut World| {
        let entities: Box<[Entity]> = world.spawn_batch(bundles_iter).collect();

        if let Some(disabled_id) = world.component_id::<RevDisabled>() {
            const BUNDLE_EXPECT: &'static str =
                "iterated SpawnBatchIter should have registered bundle";
            let bundles = world.bundles();
            let bundle_id = bundles
                .get_id(TypeId::of::<I::Item>())
                .expect(BUNDLE_EXPECT);
            let bundle_info = bundles.get(bundle_id).expect(BUNDLE_EXPECT);

            if bundle_info.contributed_components().contains(&disabled_id) {
                //world.buffer_finalize(SpawnBatch(entities));
                return;
            }
        }

        //world.buffer_undo_redo_finalize(SpawnBatch(Arc::from(entities)));
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
        components: Box<[ComponentId]>,
    }

    impl InsertBatchKeep {
        fn undo_redo<const UNDO: bool>(&self, world: &mut World) {
            for &(entity, buffer) in self.entity_buffer_pairs.iter() {
                let components = self.components.clone();
                let mut builder = EntityCloneBuilder::new(world);
                builder
                    .deny_all()
                    .without_required_components(|builder| {
                        builder.allow_by_ids(components);
                    })
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
        entity_buffer_pairs: Box<[(Entity, Entity)]>,
        components: ReplaceComponents,
    }

    #[derive(PartialEq, Eq, Hash)]
    struct ReplaceComponents {
        insert: Box<[ComponentId]>,
        backup: Box<[ComponentId]>,
    }

    impl InsertBatchReplace {
        fn init(&self, world: &mut World) {
            for &(entity, buffer) in self.entity_buffer_pairs.iter().rev() {
                move_with_despawn_at_out_of_log(
                    world,
                    entity,
                    buffer,
                    self.components.backup.clone(),
                );
            }
        }
        fn undo_redo<const UNDO: bool>(&mut self, world: &mut World) {
            let mut iter = self.entity_buffer_pairs.iter_mut();
            let mut next_pair = || {
                if UNDO {
                    iter.next()
                } else {
                    iter.next_back()
                }
            };
            let (components1, components2) = if UNDO {
                (&self.components.insert, &self.components.backup)
            } else {
                (&self.components.backup, &self.components.insert)
            };

            empty_entity_scope(world, |world, empty_entity| {
                /*
                E1    E2    E3      E1    E2    E3
                mt    b1    b2      b1    b2    mt
                  \  /  \  /    ->    \  /  \  /
                   e1    e2            e1    e2
                 */
                let original_empty = *empty_entity;
                while let Some((entity, buffer)) = next_pair() {
                    move_with_despawn_at_out_of_log(
                        world,
                        *entity,
                        *empty_entity,
                        components1.clone(),
                    );
                    move_with_despawn_at_out_of_log(world, *buffer, *entity, components2.clone());
                    std::mem::swap(buffer, empty_entity); // todo Problem: updated nicht InsertBatchBuffers
                }
                todo!()
            })
        }
    }

    impl UndoRedo for InsertBatchReplace {
        fn undo(&mut self, world: &mut World) {
            self.undo_redo::<true>(world);
        }
        fn redo(&mut self, world: &mut World) {
            self.undo_redo::<false>(world);
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

    fn buffer<B: Bundle, const KEEP: bool>(
        world: &mut World,
        mut entities_per_archetype: HashMap<ArchetypeId, Vec<Entity>>,
        len: usize,
    ) {
        let bundle_id = get_bundle_id::<B>(world);
        let bundle_info = world.bundles().get(bundle_id).expect("todo");
        let mut entities_per_insert_components: HashMap<Box<[ComponentId]>, Vec<Entity>> =
            Default::default();
        let mut entities_per_replace_components: HashMap<ReplaceComponents, Vec<Entity>> =
            Default::default();
        for (archetype_id, entities) in entities_per_archetype.iter_mut() {
            let archetype = world.archetypes().get(*archetype_id).expect("todo");
            if KEEP {
                let mut keep = archetype_insert_if_new(bundle_info, archetype);
                keep.sort();
                update_entry(entities_per_insert_components.entry(keep), entities);
            } else {
                let mut insert = archetype_insert_replace(bundle_info, archetype);
                insert.sort();
                let mut backup = archetype_insert_replace_backup(bundle_info, archetype);
                if !backup.is_empty() {
                    backup.sort();
                    let components = ReplaceComponents { insert, backup };
                    update_entry(entities_per_replace_components.entry(components), entities);
                } else {
                    update_entry(entities_per_insert_components.entry(insert), entities);
                }
            }
        }
        let buffer_entities: Box<[Entity]> =
            world.entities().reserve_entities(len as u32).collect();
        let mut buffer_iter = buffer_entities.iter().copied();
        let keep: Box<[InsertBatchKeep]> = entities_per_insert_components
            .into_iter()
            .map(|(components, entities)| InsertBatchKeep {
                entity_buffer_pairs: entities.into_iter().zip(buffer_iter.by_ref()).collect(),
                components,
            })
            .collect();
        world.buffer_undo_redo(keep);
        if !KEEP && !entities_per_replace_components.is_empty() {
            world.flush(); // flush buffer entities to backup components that are to be replaced
            let replace: Box<[InsertBatchReplace]> = entities_per_replace_components
                .into_iter()
                .map(|(components, entities)| InsertBatchReplace {
                    entity_buffer_pairs: entities.into_iter().zip(buffer_iter.by_ref()).collect(),
                    components,
                })
                .inspect(|replace| replace.init(world))
                .collect();
            world.buffer_undo_redo(replace);
        }
        //world.buffer_finalize(InsertBatchBuffers(buffer_entities));
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

        match insert_mode {
            InsertMode::Keep => buffer::<B, true>(world, entities_per_archetype, batch.len()),
            InsertMode::Replace => buffer::<B, false>(world, entities_per_archetype, batch.len()),
        }

        world.insert_batch(batch);
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
