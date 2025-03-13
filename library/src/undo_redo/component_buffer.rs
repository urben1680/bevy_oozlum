use std::{borrow::Borrow, hash::{BuildHasher, Hash, Hasher}, marker::PhantomData};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleId},
        component::{ComponentCloneBehavior, ComponentId},
        entity::{Entity, EntityCloner},
        resource::Resource,
        world::{Mut, World},
    },
    platform_support::{
        collections::{HashMap, HashSet},
        hash::{FixedHasher, PassHash},
    },
};

use crate::meta::RevMeta;

use super::{BuffersUndoRedo, DespawnAtOutOfLog, UndoRedo};

#[derive(Resource)]
pub(super) struct BufferComponentsInProgress;

#[derive(Resource, Default)]
pub(crate) struct BufferBundles {
    cache: HashMap<u64, Option<BundleId>, PassHash>,
    unclonable: HashSet<ComponentId>,
}

/*
Ideen:
- RevWorld Methoden statt resourcenmethoden, die resource wird in der funktion wenn benötigt aufgerufen
-- mit und ohne at_undo nutzen besser weiterhin eine inner function
- Hilfsmethode die nochmal die Components nach clonable überprüft aber nicht mehr filtert
*/

trait BufferSubject<Marker> {
    fn to_bundle(self, world: &mut World) -> BundleId;
}

struct BundleIdMarker;
impl BufferSubject<BundleIdMarker> for BundleId {
    fn to_bundle(self, world: &mut World) -> BundleId {
        check_unclonable(world, |world| contributed_components(world, self));
        self
    }
}

struct BundleMarker;
impl<T: Bundle> BufferSubject<BundleMarker> for PhantomData<T> {
    fn to_bundle(self, world: &mut World) -> BundleId {
        let bundle = world.register_bundle::<T>().id();
        check_unclonable(world, |world| contributed_components(world, bundle));
        bundle
    }
}

struct ComponentIdMarker;
impl BufferSubject<ComponentIdMarker> for ComponentId {
    fn to_bundle(self, world: &mut World) -> BundleId {
        check_unclonable(world, |_| &[self]);
        world.register_dynamic_bundle(&[self]).id()
    }
}

struct IterMarker;
impl<I: IntoIterator<Item: Borrow<ComponentId>>> BufferSubject<IterMarker> for I {
    fn to_bundle(self, world: &mut World) -> BundleId {
        let ids: Vec<ComponentId> = self.into_iter().map(|id| *id.borrow()).collect();
        let slice = &ids;
        check_unclonable(world, move |_| slice);
        world.register_dynamic_bundle(&ids).id()
    }
}

struct WorldClosureMarker<Marker>(PhantomData<Marker>);
impl<F: FnOnce(&mut World) -> T, T: BufferSubject<Marker>, Marker> BufferSubject<WorldClosureMarker<Marker>> for F {
    fn to_bundle(self, world: &mut World) -> BundleId {
        // implementors check for clonable
        self(world).to_bundle(world)
    }
}

pub(super) fn buffer_components<Marker>(
    world: &mut World,
    entity: Entity,
    bundle: impl BufferSubject<Marker>,
    now: bool
) {
    let bundle = bundle.to_bundle(world);
    check_unclonable(world, bundle);
    buffer_components_inner(world, entity, bundle, now);
}

pub(super) fn buffer_components_cached<T: BufferSubject<Marker>, Marker>(
    world: &mut World,
    entity: Entity,
    bundle: impl FnOnce(&mut World) -> T,
    now: bool,
    cache: impl Hash
) {
    #[derive(Resource)]
    struct CachedBufferIds(HashMap<u64, BundleId, PassHash>);
    let bundle = world.resource_scope(|world, mut cache_res: Mut<CachedBufferIds>| {
        let mut hasher = FixedHasher::default().build_hasher();
        cache.hash(&mut hasher);
        let cache = hasher.finish();
        *cache_res.0.entry(cache).or_insert_with(|| {
            let bundle = bundle(world).to_bundle(world);
            check_unclonable(world, bundle);
            bundle
        })
    });
    buffer_components_inner(world, entity, bundle, now);
}

// todo: remove this resource and scope when linked issue is fixed
fn check_unclonable(world: &mut World, ids: impl FnOnce(&World) -> &[ComponentId]) {
    #[derive(Resource)]
    struct Unclonable(HashSet<ComponentId>);
    world.resource_scope(|world, mut unclonable_res : Mut<Unclonable>| {
        let ids = ids(world);
        for &component_id in components {
            let component_info = world.components().get_info(component_id).expect("todo");
            let unclonable = component_info.clone_behavior() == &ComponentCloneBehavior::Ignore;
            if unclonable && unclonable_res.0.insert(component_id) {
                bevy::log::error!(
                    "Component {} is unclonable, it's insert, remove or overwrite will not \
                    be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                    component_info.name()
                );
            }
        }
    });
}

