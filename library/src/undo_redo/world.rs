use std::{
    any::TypeId,
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, NoBundleEffect},
        component::ComponentId,
        entity::{Entity, EntityLocation},
        resource::Resource,
        world::{EntityWorldMut, FromWorld, World, error::TryInsertBatchError},
    },
    log::warn,
    platform::{
        collections::HashMap,
        hash::{FixedHasher, PassHash},
    },
};

use crate::meta::RevMeta;

use super::*;

pub trait RevWorld {
    fn buffer_components_in_progress(&self) -> Option<BufferInProgress>;

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, ()>;

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, ()>;

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, ()>;

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`World::despawn`].
    fn rev_despawn(&mut self, entity: Entity) -> bool;

    /// Reversible version of [`World::get_resource_or_init`].
    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R>;

    /// Reversible version of [`World::get_resource_or_insert_with`].
    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R>;

    // rev_init_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);

    /// Reversible version of [`World::insert_batch`].
    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    // rev_insert_non_send_by_id
    // out of scope due Send bound on UndoRedo

    // rev_insert_non_send_resource
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);

    // rev_insert_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    // rev_remove_non_send_by_id
    // out of scope due Send bound on UndoRedo

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource, Out>(&mut self, c: impl FnOnce(&R) -> Out) -> Option<Out>;

    /// rev_remove_resource_by_id
    // blocked on https://github.com/bevyengine/bevy/pull/17485

    /// Reversible version of [`World::spawn`].
    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut;

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_>;

    /// Reversible version of [`World::try_despawn`].
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevEntityDespawnError>;

    /// Reversible version of [`World::try_insert_batch`].
    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), TryInsertBatchError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), TryInsertBatchError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>;
}

impl RevWorld for World {
    fn buffer_components_in_progress(&self) -> Option<BufferInProgress> {
        buffer_components_in_progress(self)
    }

