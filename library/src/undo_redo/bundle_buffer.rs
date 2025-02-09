/*
structure that contains (Bundle, Archetype) -> [&[ComponentId]; 3] mapping
[&[ComponentId]; 3] ideally are unique too, like Interned does
UndoRedo reference into values of this

ideas of stored types in UndoRedo:
- Box<[ComponentId]>
-- cloned from resource
- Arc<[ComponentId]>
-- cloned from resource
- Interned
-- cloned from resource (no cleanup)
- usize
-- index into Vec in resource (no cleanup)
- HashMap keys
-- index into Map in resource

cleanup or not?

cleanup adds runtime overhead since Arc counters need to be modified
ideally there is only one arc in the resource and references in the UndoRedo
Weak still has counters and checking upgrade is not free either

cleanup without internal index logic:

- one Map<(BundleId, ArchetypeId), Arcs> field
- drop if key elements are no longer valid to be future proof of cleanup logic
- drop if all Weak values have a strong count of 1
-
*/

use std::{
    any::TypeId,
    sync::{Arc, Weak},
};

use bevy::{
    ecs::{
        archetype::{Archetype, ArchetypeId},
        bundle::{Bundle, BundleId, BundleInfo},
        component::ComponentId,
        entity::{Entity, EntityLocation},
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
    // always retain the empty slice by keeping the strong count > 1
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
        world: &World,
        bundle_info: &BundleInfo,
        archetype_id: ArchetypeId,
    ) -> Arc<[ComponentId]> {
        self.inner(
            world,
            bundle_info,
            archetype_id,
            |bundle_info, archetype| {
                bundle_info
                    .iter_contributed_components()
                    .filter(|id| !archetype.contains(*id))
            },
        )
    }
    fn insert_replace_adds(
        &mut self,
        world: &World,
        bundle_info: &BundleInfo,
        archetype_id: ArchetypeId,
    ) -> Arc<[ComponentId]> {
        self.inner(
            world,
            bundle_info,
            archetype_id,
            |bundle_info, archetype| {
                bundle_info
                    .iter_required_components()
                    .filter(|id| !archetype.contains(*id))
                    .chain(bundle_info.iter_explicit_components())
            },
        )
    }
    fn insert_replace_replaces(
        &mut self,
        world: &World,
        bundle_info: &BundleInfo,
        archetype_id: ArchetypeId,
    ) -> Arc<[ComponentId]> {
        self.inner(
            world,
            bundle_info,
            archetype_id,
            |bundle_info, archetype| {
                bundle_info
                    .iter_explicit_components()
                    .filter(|id| archetype.contains(*id))
            },
        )
    }
    fn inner<'a, I: Iterator<Item = ComponentId> + 'a>(
        &mut self,
        world: &'a World,
        bundle_info: &'a BundleInfo,
        archetype_id: ArchetypeId,
        c: impl FnOnce(&'a BundleInfo, &'a Archetype) -> I,
    ) -> Arc<[ComponentId]> {
        let archetype = world.archetypes().get(archetype_id).expect("todo");
        let components = c(bundle_info, archetype).collect();
        self.arcs.get_or_insert(components).clone()
    }
}

/// Store [`Weak`] so cleaning up via the strong count becomes trivial.
struct Weaks {
    insert_keep_adds: Weak<[ComponentId]>,
    insert_replace_adds: Weak<[ComponentId]>,
    insert_replace_replaces: Weak<[ComponentId]>,
}

impl Weaks {
    fn retain(&self) -> bool {
        [
            &self.insert_keep_adds,
            &self.insert_replace_adds,
            &self.insert_replace_replaces,
        ]
        .into_iter()
        .any(|weak| weak.strong_count() > 1)
    }
}

impl Default for Weaks {
    fn default() -> Self {
        let arc = Arc::new([]);
        let weak = || Arc::downgrade(&arc);
        Self {
            insert_keep_adds: weak(),
            insert_replace_adds: weak(),
            insert_replace_replaces: weak(),
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
        self.weaks.retain(|_, components| components.retain());
    }
    pub(super) fn insert_keep(
        &mut self,
        world: &World,
        bundle_info: &BundleInfo,
        archetype_id: ArchetypeId,
    ) -> Arc<[ComponentId]> {
        match self.weaks.entry((bundle_info.id(), archetype_id)) {
            Entry::Occupied(mut occupied) => {
                let components = occupied.get_mut();
                components.insert_keep_adds.upgrade().unwrap_or_else(|| {
                    let arc = self.arcs.insert_keep(world, bundle_info, archetype_id);
                    components.insert_keep_adds = Arc::downgrade(&arc);
                    arc
                })
            }
            Entry::Vacant(vacant) => {
                let arc = self.arcs.insert_keep(world, bundle_info, archetype_id);
                vacant.insert(Weaks {
                    insert_keep_adds: Arc::downgrade(&arc),
                    ..Default::default()
                });
                arc
            }
        }
    }
    pub(super) fn insert_replace(
        &mut self,
        world: &World,
        bundle_info: &BundleInfo,
        archetype_id: ArchetypeId,
    ) -> InsertReplace {
        match self.weaks.entry((bundle_info.id(), archetype_id)) {
            Entry::Occupied(mut occupied) => {
                let components = occupied.get_mut();
                let adds = components.insert_replace_adds.upgrade().unwrap_or_else(|| {
                    let arc = self
                        .arcs
                        .insert_replace_adds(world, bundle_info, archetype_id);
                    components.insert_replace_adds = Arc::downgrade(&arc);
                    arc
                });
                let replaces = components
                    .insert_replace_replaces
                    .upgrade()
                    .unwrap_or_else(|| {
                        let arc =
                            self.arcs
                                .insert_replace_replaces(world, bundle_info, archetype_id);
                        components.insert_replace_replaces = Arc::downgrade(&arc);
                        arc
                    });
                InsertReplace { adds, replaces }
            }
            Entry::Vacant(vacant) => {
                let adds = self
                    .arcs
                    .insert_replace_adds(world, bundle_info, archetype_id);
                let replaces = self
                    .arcs
                    .insert_replace_replaces(world, bundle_info, archetype_id);
                vacant.insert(Weaks {
                    insert_replace_adds: Arc::downgrade(&adds),
                    insert_replace_replaces: Arc::downgrade(&replaces),
                    ..Default::default()
                });
                InsertReplace { adds, replaces }
            }
        }
    }
}

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
