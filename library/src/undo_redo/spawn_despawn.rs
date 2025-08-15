use bevy::{
    ecs::{
        bundle::Bundle,
        change_detection::MaybeLocation,
        component::Component,
        entity::Entity,
        resource::Resource,
        world::{
            EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut,
            FilteredEntityMut, FilteredEntityRef, World,
        },
    },
    log::{error, error_once, once},
    reflect::Reflect,
};

use crate::{
    log::{DenseTransitionsLog, OutOfLog},
    meta::{NonLogNow, RevDirection, RevMeta},
    undo_redo::Spawn,
};

use super::{BufferInProgressRes, BuffersUndoRedo, RevOpInProgress};

pub(crate) fn rev_spawn_finish(
    some_entity_world_mut: &mut EntityWorldMut,
    now: NonLogNow,
    spawned_entity: Entity,
    caller: MaybeLocation,
) {
    some_entity_world_mut.world_scope(|world| {
        world.buffer_undo_redo(
            now,
            Spawn {
                entity: spawned_entity,
                location: caller,
            },
        );
        world
            .resource_mut::<RevDespawnCleaner>()
            .log_spawn(spawned_entity, caller, now);
    });
}

#[derive(Resource, Default, Reflect)]
pub(crate) struct RevDespawnCleaner {
    spawn: DenseTransitionsLog<Entity>,
    despawn: DenseTransitionsLog<Entity>,
    spawn_buffer: DenseTransitionsLog<(Option<Entity>, MaybeLocation)>,
    spawn_queue: Vec<(Entity, MaybeLocation)>,
    despawn_queue: Vec<(Entity, MaybeLocation)>,
    spawn_buffer_queue: Vec<(Option<Entity>, MaybeLocation)>,
    spawn_buffer_queue_fallback: Vec<(Entity, MaybeLocation)>,
}

impl RevDespawnCleaner {
    pub(crate) fn log_spawn(&mut self, entity: Entity, location: MaybeLocation, _: NonLogNow) {
        self.spawn_queue.push((entity, location));
    }

    pub(crate) fn log_spawn_batch(
        &mut self,
        entities: &[Entity],
        location: MaybeLocation,
        _: NonLogNow,
    ) {
        self.spawn_queue
            .extend(entities.iter().copied().map(|entity| (entity, location)));
    }

    pub(crate) fn log_despawn(&mut self, entity: Entity, location: MaybeLocation, _: NonLogNow) {
        self.despawn_queue.push((entity, location));
    }

    /// Must be called during [`RevDirection::NOT_LOG`] or [`RevDirection::BackwardLog`].
    ///
    /// At the latter, `buffer` must be `Some` and the current frame's [`RevDirection::NOT_LOG`]
    /// run must have called this with `None` to reserve the entity that is pushed here.
    pub(crate) fn log_spawn_buffer(&mut self, buffer: Option<Entity>, location: MaybeLocation) {
        self.spawn_buffer_queue.push((buffer, location));
    }

    /// Despawn entities that contain [`RevDespawned`] and their relevant operation (spawn, despawn, move_components) fell out of log.
    pub(crate) fn update(&mut self, meta: &RevMeta, world: &mut World) -> Result<(), OutOfLog> {
        match meta.running_direction() {
            RevDirection::NOT_LOG => Ok(self.forward(world, meta.past_len() as usize)),
            RevDirection::FORWARD_LOG => self.forward_log(),
            RevDirection::BackwardLog => self.backward_log(),
        }
    }

    fn forward(&mut self, world: &mut World, max_past_len: usize) {
        let progress = RevOpInProgress::FinalDespawn { buffer: false };
        progress.scope(world, |world| {
            let (despawned, _) = self.spawn.drain_future();
            for entity in despawned {
                let _ = world.try_despawn(entity); // todo: upstream a way to set the location
            }
            self.spawn.push_and_pop_past(max_past_len, |mut log| {
                log.extend(self.spawn_queue.drain(..).map(|(entity, _)| entity));
            });

            let despawned = self
                .despawn
                .push_and_pop_past(max_past_len, |mut log| {
                    log.extend(self.despawn_queue.drain(..).map(|(entity, _)| entity));
                })
                .into_iter()
                .flat_map(|value_entry| value_entry.value);
            for entity in despawned {
                let _ = world.try_despawn(entity); // todo: upstream a way to set the location
            }

            world.insert_resource(BufferInProgressRes(RevOpInProgress::FinalDespawn {
                buffer: true,
            }));

            let despawned = self
                .spawn_buffer
                .drain_future()
                .0
                .flat_map(|(entity, _)| entity);
            for entity in despawned {
                let _ = world.try_despawn(entity); // todo: upstream a way to set the location
            }
            self.spawn_buffer
                .push_and_pop_past(max_past_len, |mut log| {
                    log.extend(self.spawn_buffer_queue.drain(..));
                    log.extend(
                        self.spawn_buffer_queue_fallback
                            .drain(..)
                            .map(|(entity, location)| (Some(entity), location)),
                    );
                });
        });
    }

    fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.spawn.forward_log()?;
        let _ok = self.despawn.forward_log();
        let _ok = self.spawn_buffer.forward_log();

        if size_of::<MaybeLocation>() == 0 {
            if !self.spawn_queue.is_empty() {
                error_once!(
                    "a reversible spawn was queued during the forward log direction instead of the non-log direction"
                );
            }
            if !self.despawn_queue.is_empty() {
                error_once!(
                    "a reversible despawn was queued during the forward log direction instead of the non-log direction"
                );
            }
            if !self.spawn_buffer_queue.is_empty() {
                error_once!(
                    "an internal buffer entity was queued during the forward log direction"
                );
            }
        } else {
            for (_, location) in &self.spawn_queue {
                error!(
                    "a reversible spawn was queued during the forward log direction instead of the non-log direction at {location}"
                );
            }
            for (_, location) in &self.despawn_queue {
                error!(
                    "a reversible despawn was queued during the forward log direction instead of the non-log direction at {location}"
                );
            }
            for (_, location) in &self.spawn_buffer_queue {
                error!(
                    "an internal buffer entity was queued during the forward log direction at {location}"
                );
            }
        }

        Ok(())
    }

    fn backward_log(&mut self) -> Result<(), OutOfLog> {
        self.spawn.backward_log()?;
        let _ok = self.despawn.backward_log();
        let mut buffer_iter = self
            .spawn_buffer
            .backward_log()
            .unwrap() // should not be out-of-log like `Self::spawn` as they get called identically in `forward`
            .value;
        let mut delayed_iter = self.spawn_buffer_queue.drain(..);

        'delayed_loop: for (delayed, location) in delayed_iter.by_ref() {
            let Some(delayed) = delayed else {
                match location.into_option() {
                    Some(location) => error!(
                        "an empty internal buffer entity was reserved during the backward direction instead of the non-log direction at {location}"
                    ),
                    None => error_once!(
                        "an empty internal buffer entity was reserved during the backward direction instead of the non-log direction"
                    ),
                }
                continue;
            };

            for buffer in buffer_iter.by_ref() {
                if buffer.0.is_none() {
                    buffer.0 = Some(delayed);
                    continue 'delayed_loop;
                }
            }

            // fallback: postpone to later frame to prevent leak
            self.spawn_buffer_queue_fallback.push((delayed, location));

            match location.into_option() {
                Some(location) => error!(
                    "an internal buffer entity was attempted to be logged during the backward direction at {location} but no slot was reserved"
                ),
                None => error_once!(
                    "an internal buffer entity was attempted to be logged during the backward direction but no slot was reserved"
                ),
            }
        }

        if size_of::<MaybeLocation>() == 0 {
            if !self.spawn_queue.is_empty() {
                error_once!(
                    "a reversible spawn was queued during the backward direction instead of the non-log direction"
                );
            }
            if !self.despawn_queue.is_empty() {
                error_once!(
                    "a reversible despawn was queued during the backward direction instead of the non-log direction"
                );
            }
            once!(if buffer_iter.any(|buffer| buffer.0.is_none()) {
                error!(
                    "an internal buffer entity reservation remained unclaimed during the backward direction"
                );
            });
        } else {
            for (_, location) in &self.spawn_queue {
                error!(
                    "a reversible spawn was queued during the backward direction instead of the non-log direction at {location}"
                );
            }
            for (_, location) in &self.despawn_queue {
                error!(
                    "a reversible despawn was queued during the backward direction instead of the non-log direction at {location}"
                );
            }
            for (buffer, location) in buffer_iter {
                if buffer.is_none() {
                    error!(
                        "an internal buffer entity reservation remained unclaimed during the backward direction at {location}"
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Component, Debug, Reflect)]
#[component(immutable)]
pub(crate) struct RevDespawned; // todo: store MaybeLocation in component change meta

pub trait RevIsDespawned {
    fn is_rev_despawned(&self) -> bool;
}

/* todo: implement for entity pointers when https://github.com/bevyengine/bevy/issues/20494 landed
pub trait RevDespawnedBy {
    fn get_rev_despawned_by(&self) -> Option<MaybeLocation>;
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>>;
}
*/

impl RevIsDespawned for EntityRef<'_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityRefExcept<'_, '_, B> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl RevIsDespawned for FilteredEntityRef<'_, '_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl RevIsDespawned for EntityMut<'_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityMutExcept<'_, '_, B> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl RevIsDespawned for FilteredEntityMut<'_, '_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

impl RevIsDespawned for EntityWorldMut<'_> {
    fn is_rev_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // todo
}