    fn rev_despawn(&mut self, entity: Entity) -> bool {
        self.rev_try_despawn(entity)
            .inspect_err(|error| warn!("{error}"))
            .is_ok()
    }

    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        let batch: Vec<_> = batch.into_iter().collect();
        for &(entity, _) in batch.iter() {
            let archetype_id = self.entities().get(entity).unwrap().archetype_id;
            pre_insert::<B>(self, entity, archetype_id, InsertMode::Replace).expect("todo");
        }
        self.insert_batch(batch);
    }

    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        let batch = batch.into_iter();
        let (min, max) = batch.size_hint();
        let mut entities = Vec::with_capacity(max.unwrap_or(min));
        self.insert_batch_if_new(batch.inspect(|(entity, _)| entities.push(*entity)));
        for entity in entities {
            let archetype_id = self.entities().get(entity).unwrap().archetype_id;
            pre_insert::<B>(self, entity, archetype_id, InsertMode::Keep).expect("todo");
        }
    }

    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let mut entity_mut = self.spawn(bundle);
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(Spawn { entity, marker });
        entity_mut
    }

    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>,
    {
        struct SpawnBatch {
            entities: Arc<[Entity]>,
            marker: DespawnAtOutOfLog,
        }

        impl UndoRedo for SpawnBatch {
            fn undo(&mut self, world: &mut World) {
                world.insert_batch(self.entities.iter().map(|entity| (*entity, self.marker)));
            }
            fn redo(&mut self, world: &mut World) {
                let id = world.component_id::<DespawnAtOutOfLog>().expect("todo");
                for entity in self.entities.iter() {
                    world.entity_mut(*entity).remove_by_id(id);
                }
            }
        }

        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let entities: Arc<[Entity]> = self.spawn_batch(iter).collect();
        self.buffer_undo_redo(SpawnBatch {
            entities: entities.clone(),
            marker,
        });
        entities
    }

    fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_> {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let mut entity_mut = self.spawn_empty();
        let entity = entity_mut.id();
        entity_mut.buffer_undo_redo(Spawn { entity, marker });
        entity_mut
    }

    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        if !self.contains_resource::<R>() {
            self.init_resource::<R>();
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
    }

    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        let swap = ResourceSwap(self.remove_resource::<R>());
        self.insert_resource(resource);
        self.buffer_undo_redo(swap);
    }

    fn rev_remove_resource<R: Resource, Out>(&mut self, c: impl FnOnce(&R) -> Out) -> Option<Out> {
        self.remove_resource::<R>().map(|resource| {
            let out = c(&resource);
            self.buffer_undo_redo(ResourceSwap(Some(resource)));
            out
        })
    }

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: &[ComponentId],
    ) -> Result<Option<Entity>, ()> {
        let bundle = components_to_bundle(self, components);
        self.buffer_bundle(entity, at, bundle)
    }

    fn buffer_components_cached<T: AsRef<[ComponentId]>>(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, T),
    ) -> Result<Option<Entity>, ()> {
        #[derive(Resource, Default)]
        pub(crate) struct CachedBundles(HashMap<u64, Option<(BufferAt, BundleId)>, PassHash>);
        fn type_id_of_var<T: 'static>(_: &T) -> TypeId {
            TypeId::of::<T>()
        }

        let mut hasher = FixedHasher::default().build_hasher();
        type_id_of_var(&key).hash(&mut hasher);
        key.hash(&mut hasher);
        let key = hasher.finish();

        let mut cache = self.remove_resource::<CachedBundles>().unwrap_or_default();

        let maybe_components = *cache.0.entry(key).or_insert_with(|| {
            let (at, components) = components(self);
            let components = components.as_ref();
            if components.is_empty() {
                None
            } else {
                Some((at, components_to_bundle(self, &components)))
            }
        });

        self.insert_resource(cache);

        match maybe_components {
            Some((at, bundle)) => self.buffer_bundle(entity, at, bundle),
            None => match self.entities().get(entity) {
                Some(_) => Ok(None),
                None => Err(()),
            },
        }
    }

    fn buffer_bundle(
        &mut self,
        entity: Entity,
        at: BufferAt,
        bundle: BundleId,
    ) -> Result<Option<Entity>, ()> {
        buffer_bundle(self, entity, at, bundle)
    }

    fn rev_get_resource_or_init<R: Resource + FromWorld>(&mut self) -> Mut<'_, R> {
        self.rev_init_resource::<R>();
        self.resource_mut::<R>()
    }

    fn rev_get_resource_or_insert_with<R: Resource>(
        &mut self,
        func: impl FnOnce() -> R,
    ) -> Mut<'_, R> {
        if !self.contains_resource::<R>() {
            self.buffer_undo_redo(ResourceSwap::<R>(None));
        }
        self.get_resource_or_insert_with(func)
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, entity: Entity) -> Result<(), RevEntityDespawnError> {
        match self.get_entity_mut(entity) {
            Ok(entity_mut) => {
                if !rev_despawn_inner(entity_mut) {
                    Err(RevEntityDespawnError::AlreadyMarkedForDespawn(entity))
                } else {
                    Ok(())
                }
            }
            Err(error) => Err(RevEntityDespawnError::Other(error.into())),
        }
    }

    fn rev_try_insert_batch<I, B>(&mut self, batch: I) -> Result<(), TryInsertBatchError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Replace)
    }

    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I) -> Result<(), TryInsertBatchError>
    where
        I: IntoIterator,
        I::IntoIter: Iterator<Item = (Entity, B)>,
        B: Bundle<Effect: NoBundleEffect>,
    {
        try_insert_batch_inner(self, batch, InsertMode::Keep)
    }
}

fn try_insert_batch_inner<I, B>(
    world: &mut World,
    batch: I,
    insert_mode: InsertMode,
) -> Result<(), TryInsertBatchError>
where
    I: IntoIterator,
    I::IntoIter: Iterator<Item = (Entity, B)>,
    B: Bundle<Effect: NoBundleEffect>,
{
    let mut invalid_entities = Vec::new();
    let batch: Vec<_> = batch
        .into_iter()
        .filter(|&(entity, _)| {
            world
                .entities()
                .get(entity)
                .filter(|&EntityLocation { archetype_id, .. }| {
                    pre_insert::<B>(world, entity, archetype_id, insert_mode).is_ok()
                })
                .or_else(|| {
                    invalid_entities.push(entity);
                    None
                })
                .is_some()
        })
        .collect();
    match insert_mode {
        InsertMode::Replace => world.insert_batch(batch),
        InsertMode::Keep => world.insert_batch_if_new(batch),
    }

    if invalid_entities.is_empty() {
        Ok(())
    } else {
        Err(TryInsertBatchError {
            bundle_type: core::any::type_name::<B>(),
            entities: invalid_entities,
        })
    }
}

