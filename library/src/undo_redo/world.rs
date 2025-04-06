use std::{
    any::TypeId,
    hash::{BuildHasher, Hash, Hasher},
    marker::PhantomData,
    sync::Arc,
};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId, NoBundleEffect},
        component::{ComponentCloneBehavior, ComponentId, ComponentMutability},
        entity::{Entity, EntityCloner},
        resource::Resource,
        world::{EntityRef, EntityWorldMut, FromWorld, Mut, World},
    },
    platform_support::{
        collections::{HashMap, HashSet},
        hash::{FixedHasher, PassHash},
    },
};

use crate::{meta::RevMeta, unique_for_location};

use super::*;

pub trait RevWorld {
    /// Reversible version of [`World::despawn`].
    fn rev_despawn(&mut self, entity: Entity) -> bool;

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

    /// Reversible version of [`World::spawn`].
    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut;

    /// Reversible version of [`World::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, iter: I) -> Arc<[Entity]>
    where
        I: IntoIterator,
        I::Item: Bundle<Effect: NoBundleEffect>;

    /// Reversible version of [`World::spawn_empty`].
    fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_>;

    /// Reversible version of [`World::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);

    /// Reversible version of [`World::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);

    /// Reversible version of [`World::remove_resource`].
    fn rev_remove_resource<R: Resource>(&mut self);

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: Vec<ComponentId>,
    ) -> Option<EntityRef>;

    fn buffer_components_cached(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, Vec<ComponentId>),
    ) -> Option<EntityRef>;

    /// Reversible structural operations may trigger more hooks than expected as they also
    /// react on the buffer entity changing.
    ///
    /// For example [`EntityWorldMut::rev_insert`] will:
    ///
    /// 1. Move components that would be overwritten to a buffer entity:
    ///   - may trigger remove hook on the source entity
    ///   - may trigger insert hook on the buffer entity 1
    /// 2. ... and insert the given components in the source entity as expected
    /// 3. Undoing will move the inserted components to a second buffer before bringing the
    /// previous values back from the first:
    ///   - may trigger remove hook on the source entity
    ///   - may trigger insert hook on the buffer entity 2
    ///   - may trigger insert hook on the source entity
    ///   - may trigger remove hook on the buffer entity 1
    /// 4. Redoing is Undoing in reverse:
    ///   - may trigger remove hook on the source entity
    ///   - may trigger insert hook on the buffer entity 1
    ///   - may trigger remove hook on the buffer entity 2
    ///   - may trigger insert hook on the source entity
    /// 5. When the buffer entities go out of log while containing the components, remove
    /// hooks will run too for them.
    ///
    /// To identify these hooks, this method can be used which will return `true` for all
    /// hooks in the list above _except_ of 2. and 5. for which this returns `false`.
    ///
    /// Note though that during _all_ pure removal oparations like [`EntityWorldMut::rev_remove`]
    /// this method returns `true` as the buffering steps fully replace the usual
    /// [`EntityWorldMut::remove`] call.
    ///
    /// Additionally, the source and buffer entity can be differenced by the latter containing
    /// the [`DespawnAtOutOfLog`] component.
    ///
    /// Reversible despawns will delay remove hooks to a later time as the entity is covered
    /// by the same mechanic descibed in 5. in the list above.
    fn buffer_components_in_progress(&self) -> bool;
}

