use std::panic::Location;

use crate::log::OutOfLog;

use super::*;

/*
wie umsetzen?

resource Problem: unterstützt keine subframe Änderungen

muss doch via UndoRedo umgesetzt werden!

wie delayed spawns in UndoRedo umsetzen?

Kombination: Resource wird nur genutzt um finales Despawn umzusetzen
todo: rework forward_log + backward_log
*/

#[derive(Resource)]
pub struct SpawnDespawnRes {
    spawn: DenseTransitionsLog<(Entity, MaybeLocation)>,
    despawn: DenseTransitionsLog<(Entity, MaybeLocation)>,
    spawn_buffer: DenseTransitionsLog<Option<Entity>>,
    spawn_queue: Vec<(Entity, MaybeLocation)>,
    despawn_queue: Vec<(Entity, MaybeLocation)>,
    spawn_buffer_queue: Vec<Option<Entity>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpawnDespawnBackwardErr {
    OutOfLog,
    EmptyBufferSpawn,
    UnreservedBufferSpawn,
    UnusedBufferSpawnReservation,
}

impl From<OutOfLog> for SpawnDespawnBackwardErr {
    fn from(_: OutOfLog) -> Self {
        Self::OutOfLog
    }
}

impl Display for SpawnDespawnBackwardErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfLog => write!(f, "{}", OutOfLog),
            Self::EmptyBufferSpawn => write!(
                f,
                "an empty internal buffer entity was reserved during the backward direction instead of the non-log direction",
            ),
            Self::UnreservedBufferSpawn => write!(
                f,
                "an internal buffer entity was attempted to be logged during the backward direction but no slot was reserved"
            ),
            Self::UnusedBufferSpawnReservation => write!(
                f,
                "an internal buffer entity reservation remained unclaimed during the backward direction",
            ),
        }
    }
}

impl Error for SpawnDespawnBackwardErr {}

impl SpawnDespawnRes {
    /// Must be called during [`RevDirection::NOT_LOG`].
    pub(crate) fn spawn(&mut self, entity: Entity, location: MaybeLocation) {
        self.spawn_queue.push((entity, location));
    }

    /// Must be called during [`RevDirection::NOT_LOG`].
    pub(crate) fn despawn(&mut self, entity: Entity, location: MaybeLocation) {
        self.despawn_queue.push((entity, location));
    }

    /// Must be called during [`RevDirection::NOT_LOG`] or [`RevDirection::BackwardLog`].
    ///
    /// At the latter, `buffer` must be `Some` and the current frame's [`RevDirection::NOT_LOG`]
    /// run must have called this with `None` to reserve the entity that is pushed here.
    pub(crate) fn spawn_buffer(&mut self, buffer: Option<Entity>) {
        self.spawn_buffer_queue.push(buffer);
    }

    /// Must be called at the end of [`RevDirection::NOT_LOG`].
    pub(crate) fn forward(&mut self, world: &mut World, max_past_len: usize) {
        world.insert_resource(BufferInProgressRes(BufferInProgress::FinalDespawn {
            buffer: false,
        }));

        let (despawned, _) = self.spawn.drain_future();
        for (entity, _) in despawned {
            let _ = world.try_despawn(entity);
        }
        self.spawn.push_and_pop_past(max_past_len, |mut log| {
            log.extend(self.spawn_queue.drain(..));
        });

        let despawned = self
            .despawn
            .push_and_pop_past(max_past_len, |mut log| {
                log.extend(self.despawn_queue.drain(..));
            })
            .into_iter()
            .flat_map(|value_entry| value_entry.value);
        for (entity, _) in despawned {
            let _ = world.try_despawn(entity);
        }

        world.insert_resource(BufferInProgressRes(BufferInProgress::FinalDespawn {
            buffer: true,
        }));

        let despawned = self.spawn_buffer.drain_future().0.flatten();
        for entity in despawned {
            let _ = world.try_despawn(entity);
        }
        self.spawn_buffer
            .push_and_pop_past(max_past_len, |mut log| {
                log.extend(self.spawn_buffer_queue.drain(..));
            });

        world.remove_resource::<BufferInProgressRes>();
    }

    /// Must be called at the end of [`RevDirection::FORWARD_LOG`].
    pub(crate) fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.spawn.forward_log()?;
        let _ok = self.despawn.forward_log();
        let _ok = self.spawn_buffer.forward_log();
        Ok(())
    }

    /// Must be called at the beginning of [`RevDirection::BackwardLog`].
    pub(crate) fn backward_log(&mut self) -> Result<(), SpawnDespawnBackwardErr> {
        self.spawn.backward_log()?;
        let _ok = self.despawn.backward_log();
        let mut buffer_iter = self
            .spawn_buffer
            .backward_log()
            .expect("should not be out-of-log like `Self::spawn` as the logs they get called identically in `forward`")
            .value;
        let delayed_iter = self.spawn_buffer_queue.drain(..);

        'delayed_loop: for delayed in delayed_iter {
            let Some(delayed) = delayed else {
                return Err(SpawnDespawnBackwardErr::EmptyBufferSpawn);
            };
            for buffer in buffer_iter.by_ref() {
                if buffer.is_none() {
                    *buffer = Some(delayed);
                    continue 'delayed_loop;
                }
            }
            return Err(SpawnDespawnBackwardErr::UnreservedBufferSpawn);
        }

        if !buffer_iter.all(|buffer| buffer.is_some()) {
            return Err(SpawnDespawnBackwardErr::UnusedBufferSpawnReservation);
        }

        Ok(())
    }
}

#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub(crate) struct RevDespawned(MaybeLocation);

impl RevDespawned {
    #[track_caller]
    pub(crate) fn new() -> Self {
        Self(MaybeLocation::caller())
    }
    pub(crate) fn with_caller(location: MaybeLocation) -> Self {
        Self(location)
    }
    pub(crate) fn caller(self) -> MaybeLocation {
        self.0
    }
}

pub trait RevIsDespawned {
    fn rev_is_despawned(&self) -> bool;
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>>;
}

impl RevIsDespawned for EntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

impl<B: Bundle> RevIsDespawned for EntityRefExcept<'_, '_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

impl RevIsDespawned for FilteredEntityRef<'_, '_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    // todo: may return None if not part of filter
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

impl RevIsDespawned for EntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

impl<B: Bundle> RevIsDespawned for EntityMutExcept<'_, '_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

impl RevIsDespawned for FilteredEntityMut<'_, '_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

impl RevIsDespawned for EntityWorldMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<RevDespawned>()
    }
    fn rev_despawned_by(&self) -> MaybeLocation<Option<&'static Location<'static>>> {
        MaybeLocation::new_with(|| {
            self.get::<RevDespawned>()
                .map(|despawned| despawned.0.into_option().unwrap())
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // todo
}
