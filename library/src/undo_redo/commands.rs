use std::{
    any::TypeId,
    hash::{BuildHasher, Hash, Hasher},
    ops::Deref,
    sync::Arc,
};

use bevy::{
    ecs::{
        archetype::{Archetype, ArchetypeId},
        bundle::{Bundle, BundleId, BundleInfo, InsertMode},
        change_detection::Mut,
        component::{Component, ComponentId},
        entity::{Entity, EntityCloneBuilder, EntityLocation},
        resource::Resource,
        result::Result as CommandResult,
        system::{Command, Commands},
        world::{error::TryInsertBatchError, FromWorld, World},
    },
    platform_support::{
        collections::HashMap,
        hash::{DefaultHasher, FixedHasher, FixedState},
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

#[derive(Resource, Default)]
struct EmptyBuffers(Vec<Entity>);

impl EmptyBuffers {
    fn get_buffers_and_needs_flushing(
        &mut self,
        world: &World,
        buffers: usize,
    ) -> (Vec<Entity>, bool) {
        if self.0.len() < buffers {
            let count = buffers - self.0.len();
            let mut entities = Vec::with_capacity(buffers);
            entities.extend(
                world
                    .entities()
                    .reserve_entities(count as u32)
                    .chain(self.0.drain(..)),
            );
            (entities, true)
        } else {
            let start = self.0.len() - buffers;
            let entities = self.0.drain(start..).collect();
            (entities, false)
        }
    }
}

/// todo: replace with first party Disabled when bevy main branch can be used
#[derive(Component)]
struct Disabled;

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

    move |world: &mut World| {
        if TypeId::of::<()>() == TypeId::of::<B>() {
            return Ok(());
        }

        // check if all entities exist
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

        let mut buffer_entities: Vec<Entity> = Vec::new();
        let mut keep: HashMap<Arc<[ComponentId]>, Vec<ArchetypeId>> = HashMap::default();
        world.resource_scope(|world, mut bundle_buffers: Mut<BundleBuffers>| {
            let bundle_id = get_bundle_id::<B>(world);
            let bundle_info = world.bundles().get(bundle_id).expect("todo");
            match insert_mode {
                InsertMode::Keep => {
                    for &archetype_id in entities_per_archetype.keys() {
                        let components =
                            bundle_buffers.insert_keep(world, bundle_info, archetype_id);
                        keep.entry(components)
                            .or_insert_with(|| Vec::with_capacity(1))
                            .push(archetype_id);
                    }
                    buffer_entities = world
                        .entities()
                        .reserve_entities(batch.len() as u32)
                        .collect();
                }
                InsertMode::Replace => {
                    let mut replace: HashMap<InsertReplace, Vec<ArchetypeId>> = HashMap::default();
                    for &archetype_id in entities_per_archetype.keys() {
                        let insert_replace =
                            bundle_buffers.insert_replace(world, bundle_info, archetype_id);
                        if insert_replace.replaces.is_empty() {
                            keep.entry(insert_replace.adds)
                                .or_insert_with(|| Vec::with_capacity(1))
                                .push(archetype_id);
                        } else {
                            replace
                                .entry(insert_replace)
                                .or_insert_with(|| Vec::with_capacity(1))
                                .push(archetype_id);
                        }
                    }
                    buffer_entities = world
                        .entities()
                        .reserve_entities((batch.len() + replace.len()) as u32)
                        .collect();
                    // todo
                }
            }
        });
        let keeps: Box<[InsertBatchKeep]> = keep
            .into_iter()
            .flat_map(|(inserts, archetypes)| {
                archetypes
                    .into_iter()
                    .map(|archetype_id| entities_per_archetype.remove(&archetype_id).unwrap())
                    .map(|entities| {
                        let entity_buffer_pairs = entities
                            .into_iter()
                            .zip(buffer_entities.drain(..))
                            .collect();
                        InsertBatchKeep {
                            entity_buffer_pairs,
                            inserts,
                        }
                    })
            })
            .collect();

        Ok(())
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
