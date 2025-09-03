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
    log::{error, error_once},
    reflect::Reflect,
};

use crate::{
    log::{TransitionsLog, OutOfLog, PreUpdateVariant},
    meta::{NonLogNow, RevDirection, RevMeta},
};

use super::RevOp;

#[derive(Resource, Default, Debug)]
pub(crate) struct RevDespawnCleaner {
    spawn: TransitionsLog<Entity>,
    despawn: TransitionsLog<Entity>,
    spawn_buffer: TransitionsLog<(Option<Entity>, MaybeLocation)>,
    spawn_queue: Vec<(Entity, MaybeLocation)>,
    despawn_queue: Vec<(Entity, MaybeLocation)>,
    spawn_buffer_queue: Vec<(Option<Entity>, MaybeLocation)>,
    spawn_buffer_queue_fallback: Vec<(Entity, MaybeLocation)>,
}

pub(crate) enum RevDespawnCleanerErr {
    OutOfLog(RevMeta),
    RevMetaMissing,
    RevMetaNotRunning(RevMeta)
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

    pub(crate) fn log_spawn_buffer_batch_reserve(
        &mut self,
        buffers: usize,
        location: MaybeLocation,
    ) {
        self.spawn_buffer_queue
            .extend(core::iter::repeat_n((None, location), buffers));
    }

    pub(crate) fn log_spawn_buffer_batch(&mut self, buffers: &[Entity], location: MaybeLocation) {
        self.spawn_buffer_queue
            .extend(buffers.into_iter().map(|buffer| (Some(*buffer), location)));
    }

    /// Despawn entities that contain [`RevDespawned`] and their relevant operation (spawn, despawn, move_components) fell out of log.
    pub(crate) fn update_get_meta(&mut self, world: &mut World, not_running: &mut bool, out_of_log: &mut bool) -> Option<RevMeta> {
        let meta = world.remove_resource::<RevMeta>()?;
        let direction = match meta.get_running_direction() {
            Some(direction) => direction,
            None => {
                *not_running = true;
                return Some(meta);
            }
        };
        let past_len = meta.past_len() as usize;
        let iter = self
            .spawn_buffer
            .pre_update_drain(&meta)
            .0
            .flat_map(|buffer| buffer.0)
            .chain(self.spawn.pre_update_drain_future(&meta).0)
            .chain(self.despawn.pre_update_drain_past(&meta).0);

        for entity in iter {
            let _ = world.try_despawn(entity); // todo: upstream a way to set the location
        }
        
        let log_result = match direction {
            RevDirection::NOT_LOG => Ok(self.forward(world, past_len)),
            RevDirection::FORWARD_LOG => self.forward_log(),
            RevDirection::BackwardLog => self.backward_log(),
        };
        *out_of_log = log_result.is_err();
        Some(meta)
    }

    pub(crate) fn forward(&mut self, world: &mut World, max_past_len: usize) {
        let progress = RevOp::FinalDespawn { buffer: false };
        progress.scope(world, |world| {
            self.spawn.push_and_truncate_past(max_past_len, |mut log| {
                log.extend(self.spawn_queue.drain(..).map(|(entity, _)| entity));
            });

            let despawned = self
                .despawn
                .push_and_drain_past(max_past_len, |mut log| {
                    log.extend(self.despawn_queue.drain(..).map(|(entity, _)| entity));
                })
                .0;
            for entity in despawned {
                let _ = world.try_despawn(entity); // todo: upstream a way to set the location
            }

            world.insert_resource(RevOp::FinalDespawn { buffer: true });

            let buffer = self
                .spawn_buffer
                .push_and_drain_past(max_past_len, |mut log| {
                    log.extend(self.spawn_buffer_queue.drain(..));
                    log.extend(
                        self.spawn_buffer_queue_fallback
                            .drain(..)
                            .map(|(entity, location)| (Some(entity), location)),
                    );
                })
                .0
                .flat_map(|buffer| buffer.0);
            for entity in buffer {
                let _ = world.try_despawn(entity); // todo: upstream a way to set the location
            }
        });
    }