impl RevWorld for World {
    fn rev_despawn(&mut self, entity: Entity) -> bool {
        let entity_mut = self.entity_mut(entity);
        rev_despawn_inner(entity_mut)
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
            self.buffer_components_cached(
                entity,
                unique_for_location!(archetype_id, PhantomData::<B>),
                |world| {
                    let bundle_id = world.register_bundle::<B>().id();
                    insert_maybe_overwrite(world, bundle_id, archetype_id)
                },
            );
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
            self.buffer_components_cached(
                entity,
                unique_for_location!(archetype_id, PhantomData::<B>),
                |world| {
                    let bundle_id = world.register_bundle::<B>().id();
                    insert_no_overwrite(world, bundle_id, archetype_id)
                },
            );
        }
    }

    fn rev_spawn<B: Bundle>(&mut self, bundle: B) -> EntityWorldMut {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let bundle_id = self.register_bundle::<B>().id();
        let mut entity_mut = self.spawn(bundle);
        let entity = entity_mut.id();
        let world = unsafe {
            // SAFETY: only resources are mutated
            entity_mut.world_mut()
        };
        buffer_bundle(world, entity, BufferAt::Undo, bundle_id);
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

    fn rev_remove_resource<R: Resource>(&mut self) {
        if let Some(resource) = self.remove_resource::<R>() {
            self.buffer_undo_redo(ResourceSwap(Some(resource)));
        }
    }

    fn buffer_components(
        &mut self,
        entity: Entity,
        at: BufferAt,
        components: Vec<ComponentId>,
    ) -> Option<EntityRef> {
        if components.is_empty() {
            return None;
        }
        let bundle = components_to_bundle(self, components);
        buffer_bundle(self, entity, at, bundle)
    }

    fn buffer_components_cached(
        &mut self,
        entity: Entity,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, Vec<ComponentId>),
    ) -> Option<EntityRef> {
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
            if components.is_empty() {
                None
            } else {
                Some((at, components_to_bundle(self, components)))
            }
        });

        self.insert_resource(cache);

        maybe_components.and_then(|(at, bundle)| buffer_bundle(self, entity, at, bundle))
    }

    fn buffer_components_in_progress(&self) -> bool {
        self.contains_resource::<BufferComponentsInProgress>()
    }
}

#[derive(Resource, Default)]
struct NonEntityBufferRes(HashMap<ComponentId, fn(&mut World, Entity, BufferAt)>);

fn non_entity_buffer(world: &mut World, entity: Entity, at: BufferAt, components: &[ComponentId]) {
    if !world.contains_resource::<NonEntityBufferRes>() {
        return;
    }
    world.resource_scope(|world, non_entity_buffers: Mut<NonEntityBufferRes>| {
        for component in components.iter() {
            if let Some(c) = non_entity_buffers.0.get(component) {
                c(world, entity, at);
            }
        }
    })
}

pub(crate) fn register_non_entity_buffer<T: Component>(world: &mut World) {
    struct NonEntityBuffer<T: Component> {
        entity: Entity,
        component: Option<T>,
    }

    impl<T: Component> UndoRedo for NonEntityBuffer<T> {
        fn undo(&mut self, world: &mut World) {
            let mut entity = world.entity_mut(self.entity);
            if T::Mutability::MUTABLE {
                let component = unsafe {
                    // SAFETY: this if branch asserts the component is mutable
                    entity.get_mut_assume_mutable::<T>()
                };
                match component {
                    Some(mut c1) => match self.component.as_mut() {
                        Some(c2) => core::mem::swap(&mut *c1, c2),
                        None => self.component = entity.take::<T>(),
                    },
                    None => {
                        if let Some(c2) = self.component.take() {
                            entity.insert(c2);
                        }
                    }
                }
            } else {
                match entity.take::<T>() {
                    Some(mut c1) => match self.component.as_mut() {
                        Some(c2) => {
                            core::mem::swap(&mut c1, c2);
                            entity.insert(c1);
                        }
                        None => self.component = Some(c1),
                    },
                    None => {
                        if let Some(c2) = self.component.take() {
                            entity.insert(c2);
                        }
                    }
                }
            }
        }
        fn redo(&mut self, world: &mut World) {
            self.undo(world);
        }
    }

    let component_id = world.register_component::<T>();
    world
        .get_resource_or_init::<NonEntityBufferRes>()
        .0
        .entry(component_id)
        .or_insert_with(|| {
            |world, entity, at| {
                let mut component = None;
                if at != BufferAt::Undo {
                    component = world.entity_mut(entity).take::<T>();
                }
                let undo_redo = NonEntityBuffer { entity, component };
                world.buffer_undo_redo(undo_redo);
            }
        });
}

