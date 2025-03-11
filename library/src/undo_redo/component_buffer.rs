use std::hash::{BuildHasher, Hash, Hasher};

use bevy::{
    ecs::{
        archetype::{Archetype, ArchetypeGeneration, ArchetypeId},
        bundle::BundleId,
        component::{ComponentCloneBehavior, ComponentDescriptor, ComponentId, StorageType},
        entity::{Entity, EntityCloner},
        resource::Resource,
        world::{EntityWorldMut, FromWorld, World},
    },
    platform_support::{
        collections::{HashMap, HashSet},
        hash::{FixedHasher, PassHash},
    },
};
use fixedbitset::FixedBitSet;

use crate::meta::RevMeta;

use super::{BuffersUndoRedo, DespawnAtOutOfLog, UndoRedo};

#[derive(Resource)]
pub(super) struct BufferComponentsInProgress;

#[derive(Resource)]
pub(crate) struct ComponentBufferRes {
    query_states: HashMap<BundleId, BufferQueryState>,
    reservations: HashMap<ComponentId, ComponentId>,
    unclonable: HashSet<ComponentId>,
    empty_with_marker: ArchetypeId,
    cache: HashMap<u64, Option<BundleId>, PassHash>,
    archetypes_buffered_to_this_frame: FixedBitSet,
    buffered_in_archetype_at: u64,
}

