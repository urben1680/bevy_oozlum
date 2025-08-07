use std::{any::TypeId, panic::Location};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, BundleId, InsertMode},
        change_detection::MaybeLocation,
        component::ComponentId,
        entity::{Entity, EntityCloner},
        resource::Resource,
        world::{EntityWorldMut, World},
    },
    platform::collections::{HashMap, HashSet},
};

use crate::meta::{NonLogNow, RevDirection};

use super::{
    BuffersUndoRedo, EntityRevDespawnedError, ResourceSwap, RevDespawnCleaner, RevDespawned,
    UndoRedo,
};

// todo: move to entity_world module except RevOpInProgress + friends
// does that work out with MaybeLocation of rev commands?
// alternativeley, move all logic to commands with correct location passing and make RevEntityWorldMut generate and apply them
// make all RevEntityWorldMut methods check for rev_despawned

pub(super) fn try_rev_clear(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    location: MaybeLocation,
) -> Result<(), EntityRevDespawnedError> {
    check_spawned(&entity)?;
    let id = entity.id();
    entity.world_scope(|world| {
        let mut buffer = BundleBuffer::new((), id, location);
        let mut cloner = ().cloner(world);
        let entities = buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
        world.buffer_undo_redo(now, buffer);
    });
    Ok(())
}

pub(super) fn try_rev_remove<T: Bundle, const WITH_REQUIRED: bool>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    location: MaybeLocation,
) -> Result<(), EntityRevDespawnedError> {
    check_spawned(&entity)?;
    let id = entity.id();
    entity.world_scope(|world| {
        let bundle_id = world.register_bundle::<T>().id();
        let cloner_builder = BundleIdCloner::<WITH_REQUIRED>(bundle_id);
        let mut buffer = BundleBuffer::new(cloner_builder, id, location);
        let mut cloner = cloner_builder.cloner(world);
        let entities = buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
        world.buffer_undo_redo(now, buffer);
    });
    Ok(())
}

pub(super) fn try_rev_retain<T: Bundle>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    location: MaybeLocation,
) -> Result<(), EntityRevDespawnedError> {
    let archetype = check_spawned(&entity)?;
    let id = entity.id();
    entity.world_scope(|world| {
        let bundle_id = world.resource_scope::<BundleIdOfOpCache, _>(|world, mut res| {
            res.get_retain::<T>(world, archetype)
        });
        // using an opt-out cloner does not work because moving would not opt out of required but required_by components of bundle components
        let cloner_builder = BundleIdCloner::<false>(bundle_id);
        let mut buffer = BundleBuffer::new(cloner_builder, id, location);
        let mut cloner = cloner_builder.cloner(world);
        let entities = buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
        world.buffer_undo_redo(now, buffer);
    });
    Ok(())
}

pub(super) fn try_rev_insert<T: Bundle>(
    entity: &mut EntityWorldMut,
    bundle: T,
    mut insert_mode: InsertMode,
    now: NonLogNow,
    location: MaybeLocation,
) -> Result<(), EntityRevDespawnedError> {
    let archetype = check_spawned(&entity)?;
    let id = entity.id();
    entity.world_scope(|world| { // todo: manually do the logic of world_scope without a closure so #[track_caller] enters below logic
        let bundle_id =
            world.resource_scope::<BundleIdOfOpCache, _>(|world, mut res| match insert_mode {
                InsertMode::Replace => {
                    let (bundle_id, updated_insert_mode) = res.get_insert::<T>(world, archetype);
                    insert_mode = updated_insert_mode; // when there is nothing to replace, simplify to `Keep`
                    bundle_id
                }
                InsertMode::Keep => res.get_insert_if_new::<T>(world, archetype),
            });
        let cloner_builder = BundleIdCloner::<false>(bundle_id);
        let mut buffer = BundleBuffer::new(cloner_builder, id, location);
        world
            .resource_mut::<RevDespawnCleaner>()
            .log_spawn_buffer(None, location); // reserve log entry for buffer of inserted components
        match insert_mode {
            InsertMode::Replace => {
                let mut cloner = cloner_builder.cloner(world);
                // here the `buffer` is the buffer for the overwritten components...
                let entities = buffer.toggle_state(world);
                let backup_buffer = entities.buffer;
                entities.move_components(world, &mut cloner, RevDirection::NOT_LOG);
                // ...here `buffer` becomes the buffer for the inserted components
                buffer.state = BufferState::Unspawned(location);
                let buffer = BundleBufferReplace {
                    backup_buffer,
                    insert_buffer: buffer,
                };
                world.buffer_undo_redo(now, buffer);
                world.entity_mut(id).insert(bundle); // todo: upstream a way to set the location
            }
            InsertMode::Keep => {
                world.buffer_undo_redo(now, buffer);
                world.entity_mut(id).insert_if_new(bundle); // todo: upstream a way to set the location
            }
        }
    });
    Ok(())
}