    pub(crate) fn forward_log(&mut self) -> Result<(), OutOfLog> {
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

    pub(crate) fn backward_log(&mut self) -> Result<(), OutOfLog> {
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
            {
                use core::sync::atomic::{AtomicBool, Ordering};
                static SKIP_ITER_CHECK: AtomicBool = AtomicBool::new(false);
                if !SKIP_ITER_CHECK.load(Ordering::Relaxed) {
                    if buffer_iter.any(|buffer| buffer.0.is_none()) {
                        SKIP_ITER_CHECK.store(true, Ordering::Relaxed);
                        error!(
                            "an internal buffer entity reservation remained unclaimed during the backward direction"
                        );
                    }
                }
            }
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

#[derive(Component, Debug, Reflect, Clone, Copy)]
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
    use crate::panic_on_error_events;

    use super::*;

    #[test]
    fn log_traversal_works() {
        panic_on_error_events();
        let mut world = World::new();
        let mut cleaner = RevDespawnCleaner::default();

        // do 1
        let spawn_at_1_despawn_at_2 = world.spawn_empty().id();
        cleaner.log_spawn(
            spawn_at_1_despawn_at_2,
            MaybeLocation::caller(),
            NonLogNow(1),
        );
        cleaner.forward(&mut world, 0);
        assert!(
            world.get_entity(spawn_at_1_despawn_at_2).is_ok(),
            "{cleaner:#?}"
        );

        // do 2
        let spawn_at_2 = world.spawn_empty().id();
        let spawn_buffer_at_2 = world.spawn_empty().id();
        let reserve_buffer_at_2;
        cleaner.log_spawn(spawn_at_2, MaybeLocation::caller(), NonLogNow(2));
        cleaner.log_despawn(
            spawn_at_1_despawn_at_2,
            MaybeLocation::caller(),
            NonLogNow(2),
        );
        cleaner.log_spawn_buffer(Some(spawn_buffer_at_2), MaybeLocation::caller());
        cleaner.log_spawn_buffer(None, MaybeLocation::caller());
        cleaner.forward(&mut world, 1);
        assert!(
            world.get_entity(spawn_at_1_despawn_at_2).is_ok(),
            "{cleaner:#?}"
        );
        assert!(world.get_entity(spawn_at_2).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_buffer_at_2).is_ok(), "{cleaner:#?}");

        // undo 2
        reserve_buffer_at_2 = world.spawn_empty().id();
        cleaner.log_spawn_buffer(Some(reserve_buffer_at_2), MaybeLocation::caller());
        cleaner.backward_log().unwrap();

        // redo 2
        cleaner.forward_log().unwrap();

        // do 3
        let spawn_at_3 = world.spawn_empty().id();
        cleaner.log_spawn(spawn_at_3, MaybeLocation::caller(), NonLogNow(3));
        cleaner.forward(&mut world, 1);
        assert!(
            world.get_entity(spawn_at_1_despawn_at_2).is_ok(),
            "{cleaner:#?}"
        );
        assert!(world.get_entity(spawn_at_2).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_buffer_at_2).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_at_3).is_ok(), "{cleaner:#?}");

        // do 4
        let spawn_at_4 = world.spawn_empty().id();
        let spawn_buffer_at_4 = world.spawn_empty().id();
        cleaner.log_spawn(spawn_at_4, MaybeLocation::caller(), NonLogNow(4));
        cleaner.log_spawn_buffer(Some(spawn_buffer_at_4), MaybeLocation::caller());
        cleaner.forward(&mut world, 1);
        assert!(
            world.get_entity(spawn_at_1_despawn_at_2).is_err(),
            "{cleaner:#?}"
        ); // finalized despawn_at_2
        assert!(world.get_entity(spawn_at_2).is_ok(), "{cleaner:#?}");
        assert!(
            world.get_entity(reserve_buffer_at_2).is_err(),
            "{cleaner:#?}"
        ); // buffer out of log
        assert!(world.get_entity(spawn_buffer_at_2).is_err(), "{cleaner:#?}"); // buffer out of log
        assert!(world.get_entity(spawn_at_3).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_at_4).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_buffer_at_4).is_ok(), "{cleaner:#?}");

        // undo 4
        cleaner.backward_log().unwrap();

        // do fresh 4
        cleaner.forward(&mut world, 1);
        assert!(world.get_entity(spawn_at_2).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_at_3).is_ok(), "{cleaner:#?}");
        assert!(world.get_entity(spawn_at_4).is_err(), "{cleaner:#?}"); // future spawn undone with forward
        assert!(world.get_entity(spawn_buffer_at_4).is_err(), "{cleaner:#?}"); // future spawn buffer undone with forward
    }
}