impl ComponentBufferRes {
    pub(super) fn buffer_components(
        &mut self,
        world: &mut World,
        entity: Entity,
        components: impl IntoIterator<Item = ComponentId>,
        now: bool,
    ) -> Option<Entity> {
        self.register_bundle(world, components)
            .map(|(bundle, components)| self.apply(world, entity, bundle, components, now))
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
        if let Some(bundle) = self.cache.get(&cache).copied() {
            return bundle.map(|bundle| self.apply(world, entity, bundle, vec![], now));
        }
        let components = components(world);
        let (bundle, buffer) = self
            .register_bundle(world, components)
            .map(|(bundle, components)| {
                (bundle, self.apply(world, entity, bundle, components, now))
            })
            .unzip();
        self.cache.insert(cache, bundle);
        buffer
    }
    pub(super) fn inserted_marker(&mut self, now: u64, archetype_id: ArchetypeId) {
        if self.buffered_in_archetype_at != now {
            self.buffered_in_archetype_at = now;
            self.archetypes_buffered_to_this_frame.clear();
        }
        self.archetypes_buffered_to_this_frame
            .insert(archetype_id.index());
    }
    fn apply(
        &mut self,
        world: &mut World,
        entity: Entity,
        bundle: BundleId,
        components: Vec<ComponentId>,
        now: bool,
    ) -> Entity {
        let meta = world.get_resource::<RevMeta>().expect("todo");
        let now_marker = DespawnAtOutOfLog::new(meta);
        let marker_id = world.component_id::<DespawnAtOutOfLog>().expect("todo");

        let query = self.query_states.entry(bundle).or_insert_with(|| {
            BufferQueryState::new(
                world,
                components,
                &mut self.reservations,
                self.empty_with_marker,
            )
        });
        query.extend_archetypes(world, bundle, marker_id);

        let buffer_archetypes = if now {
            &mut query.without_components
        } else {
            &mut query.without_components_and_reservations
        };
        let buffer_mut = buffer_archetypes.get_buffer_entity(
            world,
            &self.archetypes_buffered_to_this_frame,
            now_marker,
            marker_id,
        );

        let buffer = buffer_mut.id();
        let mut undo_redo = ComponentBuffer {
            components: bundle,
            entity,
            buffer,
            components_buffered: false,
        };

        if now {
            undo_redo.move_components(world);
        } else {
            undo_redo.reserve_components(query.reservations, buffer_mut);
        }
        world.buffer_undo_redo(undo_redo);
        buffer
    }
    fn register_bundle(
        &mut self,
        world: &mut World,
        components: impl IntoIterator<Item = ComponentId>,
    ) -> Option<(BundleId, Vec<ComponentId>)> {
        let components: Vec<ComponentId> = components
            .into_iter()
            .filter(|&component_id| {
                // todo: remove filter and unclonable set when linked issue is fixed
                let component_info = world.components().get_info(component_id).expect("todo");
                let unclonable = component_info.clone_behavior() == &ComponentCloneBehavior::Ignore;
                if unclonable && self.unclonable.insert(component_id) {
                    bevy::log::error!(
                        "Unclonable component {} will be ignored by reversible structural operations, it's insert, remove \
                        or overwrite will not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
                unclonable
            })
            .collect();

        if components.is_empty() {
            return None;
        }

        Some((world.register_dynamic_bundle(&components).id(), components))
    }
}

impl FromWorld for ComponentBufferRes {
    fn from_world(world: &mut World) -> Self {
        let entity = world.spawn(DespawnAtOutOfLog(0));
        let empty_with_marker = entity.archetype().id();
        entity.despawn();
        Self {
            query_states: HashMap::default(),
            reservations: HashMap::default(),
            unclonable: HashSet::default(),
            empty_with_marker,
            cache: HashMap::default(),
            archetypes_buffered_to_this_frame: FixedBitSet::new(),
            buffered_in_archetype_at: 0,
        }
    }
}

struct BufferQueryState {
    without_components: BufferArchetypes,
    without_components_and_reservations: BufferArchetypes,
    reservations: BundleId,
    generation: ArchetypeGeneration,
}

impl BufferQueryState {
    fn new(
        world: &mut World,
        components: Vec<ComponentId>,
        reservations: &mut HashMap<ComponentId, ComponentId>,
        empty_with_marker: ArchetypeId,
    ) -> Self {
        let reservations: Vec<ComponentId> = components
            .into_iter()
            .map(|component_id| {
                *reservations.entry(component_id).or_insert_with(|| {
                    let descriptor = unsafe {
                        // SAFETY: (): Send + Sync + !Drop
                        ComponentDescriptor::new_with_layout(
                            format!(
                                "reservation to buffer {} ({component_id:?})",
                                world
                                    .components()
                                    .get_info(component_id)
                                    .expect("todo")
                                    .name()
                            ),
                            StorageType::Table,
                            std::alloc::Layout::new::<()>(),
                            None, // !Drop
                            false,
                            ComponentCloneBehavior::Ignore,
                        )
                    };
                    world.register_component_with_descriptor(descriptor)
                })
            })
            .collect();
        let reservations = world.register_dynamic_bundle(&reservations).id();
        Self {
            reservations,
            without_components: BufferArchetypes {
                archetypes: vec![empty_with_marker],
                moved_from: 1,
            },
            without_components_and_reservations: BufferArchetypes {
                archetypes: Vec::new(),
                moved_from: 0,
            },
            generation: ArchetypeGeneration::initial(),
        }
    }
    fn extend_archetypes(&mut self, world: &World, bundle: BundleId, marker_id: ComponentId) {
        if world.archetypes().generation() == self.generation {
            return;
        }

        let none_of = |id| {
            let ids = world.bundles().get(id).expect("todo").explicit_components();
            |archetype: &Archetype| ids.iter().all(|id| !archetype.contains(*id))
        };
        let none_of_components = none_of(bundle);
        let none_of_reservations = none_of(self.reservations);

        for archetype in &world.archetypes()[self.generation..] {
            if archetype.contains(marker_id) && none_of_components(archetype) {
                self.without_components.archetypes.push(archetype.id());
                if none_of_reservations(archetype) {
                    self.without_components_and_reservations
                        .archetypes
                        .push(archetype.id());
                }
            }
        }

        self.generation = world.archetypes().generation();
    }
}

struct BufferArchetypes {
    archetypes: Vec<ArchetypeId>,
    moved_from: usize,
}

impl BufferArchetypes {
    fn get_buffer_entity<'a>(
        &mut self,
        world: &'a mut World,
        archetypes_buffered_to_this_frame: &FixedBitSet,
        now_marker: DespawnAtOutOfLog,
        marker_id: ComponentId,
    ) -> EntityWorldMut<'a> {
        let archetypes = self.archetypes.iter().copied().enumerate();
        for (i, archetype_id) in archetypes {
            if !archetypes_buffered_to_this_frame.contains(archetype_id.index()) {
                continue;
            }
            let archetype = world.archetypes().get(archetype_id).expect("todo");
            let table = world
                .storages()
                .tables
                .get(archetype.table_id())
                .expect("todo");
            for archetype_entity in archetype.entities() {
                let ptr = unsafe {
                    // SAFETY: this non-pub resource cannot have been transfered from the world it was created at to another
                    table.get_component(marker_id, archetype_entity.table_row())
                }
                .expect("todo");
                let marker = unsafe {
                    // SAFETY: marker_id was just read from the world for this type
                    ptr.deref::<DespawnAtOutOfLog>()
                };
                if *marker == now_marker {
                    if i >= self.moved_from {
                        self.archetypes.swap(i, self.moved_from);
                        self.moved_from += 1;
                    }
                    return world.entity_mut(archetype_entity.id());
                }
            }
        }

        // spawn a new buffer as no available has been found, the archetype here matches self.archetypes_without_components[0]
        world.spawn(now_marker)
    }
}