#[derive(Resource, Default)]
struct BundleIdOfOpCache {
    insert: HashMap<(ArchetypeId, TypeId), (BundleId, InsertMode)>,
    insert_if_new: HashMap<(ArchetypeId, TypeId), BundleId>,
    retain: HashMap<(ArchetypeId, TypeId), BundleId>,
}

impl BundleIdOfOpCache {
    fn get_insert<T: Bundle>(
        &mut self,
        world: &mut World,
        archetype_id: ArchetypeId,
    ) -> (BundleId, InsertMode) {
        let key = (archetype_id, TypeId::of::<T>());
        *self.insert.entry(key).or_insert_with(|| {
            // Bundle explicit:  A(2), B(2), C(2)
            // Bundle required:                    D(2), E(2)

            // Entity before:    A(1), B(1),             E(1)
            // Entity after:     A(2), B(2), C(2), D(2), E(1)

            // Buffer 1:         A(1), B(1), C(*), D(*), E(_)
            // Buffer 2 at undo: A(2), B(2), C(2), D(2), E(_)

            // * = if any appear at redo, _ = unused default

            let bundle_id = world.register_bundle::<T>().id();
            let bundle = world.bundles().get(bundle_id).unwrap();
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let components: Vec<_> = bundle
                .explicit_components()
                .iter()
                .chain(
                    bundle
                        .required_components()
                        .iter()
                        .filter(|component_id| !archetype.contains(**component_id)),
                )
                .copied()
                .collect();
            let overwrites = bundle
                .explicit_components()
                .iter()
                .any(|component_id| archetype.contains(*component_id));
            let bundle = world.register_dynamic_bundle(&components).id();

            if overwrites {
                return (bundle, InsertMode::Replace);
            }

            self.insert_if_new.insert(key, bundle);
            (bundle, InsertMode::Keep)
        })
    }
    fn get_insert_if_new<T: Bundle>(
        &mut self,
        world: &mut World,
        archetype_id: ArchetypeId,
    ) -> BundleId {
        let key = (archetype_id, TypeId::of::<T>());
        *self.insert_if_new.entry(key).or_insert_with(|| {
            // Bundle explicit:  A(2), B(2), C(2)
            // Bundle required:                    D(2), E(2)

            // Entity before:    A(1), B(1),             E(1)
            // Entity after:     A(1), B(1), C(2), D(2), E(1)

            // Buffer at undo:               C(2), D(2), E(_)

            // _ = unused default

            let bundle_id = world.register_bundle::<T>().id();
            let bundle = world.bundles().get(bundle_id).unwrap();
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let components: Vec<_> = bundle
                .contributed_components()
                .iter()
                .copied()
                .filter(|component_id| !archetype.contains(*component_id))
                .collect();
            world.register_dynamic_bundle(&components).id()
        })
    }
    fn get_retain<T: Bundle>(&mut self, world: &mut World, archetype_id: ArchetypeId) -> BundleId {
        let key = (archetype_id, TypeId::of::<T>());
        *self.retain.entry(key).or_insert_with(|| {
            let bundle_id = world.register_bundle::<T>().id();
            let bundle_components: HashSet<ComponentId> = world
                .bundles()
                .get(bundle_id)
                .unwrap()
                .contributed_components()
                .iter()
                .copied()
                .collect();
            let archetype = world.archetypes().get(archetype_id).unwrap();
            let components: Vec<_> = archetype
                .components()
                .filter(|component_id| !bundle_components.contains(component_id))
                .collect();
            world.register_dynamic_bundle(&components).id()
        })
    }
}

fn check_spawned(entity: &EntityWorldMut) -> Result<ArchetypeId, EntityRevDespawnedError> {
    let id = entity.id();
    if let Some(despawned) = entity.get::<RevDespawned>() {
        return Err(EntityRevDespawnedError {
            entity: id,
            location: despawned.0,
        });
    };
    Ok(entity.location().archetype_id)
}

struct BundleBuffer<Cloner> {
    cloner_builder: Cloner,
    entity: Entity,
    state: BufferState,
}

trait ClonerBuilder: Send + 'static {
    fn cloner(&self, world: &mut World) -> EntityCloner;
}

#[derive(Clone, Copy)]
struct BundleIdCloner<const WITH_REQUIRED: bool>(BundleId);

