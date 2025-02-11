use std::{
    any::TypeId,
    sync::{Arc, Weak},
};

use bevy::{
    ecs::{
        archetype::{Archetype, ArchetypeId},
        bundle::{Bundle, BundleId, BundleInfo},
        component::ComponentId,
        entity::Entity,
        resource::Resource,
        world::World,
    },
    platform_support::collections::{hash_map::Entry, HashMap, HashSet},
};

#[derive(Resource, Default)]
pub(crate) struct BundleBuffers {
    weaks: HashMap<(BundleId, ArchetypeId), Weaks>,
    arcs: Arcs,
}

struct Arcs {
    arcs: HashSet<Arc<[ComponentId]>>,
    /// Contains `Arc<[]>` to keep the strong count high enough to be always retained.
    _empty: Arc<[ComponentId]>,
}

impl Default for Arcs {
    fn default() -> Self {
        let empty: Arc<[ComponentId]> = Arc::new([]);
        let set = [empty.clone()].into_iter().collect();
        Self {
            arcs: set,
            _empty: empty,
        }
    }
}

impl Arcs {
    fn insert_keep(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        let components = bundle_info
            .iter_contributed_components()
            .filter(|id| !archetype.contains(*id));
        self.get_or_insert(components)
    }
    fn insert_replace_adds(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        let components = bundle_info
            .iter_required_components()
            .filter(|id| !archetype.contains(*id))
            .chain(bundle_info.iter_explicit_components());
        self.get_or_insert(components)
    }
    fn remove_or_insert_replace_replaces(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        let components = bundle_info
            .iter_explicit_components()
            .filter(|id| archetype.contains(*id));
        self.get_or_insert(components)
    }
    fn remove_with_requires(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        let components = bundle_info
            .iter_contributed_components()
            .filter(|id| archetype.contains(*id));
        self.get_or_insert(components)
    }
    fn get_or_insert(
        &mut self,
        components: impl Iterator<Item = ComponentId>,
    ) -> Arc<[ComponentId]> {
        let mut arc: Arc<[ComponentId]> = components.collect();
        Arc::get_mut(&mut arc).unwrap().sort();
        self.arcs.get_or_insert(arc).clone()
    }
}

/// Store [`Weak`] so cleaning up via the strong count becomes trivial.
struct Weaks {
    insert_keep: Weak<[ComponentId]>,
    insert_replace_adds: Weak<[ComponentId]>,
    remove_or_insert_replace_replaces: Weak<[ComponentId]>,
    remove_with_requires: Weak<[ComponentId]>,
}

impl Default for Weaks {
    fn default() -> Self {
        let arc = Arc::new([]);
        let weak = Arc::downgrade(&arc);
        Self {
            insert_keep: weak.clone(),
            insert_replace_adds: weak.clone(),
            remove_or_insert_replace_replaces: weak.clone(),
            remove_with_requires: weak,
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
pub(super) struct InsertReplace {
    pub(super) adds: Arc<[ComponentId]>,
    pub(super) replaces: Arc<[ComponentId]>,
}

impl BundleBuffers {
    pub(crate) fn retain_used_buffers(&mut self) {
        self.arcs.arcs.retain(|arc| Arc::strong_count(arc) > 1);
        self.weaks.retain(|_, weaks| {
            weaks.insert_keep.strong_count() > 1
                || weaks.insert_replace_adds.strong_count() > 1
                || weaks.remove_or_insert_replace_replaces.strong_count() > 1
        });
    }
    pub(super) fn insert_keep(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        single_arc!(self, insert_keep, bundle_info, archetype)
    }
    pub(super) fn insert_replace(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> InsertReplace {
        match self.weaks.entry((bundle_info.id(), archetype.id())) {
            Entry::Occupied(mut occupied) => {
                let weaks = occupied.get_mut();
                let adds = weaks.insert_replace_adds.upgrade().unwrap_or_else(|| {
                    let arc = self.arcs.insert_replace_adds(bundle_info, archetype);
                    weaks.insert_replace_adds = Arc::downgrade(&arc);
                    arc
                });
                let replaces = weaks
                    .remove_or_insert_replace_replaces
                    .upgrade()
                    .unwrap_or_else(|| {
                        let arc = self
                            .arcs
                            .remove_or_insert_replace_replaces(bundle_info, archetype);
                        weaks.remove_or_insert_replace_replaces = Arc::downgrade(&arc);
                        arc
                    });
                InsertReplace { adds, replaces }
            }
            Entry::Vacant(vacant) => {
                let adds = self.arcs.insert_replace_adds(bundle_info, archetype);
                let replaces = self
                    .arcs
                    .remove_or_insert_replace_replaces(bundle_info, archetype);
                vacant.insert(Weaks {
                    insert_replace_adds: Arc::downgrade(&adds),
                    remove_or_insert_replace_replaces: Arc::downgrade(&replaces),
                    ..Default::default()
                });
                InsertReplace { adds, replaces }
            }
        }
    }
    pub(super) fn remove(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        single_arc!(
            self,
            remove_or_insert_replace_replaces,
            bundle_info,
            archetype
        )
    }
    pub(super) fn remove_with_requires(
        &mut self,
        bundle_info: &BundleInfo,
        archetype: &Archetype,
    ) -> Arc<[ComponentId]> {
        single_arc!(self, remove_with_requires, bundle_info, archetype)
    }
}

macro_rules! single_arc {
    ($self:ident, $variant:ident, $bundle_info:ident, $archetype:ident) => {
        match $self.weaks.entry(($bundle_info.id(), $archetype.id())) {
            Entry::Occupied(mut occupied) => {
                let weaks = occupied.get_mut();
                weaks.$variant.upgrade().unwrap_or_else(|| {
                    let arc = $self.arcs.$variant($bundle_info, $archetype);
                    weaks.$variant = Arc::downgrade(&arc);
                    arc
                })
            }
            Entry::Vacant(vacant) => {
                let arc = $self.arcs.$variant($bundle_info, $archetype);
                vacant.insert(Weaks {
                    $variant: Arc::downgrade(&arc),
                    ..Default::default()
                });
                arc
            }
        }
    };
}

use single_arc;

/// todo workaround until manual bundle registration is possible
pub(super) fn get_bundle_id<T: Bundle>(world: &mut World) -> BundleId {
    #[derive(Resource)]
    struct EmptyEntity(Entity);

    let type_id = TypeId::of::<T>();
    if let Some(id) = world.bundles().get_id(type_id) {
        return id;
    }
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
    world.commands().entity(empty_entity).remove::<T>();
    world.flush();
    world
        .bundles()
        .get_id(type_id)
        .expect("above command should have registered bundle")
}

/*
idea:

ArchetypeId steht für ein set von components im table und sparse sets
BundleId steht für components die eine Bewegung weg vom archetype bedeuten
TableId steht für ein subset von components eines archetypes

damit die slices der UndoRedo geteilt werden müssen sie entweder in einem Arc oder Interned stehen
Arc ermöglich cleanup, was aber wiederum auch häufiger die slices erzeugen muss

Internable zu umständlich, dann direkt &'static [ComponentId] durch Box::leak sammeln und einzigartig halten
*/

struct Foo {
    weaks: HashMap<(BundleId, ArchetypeId), Bar>,
    interner: HashSet<&'static [ComponentId]>,
}

struct Bar {
    insert_keep: &'static [ComponentId],
    insert_replace_adds: &'static [ComponentId],
    remove_or_insert_replace_replaces: &'static [ComponentId],
    remove_with_requires: &'static [ComponentId],
}