#[derive(Debug)]
struct ComponentBuffer {
    components: BundleId,
    entity: Entity,
    buffer: Entity,
    components_buffered: bool,
}

impl ComponentBuffer {
    fn move_components(&mut self, world: &mut World) {
        fn move_components(
            world: &mut World,
            components: &[ComponentId],
            source: Entity,
            target: Entity,
        ) {
            EntityCloner::build(world)
                .deny_all()
                .move_components(true)
                .without_required_components(|builder| {
                    builder.allow_by_ids(components.iter().copied());
                })
                .clone_entity(source, target);
        }

        let bundle = world.bundles().get(self.components).expect("todo");
        let components: Box<[ComponentId]> = bundle.explicit_components().into();

        if self.components_buffered {
            move_components(world, &components, self.buffer, self.entity);
        } else if bundle.required_components().is_empty() {
            move_components(world, &components, self.entity, self.buffer);
        } else {
            // Moving components may cause required components to be generated that are not part of the buffering.
            // To not interfere with the same components to be buffered intentionally or to not make these components
            // show up in case the buffer gets undisabled again, the automatically generated components are removed.
            // This is not needed for the entity from which components get buffered as it is expected that moving the
            // buffered components back to it will already contain the required components from before the buffering.
            let archetype_id = world
                .entities()
                .get(self.buffer)
                .expect("todo")
                .archetype_id;
            let archetype = world.archetypes().get(archetype_id).expect("todo");
            let unwanted: Vec<ComponentId> = bundle
                .required_components()
                .iter()
                .copied()
                .filter(|component_id| !archetype.contains(*component_id))
                .collect();
            move_components(world, &components, self.entity, self.buffer);
            if !unwanted.is_empty() {
                world.entity_mut(self.buffer).remove_by_ids(&unwanted);
            }
        }
        self.components_buffered = !self.components_buffered;
    }

    fn reserve_components(&self, reservations: BundleId, mut buffer: EntityWorldMut) {
        let undo_redo = ReserveComponents {
            reservations,
            buffer: buffer.id(),
        };
        undo_redo.redo_inner(&mut buffer);
        buffer.buffer_undo_redo(undo_redo);
    }
}

impl UndoRedo for ComponentBuffer {
    fn undo(&mut self, world: &mut World) {
        world.insert_resource(BufferComponentsInProgress);
        self.move_components(world);
        world.remove_resource::<BufferComponentsInProgress>();
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world);
    }
}

struct ReserveComponents {
    reservations: BundleId,
    buffer: Entity,
}

impl ReserveComponents {
    fn component_ids(&self, world: &World) -> Box<[ComponentId]> {
        world
            .bundles()
            .get(self.reservations)
            .expect("todo")
            .explicit_components()
            .into()
    }
    fn redo_inner(&self, buffer: &mut EntityWorldMut) {
        let component_ids = self.component_ids(buffer.world());
        let iter = std::iter::repeat_with(|| unsafe {
            // SAFETY: () is a ZST which makes NonNull::dangling a valid pointer to read from regardless of lifetimes
            bevy::ptr::OwningPtr::new(std::ptr::NonNull::dangling())
        })
        .take(component_ids.len());
        unsafe {
            // SAFETY: ids are registered in this world for () that the iterator yields OwningPtr of
            buffer.insert_by_ids(&component_ids, iter);
        }
    }
}

impl UndoRedo for ReserveComponents {
    fn undo(&mut self, world: &mut World) {
        let component_ids = self.component_ids(&world);
        world.entity_mut(self.buffer).remove_by_ids(&component_ids);
    }
    fn redo(&mut self, world: &mut World) {
        self.redo_inner(&mut world.entity_mut(self.buffer));
    }
}