impl<const WITH_REQUIRED: bool> ClonerBuilder for BundleIdCloner<WITH_REQUIRED> {
    fn cloner(&self, world: &mut World) -> EntityCloner {
        let mut builder = EntityCloner::build_opt_in(world);
        builder
            .move_components(true)
            .without_required_components(|builder| {
                if WITH_REQUIRED {
                    builder.allow_by_ids(self.0);
                } else {
                    builder.allow_by_ids_if_new(self.0);
                }
            });
        builder.finish()
    }
}

impl ClonerBuilder for () {
    fn cloner(&self, world: &mut World) -> EntityCloner {
        let mut builder = EntityCloner::build_opt_out(world);
        builder.move_components(true);
        builder.finish()
    }
}

#[derive(Copy, Clone)]
enum BufferState {
    Unspawned(MaybeLocation),
    Empty(Entity),
    Filled(Entity),
}

struct BundleEntities {
    target: Entity,
    source: Entity,
    buffer: Entity,
}

impl BundleEntities {
    fn move_components(
        self,
        world: &mut World,
        cloner: &mut EntityCloner,
        direction: RevDirection,
    ) {
        let progress = RevOpInProgress::Buffer {
            direction,
            buffer: self.buffer,
        };
        progress.scope(world, |world| {
            cloner.clone_entity(world, self.source, self.target);
        })
    }
}

impl<Cloner: ClonerBuilder> BundleBuffer<Cloner> {
    fn new(cloner_builder: Cloner, entity: Entity, location: MaybeLocation) -> Self {
        Self {
            cloner_builder,
            entity,
            state: BufferState::Unspawned(location),
        }
    }
    fn toggle_state(&mut self, world: &mut World) -> BundleEntities {
        match self.state {
            BufferState::Unspawned(location) => {
                let buffer = world.spawn(RevDespawned(location)).id();
                world
                    .resource_mut::<RevDespawnCleaner>()
                    .log_spawn_buffer(Some(buffer), location);
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
    fn undo_redo(&mut self, world: &mut World, direction: RevDirection) {
        let mut cloner = self.cloner_builder.cloner(world);
        let entities = self.toggle_state(world);
        entities.move_components(world, &mut cloner, direction);
    }
}

impl<Cloner: ClonerBuilder> UndoRedo for BundleBuffer<Cloner> {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world, RevDirection::BackwardLog);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world, RevDirection::FORWARD_LOG);
    }
}

struct BundleBufferReplace {
    backup_buffer: Entity,
    insert_buffer: BundleBuffer<BundleIdCloner<false>>,
}

impl UndoRedo for BundleBufferReplace {
    fn undo(&mut self, world: &mut World) {
        let mut cloner = self.insert_buffer.cloner_builder.cloner(world);

        // move inserted components from the entity into the insert_buffer
        let entities = self.insert_buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::BackwardLog);

        // move backuped components from the backup_buffer into the entity
        let entities = BundleEntities {
            source: self.backup_buffer,
            target: self.insert_buffer.entity,
            buffer: self.backup_buffer,
        };
        entities.move_components(world, &mut cloner, RevDirection::BackwardLog);
    }
    fn redo(&mut self, world: &mut World) {
        let mut cloner = self.insert_buffer.cloner_builder.cloner(world);

        // move backuped components from the entity into the backup_buffer
        let entities = BundleEntities {
            source: self.insert_buffer.entity,
            target: self.backup_buffer,
            buffer: self.backup_buffer,
        };
        entities.move_components(world, &mut cloner, RevDirection::FORWARD_LOG);

        // move inserted components from the insert_buffer into the entity
        let entities = self.insert_buffer.toggle_state(world);
        entities.move_components(world, &mut cloner, RevDirection::FORWARD_LOG);
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RevOpInProgress {
    Buffer {
        direction: RevDirection,
        buffer: Entity,
    },
    FinalDespawn {
        buffer: bool,
    },
}

impl RevOpInProgress {
    pub fn check(world: &World) -> Option<Self> {
        world.get_resource::<BufferInProgressRes>().map(|res| res.0)
    }
    pub fn direction(self) -> RevDirection {
        match self {
            Self::Buffer { direction, .. } => direction,
            Self::FinalDespawn { .. } => RevDirection::NOT_LOG,
        }
    }
    pub(crate) fn scope(
        self,
        world: &mut World,
        c: impl FnOnce(&mut World),
    ) {
        let mut swap = ResourceSwap(Some(BufferInProgressRes(self)));
        swap.redo(world);
        c(world);
        swap.undo(world);
    }
}

#[derive(Resource)]
pub(super) struct BufferInProgressRes(pub(super) RevOpInProgress);

#[cfg(test)]
mod test {
    use super::*;
    //todo
}