#[cfg(test)]
mod test {

    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::panic_on_error_events;

    use super::*;

    #[derive(Resource, PartialEq, Debug, Default, Copy, Clone)]
    struct TestRes(u8);

    #[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
    #[require(Required1)]
    struct Explicit1(u8);

    #[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
    #[require(Required2)]
    struct Explicit2(u8);

    #[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
    struct Required1(u8);

    #[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
    struct Required2(u8);

    fn setup() -> World {
        panic_on_error_events();
        let mut world = World::new();
        world.init_resource::<UndoRedoBuffer>();
        world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
        world
    }

    mod buffer_at_now {
        use super::*;

        fn inner(c: impl FnOnce(&mut World, Entity, ComponentId) -> Result<Option<Entity>, ()>) {
            let mut world = setup();
            let explicit_id = world.register_component::<Explicit1>();
            let entity = world.spawn((Explicit1(1), Required1(1))).id();
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            let buffer_entity = c(&mut world, entity, explicit_id)
                .expect("should be Ok")
                .expect("should be Some");
            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

            assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Explicit1>(entity), None);
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            buffer.undo(&mut world);
            assert_eq!(world.get::<Explicit1>(buffer_entity), None);
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            buffer.redo(&mut world);
            assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Explicit1>(entity), None);
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));
        }

        #[test]
        fn buffer_components_buffers() {
            inner(|world, entity, component_id| {
                world.buffer_components(entity, BufferAt::Now, &[component_id])
            });
        }

        #[test]
        fn buffer_components_cached_buffers() {
            inner(|world, entity, component_id| {
                let components = |_: &mut World| {
                    static ASSERT_CACHED: AtomicBool = AtomicBool::new(true);
                    assert!(ASSERT_CACHED.fetch_not(Ordering::Relaxed));
                    (BufferAt::Now, [component_id])
                };

                let another_entity = world.spawn_empty().id();
                world
                    .buffer_components_cached(another_entity, (), components)
                    .expect("should be Ok")
                    .expect("should be Some");

                world.buffer_components_cached(entity, (), components)
            });
        }

        #[test]
        fn buffer_bundle_buffers() {
            inner(|world, entity, component_id| {
                let bundle = world.register_dynamic_bundle(&[component_id]).id();
                world.buffer_bundle(entity, BufferAt::Now, bundle)
            })
        }
    }

    mod buffer_at_undo {
        use super::*;

        fn inner(c: impl FnOnce(&mut World, Entity, ComponentId) -> Result<Option<Entity>, ()>) {
            let mut world = setup();
            let explicit_id = world.register_component::<Explicit1>();
            let entity = world.spawn((Explicit1(1), Required1(1))).id();
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            let result = c(&mut world, entity, explicit_id);
            assert_eq!(result, Ok(None));
            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            buffer.undo(&mut world);
            assert_eq!(world.get::<Explicit1>(entity), None);
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            buffer.redo(&mut world);
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));
        }

        #[test]
        fn buffer_components_buffers() {
            inner(|world, entity, component_id| {
                world.buffer_components(entity, BufferAt::Undo, &[component_id])
            });
        }

        #[test]
        fn buffer_components_cached_buffers() {
            inner(|world, entity, component_id| {
                let components = |_: &mut World| {
                    static ASSERT_CACHED: AtomicBool = AtomicBool::new(true);
                    assert!(ASSERT_CACHED.fetch_not(Ordering::Relaxed));
                    (BufferAt::Undo, [component_id])
                };

                let another_entity = world.spawn_empty().id();
                let result = world.buffer_components_cached(another_entity, (), components);
                assert_eq!(result, Ok(None));

                world.buffer_components_cached(entity, (), components)
            });
        }

        #[test]
        fn buffer_bundle_buffers() {
            inner(|world, entity, component_id| {
                let bundle = world.register_dynamic_bundle(&[component_id]).id();
                world.buffer_bundle(entity, BufferAt::Undo, bundle)
            })
        }
    }

    mod buffer_at_now_and_undo {
        use super::*;

        fn inner(c: impl FnOnce(&mut World, Entity, ComponentId) -> Result<Option<Entity>, ()>) {
            let mut world = setup();
            let explicit_id = world.register_component::<Explicit1>();
            let entity = world.spawn((Explicit1(1), Required1(1))).id();
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            let buffer_entity = c(&mut world, entity, explicit_id)
                .expect("should be Ok")
                .expect("should be Some");
            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
            world.entity_mut(entity).insert(Explicit1(2));

            assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(2)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            buffer.undo(&mut world);
            assert_eq!(world.get::<Explicit1>(buffer_entity), None);
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

            buffer.redo(&mut world);
            assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
            assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(2)));
            assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));
        }

        #[test]
        fn buffer_components_buffers() {
            inner(|world, entity, component_id| {
                world.buffer_components(entity, BufferAt::NowAndUndo, &[component_id])
            });
        }

        #[test]
        fn buffer_components_cached_buffers() {
            inner(|world, entity, component_id| {
                let components = |_: &mut World| {
                    static ASSERT_CACHED: AtomicBool = AtomicBool::new(true);
                    assert!(ASSERT_CACHED.fetch_not(Ordering::Relaxed));
                    (BufferAt::NowAndUndo, [component_id])
                };

                let another_entity = world.spawn_empty().id();
                world
                    .buffer_components_cached(another_entity, (), components)
                    .expect("should be Ok")
                    .expect("should be Some");

                world.buffer_components_cached(entity, (), components)
            });
        }

        #[test]
        fn buffer_bundle_buffers() {
            inner(|world, entity, component_id| {
                let bundle = world.register_dynamic_bundle(&[component_id]).id();
                world.buffer_bundle(entity, BufferAt::NowAndUndo, bundle)
            })
        }
    }

    #[test]
    fn buffer_fails_on_despawned_or_rev_despawned() {
        let mut world = setup();
        let explicit_id = world.register_component::<Explicit1>();
        let bundle = world.register_dynamic_bundle(&[explicit_id]).id();
        let entity_mut = world.spawn((Explicit1(1), Required1(1)));
        let entity = entity_mut.id();
        entity_mut.rev_despawn();

        for entity in [entity, Entity::PLACEHOLDER] {
            for at in [BufferAt::Now, BufferAt::Undo, BufferAt::NowAndUndo] {
                let result = world.buffer_components(entity, at, &[explicit_id]);
                assert_eq!(result, Err(()), "{entity:?}, {at:?}");

                let result =
                    world.buffer_components_cached(entity, (entity, at), |_| (at, [explicit_id]));
                assert_eq!(result, Err(()), "{entity:?}, {at:?}");

                let result = world.buffer_bundle(entity, at, bundle);
                assert_eq!(result, Err(()), "{entity:?}, {at:?}");
            }
        }
    }

    #[test]
    fn rev_init_resource_on_unexisting_inits_resource() {
        let mut world = setup();

        world.rev_init_resource::<TestRes>();
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(0)));
        buffer.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
        buffer.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(0)));
    }

    #[test]
    fn rev_init_resource_on_existing_noop() {
        let mut world = setup();
        world.init_resource::<TestRes>();
        world.rev_init_resource::<TestRes>();
        assert!(world.resource::<UndoRedoBuffer>().is_empty());
    }

    #[test]
    fn rev_insert_resource_on_unexisting_inserts() {
        let mut world = setup();

        world.rev_insert_resource(TestRes(10));
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        buffer.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
        buffer.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
    }

    #[test]
    fn rev_insert_resource_on_existing_overwrites() {
        let mut world = setup();

        world.insert_resource(TestRes(10));
        world.rev_insert_resource(TestRes(20));
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(20)));
        buffer.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        buffer.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(20)));
    }

    #[test]
    fn rev_remove_resource_on_existing_removes() {
        let mut world = setup();

        world.insert_resource(TestRes(10));
        let out = world.rev_remove_resource::<TestRes, _>(|r| *r);
        assert_eq!(out, Some(TestRes(10)));
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        assert_eq!(world.get_resource::<TestRes>(), None);
        buffer.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        buffer.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
    }

    #[test]
    fn rev_remove_resource_on_unexisting_noop() {
        let mut world = setup();
        let out = world.rev_remove_resource::<TestRes, _>(|r| *r);
        assert_eq!(out, None);
        assert!(world.resource::<UndoRedoBuffer>().is_empty());
    }
}