fn contributed_components(world: &World, bundle: BundleId) -> &[ComponentId] {
    world.bundles().get(bundle).expect("todo").contributed_components()
}

fn buffer_components_inner(
    world: &mut World,
    entity: Entity,
    bundle: BundleId,
    now: bool
) {
    let meta = world.get_resource::<RevMeta>().expect("todo");
    let marker = DespawnAtOutOfLog::new(meta);
    let mut undo_redo = BundleBuffer::new(bundle, entity, marker);
    if now {
        undo_redo.move_bundle(world);
    }
    world.buffer_undo_redo(undo_redo);
}


// todo: non-cached version with taking a bundle-id which is buffer_components_inner
impl BufferBundles {
    pub(super) fn buffer_components(
        &mut self,
        world: &mut World,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
        now: bool,
    ) -> Option<Entity> {
        let bundle = self.register_bundle(world, components)?;
        buffer_components_inner(world, entity, bundle, now)
    }

    pub(super) fn buffer_components_cached<I: IntoIterator<Item = ComponentId>>(
        &mut self,
        world: &mut World,
        entity: Entity,
        cache: impl Hash,
        components: impl FnOnce(&mut World) -> I,
        now: bool,
    ) -> Option<Entity> {
        let mut hasher = FixedHasher::default().build_hasher();
        cache.hash(&mut hasher);
        let cache = hasher.finish();
        let bundle = self.cache.get(&cache).copied().unwrap_or_else(|| {
            let components = components(world);
            let bundle = self.register_bundle(world, components);
            self.cache.insert(cache, bundle);
            bundle
        })?;
        buffer_components_inner(world, entity, bundle, now)
    }

    fn register_bundle(
        &mut self,
        world: &mut World,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<BundleId> {
        let components: Vec<ComponentId> = components
            .into_iter()
            .filter(|&component_id| {
                // todo: remove filter and unclonable set when linked issue is fixed
                let component_info = world.components().get_info(component_id).expect("todo");
                let unclonable = component_info.clone_behavior() == &ComponentCloneBehavior::Ignore;
                if unclonable && self.unclonable.insert(component_id) {
                    bevy::log::error!(
                        "Component {} is unclonable, it's insert, remove or overwrite will not \
                        be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
                unclonable
            })
            .collect();

        if components.is_empty() {
            return None;
        }

        Some(world.register_dynamic_bundle(&components).id())
    }
}

fn buffer_components_inner(
    world: &mut World,
    entity: Entity,
    bundle: BundleId,
    now: bool,
) -> Option<Entity> {
    let meta = world.get_resource::<RevMeta>().expect("todo");
    let marker = DespawnAtOutOfLog::new(meta);
    let buffer = world.spawn(marker).id();
    let mut undo_redo = BundleBuffer {
        bundle,
        entity,
        buffer,
        buffered: false,
    };
    if now {
        undo_redo.move_bundle(world);
    }
    world.buffer_undo_redo(undo_redo);
    Some(buffer)
}

struct BundleBuffer {
    bundle: BundleId,
    entity: Entity,
    state: BufferState
}

enum BufferState {
    Unspawned(DespawnAtOutOfLog),
    Spawned(Entity),
    SpawnedAndBuffered(Entity)
}

impl BundleBuffer {
    fn new(bundle: BundleId, entity: Entity, marker: DespawnAtOutOfLog) -> Self {
        Self {
            bundle,
            entity,
            state: BufferState::Unspawned(marker)
        }
    }
    fn move_bundle(&mut self, world: &mut World) {
        let (target, source);
        match self.state {
            BufferState::Unspawned(marker) => {
                target = world.spawn(marker).id();
                source = self.entity;
                self.state = BufferState::SpawnedAndBuffered(target);
            },
            BufferState::Spawned(buffer) => {
                target = buffer;
                source = self.entity;
                self.state = BufferState::SpawnedAndBuffered(buffer);
            },
            BufferState::SpawnedAndBuffered(buffer) => {
                target = self.entity;
                source = buffer;
                self.state = BufferState::Spawned(buffer);
            }
        }

        let bundle = world.bundles().get(self.bundle).expect("todo");
        let components: Box<[ComponentId]> = bundle.explicit_components().into();

        world.insert_resource(BufferComponentsInProgress);
        EntityCloner::build(world)
            .deny_all()
            .move_components(true)
            .without_required_components(|builder| {
                builder.allow_by_ids(components.iter().copied());
            })
            .clone_entity(source, target);
        world.remove_resource::<BufferComponentsInProgress>();
    }
}

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        self.move_bundle(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.move_bundle(world);
    }
}
