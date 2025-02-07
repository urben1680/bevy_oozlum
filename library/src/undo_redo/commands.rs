use std::{any::TypeId, collections::HashMap, ops::Deref, sync::Arc};

use bevy::ecs::{
    archetype::ArchetypeId,
    bundle::{Bundle, BundleId, InsertMode},
    component::{Component, ComponentId},
    entity::{Entity, EntityCloneBuilder},
    resource::Resource,
    result::Result as CommandResult,
    system::{Command, Commands},
    world::{error::TryInsertBatchError, FromWorld, World},
};

use super::{BuffersRev, Finalize, UndoRedo};

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

/// todo workaround until manual bundle registration is possible
fn get_bundle_id<B: Bundle>(world: &mut World) -> BundleId {
    #[derive(Resource)]
    struct EmptyEntity(Entity);

    let type_id = TypeId::of::<B>();
    match world.bundles().get_id(type_id) {
        Some(id) => id,
        None => {
            let empty_entity = world
                .get_resource::<EmptyEntity>()
                .filter(|res| world.entities().contains(res.0))
                .map(|res| res.0)
                .unwrap_or_else(|| {
                    let entity = world.spawn_empty().id();
                    world.flush();
                    world.insert_resource(EmptyEntity(entity));
                    entity
                });

            world.commands().entity(empty_entity).remove::<B>();
            world.flush();
            world
                .bundles()
                .get_id(type_id)
                .expect("above command should have registered bundle")
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

struct InsertBundleUndoRedo<T: Deref<Target = [ComponentId]>, const N: usize> {
    entity: Entity,
    buffers: [InsertBundleBuffer<T>; N],
}

struct InsertBundleBuffer<T: Deref<Target = [ComponentId]>> {
    entity: Entity,
    components: T,
}

struct InsertBundleFinalize<const N: usize>([Entity; N]);

impl<T: Deref<Target = [ComponentId]>, const N: usize> InsertBundleUndoRedo<T, N> {
    fn move_components<const IDX: usize, const TO_BUFFER: bool>(&self, world: &mut World) {
        if IDX >= N {
            return;
        }
        let buffer = &self.buffers[IDX];
        let mut builder = EntityCloneBuilder::new(world);
        builder
            .deny_all()
            .allow_by_ids(buffer.components.deref().iter().copied())
            .move_components(true);
        if TO_BUFFER {
            builder.clone_entity(self.entity, buffer.entity);
        } else {
            builder.clone_entity(buffer.entity, self.entity);
        }
    }
}

impl<T: Send + 'static + Deref<Target = [ComponentId]>, const N: usize> UndoRedo
    for InsertBundleUndoRedo<T, N>
{
    fn undo(&mut self, world: &mut World) {
        self.move_components::<0, true>(world);
        self.move_components::<1, false>(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.move_components::<1, true>(world);
        self.move_components::<0, false>(world);
    }
}

impl<const N: usize> Finalize for InsertBundleFinalize<N> {
    fn finalize_redone(self: Box<Self>, world: &mut World) {
        for entity in self.0.into_iter() {
            world.despawn(entity);
        }
    }
    fn finalize_undone(self: Box<Self>, world: &mut World) {
        self.finalize_redone(world);
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
    move |world: &mut World| {
        if TypeId::of::<()>() == TypeId::of::<B>() {
            return Ok(());
        }

        // check if all entities exist
        let batch: Vec<_> = batch.into_iter().collect();
        let locations: Vec<_> = batch
            .iter()
            .map(|&(entity, _)| (entity, world.entities().get(entity)))
            .collect();
        let invalid_entities: Vec<_> = locations
            .iter()
            .filter_map(|(entity, location)| location.is_none().then_some(*entity))
            .collect();
        if !invalid_entities.is_empty() {
            Err(TryInsertBatchError {
                bundle_type: core::any::type_name::<B>(),
                entities: invalid_entities,
            })?;
        }

        // get bundle information
        let bundle_id = get_bundle_id::<B>(world);
        let bundle_info = world
            .bundles()
            .get(bundle_id)
            .expect("BundleId comes from this world");

        // collect UndoRedo/Finalize per entity
        let explicit_ids = bundle_info.explicit_components();
        let required_ids = bundle_info.required_components();
        let iter = locations.into_iter().map(|(entity, location)| {
            let location = unsafe {
                // SAFETY: missing locations are handled in the return above
                location.unwrap_unchecked()
            };
            (entity, location.archetype_id)
        });

        let mut undo_redo_vec: Vec<InsertBundleUndoRedo<Arc<[ComponentId]>, 1>> = Vec::new();
        let mut finalize_vec: Vec<InsertBundleFinalize<1>> = Vec::new();
        let mut undo_redo_components: HashMap<ArchetypeId, Arc<[ComponentId]>> = HashMap::new();

        match insert_mode {
            InsertMode::Keep => {
                for (entity, archetype_id) in iter {
                    let insert_buffer_entity = world.entities().reserve_entity();
                    let insert_components =
                        undo_redo_components.entry(archetype_id).or_insert_with(|| {
                            let archetype = world.archetypes().get(archetype_id).expect("todo");
                            explicit_ids
                                .iter()
                                .copied()
                                .chain(
                                    required_ids
                                        .iter()
                                        .copied()
                                        .filter(|id| !archetype.contains(*id)),
                                )
                                .collect()
                        });
                    let insert_buffer = InsertBundleBuffer {
                        entity: insert_buffer_entity,
                        components: insert_components.clone(),
                    };
                    undo_redo_vec.push(InsertBundleUndoRedo {
                        entity,
                        buffers: [insert_buffer],
                    });
                    finalize_vec.push(InsertBundleFinalize([insert_buffer_entity]));
                }
            }
            InsertMode::Replace => {
                let mut undo_redo_overwrite_vec: Vec<InsertBundleUndoRedo<Arc<[ComponentId]>, 2>> =
                    Vec::new();
                let mut finalize_overwrite_vec: Vec<InsertBundleFinalize<2>> = Vec::new();
                let mut undo_redo_finalize_components: HashMap<ArchetypeId, Arc<[ComponentId]>> =
                    HashMap::new();

                for (entity, archetype_id) in iter {
                    let insert_buffer_entity = world.entities().reserve_entity();
                    let insert_components =
                        undo_redo_components.entry(archetype_id).or_insert_with(|| {
                            let archetype = world.archetypes().get(archetype_id).expect("todo");
                            explicit_ids
                                .iter()
                                .chain(required_ids.iter().filter(|id| !archetype.contains(**id)))
                                .copied()
                                .collect()
                        });
                    let insert_buffer = InsertBundleBuffer {
                        entity: insert_buffer_entity,
                        components: insert_components.clone(),
                    };

                    let overwrite_components = undo_redo_finalize_components
                        .entry(archetype_id)
                        .or_insert_with(|| {
                            let archetype = world.archetypes().get(archetype_id).expect("todo");
                            explicit_ids
                                .iter()
                                .copied()
                                .filter(|id| archetype.contains(*id))
                                .collect()
                        });
                    if overwrite_components.is_empty() {
                        undo_redo_vec.push(InsertBundleUndoRedo {
                            entity,
                            buffers: [insert_buffer],
                        });
                        finalize_vec.push(InsertBundleFinalize([insert_buffer_entity]));
                    } else {
                        let overwrite_buffer_entity = world.entities().reserve_entity();
                        let overwrite_buffer = InsertBundleBuffer {
                            entity: overwrite_buffer_entity,
                            components: overwrite_components.clone(),
                        };
                        undo_redo_overwrite_vec.push(InsertBundleUndoRedo {
                            entity,
                            buffers: [insert_buffer, overwrite_buffer],
                        });
                        finalize_overwrite_vec.push(InsertBundleFinalize([
                            insert_buffer_entity,
                            overwrite_buffer_entity,
                        ]));
                    }
                }

                if !undo_redo_overwrite_vec.is_empty() {
                    // spawn buffer entities for backup
                    world.flush();

                    // backup components that are about to be overwritten
                    for overwrite in undo_redo_overwrite_vec.iter() {
                        let buffer = &overwrite.buffers[1];
                        let mut builder = EntityCloneBuilder::new(world);
                        builder
                            .deny_all()
                            .allow_by_ids(buffer.components.iter().copied())
                            .move_components(true);
                        builder.clone_entity(overwrite.entity, buffer.entity);
                    }

                    // buffer UndoRedo/Finalize
                    world
                        .buffer_undo_redo(undo_redo_overwrite_vec)
                        .buffer_finalize(finalize_overwrite_vec);
                }
            }
        };

        // insert batch
        world.insert_batch(batch);

        // buffer UndoRedo/Finalize
        world
            .buffer_undo_redo(undo_redo_vec)
            .buffer_finalize(finalize_vec);

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
