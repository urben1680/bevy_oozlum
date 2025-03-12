use std::hash::{BuildHasher, Hash, Hasher};

use bevy::{
    ecs::{
        bundle::BundleId,
        component::{ComponentCloneBehavior, ComponentId},
        entity::{Entity, EntityCloner},
        resource::Resource,
        world::World,
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

#[derive(Debug)]
struct BundleBuffer {
    bundle: BundleId,
    entity: Entity,
    buffer: Entity,
    buffered: bool,
}

impl BundleBuffer {
    fn move_bundle(&mut self, world: &mut World) {
        let bundle = world.bundles().get(self.bundle).expect("todo");
        let components: Box<[ComponentId]> = bundle.explicit_components().into();

        if self.buffered {
            move_components(world, &components, self.buffer, self.entity);
        } else {
            move_components(world, &components, self.entity, self.buffer);
        }

        self.buffered = !self.buffered;
    }
}

fn move_components(world: &mut World, components: &[ComponentId], source: Entity, target: Entity) {
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

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        self.move_bundle(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.move_bundle(world);
    }
}