struct Spawn {
    entity: Entity,
    marker: DespawnAtOutOfLog,
}

impl UndoRedo for Spawn {
    fn undo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).insert(self.marker);
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).remove::<DespawnAtOutOfLog>();
    }
}

// todo: replace this with register_dynamic_bundle when linked issue is fixed
fn components_to_bundle(world: &mut World, components: Vec<ComponentId>) -> BundleId {
    #[derive(Resource, Default)]
    struct CheckedClonable(HashSet<ComponentId>);

    let mut checked = world
        .remove_resource::<CheckedClonable>()
        .unwrap_or_default();
    for &component_id in &components {
        if checked.0.insert(component_id) {
            if let Some(component_info) = world.components().get_info(component_id) {
                if component_info.clone_behavior() == &ComponentCloneBehavior::Ignore {
                    bevy::log::error!(
                        "Component {} is unclonable, it's insert, remove or overwrite will \
                        not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
            }
        }
    }
    world.insert_resource(checked);

    world.register_dynamic_bundle(&components).id()
}

fn buffer_bundle(
    world: &mut World,
    entity: Entity,
    at: BufferAt,
    bundle: BundleId,
) -> Option<EntityRef> {
    let mut buffer = BundleBuffer::new(world, entity, bundle);
    match at {
        BufferAt::Now => {
            let entities = buffer.toggle_state(world);
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            let out = buffer.move_bundle(world, entities, &components);
            world.buffer_undo_redo(buffer);
            Some(world.entity(out))
        }
        BufferAt::Undo => {
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            world.buffer_undo_redo(buffer);
            None
        }
        BufferAt::NowAndUndo => {
            let at_undo = buffer.clone(); // no buffer entity set yet so each spawns their own
            let entities = buffer.toggle_state(world);
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            let out = buffer.move_bundle(world, entities, &components);
            world.buffer_undo_redo(buffer).buffer_undo_redo(at_undo);
            Some(world.entity(out))
        }
    }
}

#[derive(Clone)]
struct BundleBuffer {
    bundle: BundleId,
    entity: Entity,
    state: BufferState,
}

#[derive(Clone)]
enum BufferState {
    Unspawned(DespawnAtOutOfLog),
    Empty(Entity),
    Filled(Entity),
}

struct BundleEntities {
    target: Entity,
    source: Entity,
    buffer: Entity,
}

impl BundleBuffer {
    fn new(world: &World, entity: Entity, bundle: BundleId) -> Self {
        let meta = world.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        Self {
            bundle,
            entity,
            state: BufferState::Unspawned(marker),
        }
    }
    fn toggle_state(&mut self, world: &mut World) -> BundleEntities {
        match self.state {
            BufferState::Unspawned(marker) => {
                let buffer = world.spawn(marker).id();
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
            BufferState::Filled(buffer) => {
                self.state = BufferState::Empty(buffer);
                BundleEntities {
                    target: self.entity,
                    source: buffer,
                    buffer,
                }
            }
            BufferState::Empty(buffer) => {
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
        }
    }
    fn get_component_ids(&self, world: &World) -> Box<[ComponentId]> {
        world
            .bundles()
            .get(self.bundle)
            .expect("todo")
            .explicit_components()
            .into()
    }
    fn move_bundle(
        &mut self,
        world: &mut World,
        entities: BundleEntities,
        components: &[ComponentId],
    ) -> Entity {
        let progress_res = world.buffer_components_in_progress();
        if !progress_res {
            world.insert_resource(BufferComponentsInProgress);
        }
        EntityCloner::build(world)
            .deny_all()
            .move_components(true)
            .without_required_components(|builder| {
                builder.allow_by_ids(components.iter().copied());
            })
            .clone_entity(entities.source, entities.target);
        if !progress_res {
            world.remove_resource::<BufferComponentsInProgress>();
        }
        entities.buffer
    }
}

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        let entities = self.toggle_state(world);
        let components = self.get_component_ids(world);
        self.move_bundle(world, entities, &components);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world);
    }
}

pub(super) struct ResourceSwap<R: Resource>(Option<R>);

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
