use bevy_ecs::{
    bundle::Bundle,
    change_detection::MaybeLocation,
    component::Component,
    entity::Entity,
    resource::Resource,
    world::{
        EntityMut, EntityMutExcept, EntityRef, EntityRefExcept, EntityWorldMut, FilteredEntityMut,
        FilteredEntityRef, World,
    },
};
use bevy_log::{error, error_once};

use crate::{
    log::{OutOfLog, TransitionsLog},
    meta::{MetaPastLen, RevDirection, RevMeta},
    prelude::RevOp,
};

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

#[derive(Debug)]
pub(crate) enum DespawnCleanerErr {
    MetaMissing,
    MetaNotRunning,
    CleanerMissing,
    CleanerOutOfLog,
}

impl From<OutOfLog> for DespawnCleanerErr {
    fn from(_: OutOfLog) -> Self {
        Self::CleanerOutOfLog
    }
}

impl RevDespawnCleaner {
    pub(crate) fn log_spawn(&mut self, entity: Entity, location: MaybeLocation, _: MetaPastLen) {
        self.spawn_queue.push((entity, location));
    }

    pub(crate) fn log_spawn_batch(
        &mut self,
        entities: &[Entity],
        location: MaybeLocation,
        _: MetaPastLen,
    ) {
        self.spawn_queue
            .extend(entities.iter().copied().map(|entity| (entity, location)));
    }

    pub(crate) fn log_despawn(&mut self, entity: Entity, location: MaybeLocation, _: MetaPastLen) {
        self.despawn_queue.push((entity, location));
    }

    /// Must be called during [`RevDirection::Forward`] or [`RevDirection::BackwardLog`].
    ///
    /// At the latter, `buffer` must be `Some` and the current frame's [`RevDirection::Forward`]
    /// run must have called this with `None` to reserve the entity that is pushed here.
    pub(crate) fn log_spawn_buffer(&mut self, buffer: Option<Entity>, location: MaybeLocation) {
        self.spawn_buffer_queue.push((buffer, location));
    }

    pub(crate) fn log_spawn_buffer_batch_reserve(
        &mut self,
        buffers: usize,
        location: MaybeLocation,
        _: MetaPastLen,
    ) {
        self.spawn_buffer_queue
            .extend(core::iter::repeat_n((None, location), buffers));
    }

    pub(crate) fn log_spawn_buffer_batch(&mut self, buffers: &[Entity], location: MaybeLocation) {
        self.spawn_buffer_queue
            .extend(buffers.into_iter().map(|buffer| (Some(*buffer), location)));
    }

    /// Despawn entities that contain [`RevDespawned`] and their relevant operation (spawn, despawn, move_components) fell out of log.
    pub(crate) fn update(world: &mut World) -> Result<(), DespawnCleanerErr> {
        world
            .try_resource_scope::<Self, _>(|world, this| {
                let this = this.into_inner();
                let meta = world
                    .get_resource::<RevMeta>()
                    .ok_or(DespawnCleanerErr::MetaMissing)?;
                let direction = meta
                    .get_running_direction()
                    .ok_or(DespawnCleanerErr::MetaNotRunning)?;

                match direction {
                    RevDirection::Forward { meta_past_len } => {
                        let new_spawn = this.spawn_queue.drain(..).map(|(entity, _)| entity);
                        let mut drain = this.spawn.forward_extend(meta, meta_past_len, new_spawn);
                        let old_spawn = drain.future();

                        let new_despawn = this.despawn_queue.drain(..).map(|(entity, _)| entity);
                        let mut drain =
                            this.despawn
                                .forward_extend(meta, meta_past_len, new_despawn);
                        let old_despawn = drain.past();

                        let new_buffer = this
                            .spawn_buffer_queue_fallback
                            .drain(..)
                            .map(|(entity, location)| (Some(entity), location))
                            .chain(this.spawn_buffer_queue.drain(..));
                        let mut drain =
                            this.spawn_buffer
                                .forward_extend(meta, meta_past_len, new_buffer);
                        let old_buffer = drain.all().flat_map(|(entity, _)| entity);

                        RevOp::FinalDespawn { buffer: false }.scope(world, |world| {
                            for entity in old_spawn.chain(old_despawn) {
                                let _ = world.try_despawn(entity);
                            }
                        });

                        RevOp::FinalDespawn { buffer: true }.scope(world, |world| {
                            for entity in old_buffer {
                                let _ = world.try_despawn(entity);
                            }
                        });

                        Ok(())
                    }
                    RevDirection::ForwardLog => this.forward_log(meta),
                    RevDirection::BackwardLog => this.backward_log(meta),
                }
            })
            .unwrap_or(Err(DespawnCleanerErr::CleanerMissing))
    }

    fn forward_log(&mut self, meta: &RevMeta) -> Result<(), DespawnCleanerErr> {
        self.spawn.forward_log(meta)?;
        self.despawn.forward_log(meta).unwrap();
        self.spawn_buffer.forward_log(meta).unwrap();

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

    fn backward_log(&mut self, meta: &RevMeta) -> Result<(), DespawnCleanerErr> {
        self.spawn.backward_log(meta)?;
        self.despawn.backward_log(meta).unwrap();
        let mut buffer_iter = self.spawn_buffer.backward_log(meta).unwrap(); // should not be out-of-log like `Self::spawn` as they get called identically in `forward`
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

#[derive(Component, Debug, Clone, Copy)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
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
    use core::num::NonZeroU64;

    use crate::{meta::RevQueue, panic_on_error_events};

    use super::*;

    struct WorldWithResources(World);

    impl WorldWithResources {
        fn new(max_past_len: u64) -> Self {
            panic_on_error_events();
            let mut world = World::new();
            world.init_resource::<RevDespawnCleaner>();
            world.insert_resource(RevMeta::new(NonZeroU64::new(max_past_len).unwrap(), false));
            Self(world)
        }
        fn spawn_batch<const N: usize>(&mut self) -> [Entity; N] {
            self.0
                .spawn_batch(core::iter::repeat_n((), N))
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        }
        fn assert_entities<const N: usize>(&self, entities: [(Entity, bool); N]) {
            for (n, (entity, exists)) in entities.into_iter().enumerate() {
                assert_eq!(self.0.get_entity(entity).is_ok(), exists, "{n}");
            }
        }
        fn forward<const SPAWN: usize, const DESPAWN: usize, const BUFFER: usize>(
            &mut self,
            spawn: [Entity; SPAWN],
            despawn: [Entity; DESPAWN],
            buffer: [Entity; BUFFER],
            buffer_reserve: usize,
            clear: bool,
        ) {
            let caller = MaybeLocation::caller();
            let mut cleaner_result = None;
            let mut meta = self.0.remove_resource::<RevMeta>().unwrap();
            let queue = if clear {
                RevQueue::ClearThenRunForward
            } else {
                RevQueue::RunForward
            };
            meta.set_queue(queue);
            meta = meta
                .update(|meta, _| {
                    let past_len = meta.meta_past_len();
                    self.0.insert_resource(meta);
                    let mut cleaner = self.0.resource_mut::<RevDespawnCleaner>();
                    cleaner.log_spawn_batch(&spawn, caller, past_len);
                    for entity in despawn {
                        cleaner.log_despawn(entity, caller, past_len);
                    }
                    cleaner.log_spawn_buffer_batch(&buffer, caller);
                    cleaner.log_spawn_buffer_batch_reserve(buffer_reserve, caller, past_len);
                    cleaner_result = Some(RevDespawnCleaner::update(&mut self.0));
                    self.0.remove_resource::<RevMeta>()
                })
                .unwrap();
            if !matches!(cleaner_result, Some(Ok(()))) {
                panic!("{cleaner_result:#?}");
            }
            self.0.insert_resource(meta);
        }
        fn forward_log(&mut self) {
            let mut meta = self.0.remove_resource::<RevMeta>().unwrap();
            let mut ran = false;
            meta.set_queue(RevQueue::RunForwardLog);
            meta = meta
                .update(|meta, _| {
                    ran = true;
                    self.0.insert_resource(meta);
                    RevDespawnCleaner::update(&mut self.0).unwrap();
                    self.0.remove_resource::<RevMeta>()
                })
                .unwrap();
            assert!(ran);
            self.0.insert_resource(meta);
        }
        fn backward_log<const BUFFER: usize>(&mut self, buffer: [Entity; BUFFER]) {
            let caller = MaybeLocation::caller();
            let mut meta = self.0.remove_resource::<RevMeta>().unwrap();
            let mut ran = false;
            meta.set_queue(RevQueue::RunBackwardLog);
            meta = meta
                .update(|meta, _| {
                    ran = true;
                    self.0.insert_resource(meta);
                    self.0
                        .resource_mut::<RevDespawnCleaner>()
                        .log_spawn_buffer_batch(&buffer, caller);
                    RevDespawnCleaner::update(&mut self.0).unwrap();
                    self.0.remove_resource::<RevMeta>()
                })
                .unwrap();
            assert!(ran);
            self.0.insert_resource(meta);
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut world_with_resources = WorldWithResources::new(1);
        let [
            spawn_at_1,
            spawn_at_2,
            despawn_at_2,
            spawn_buffer_at_2,
            reserve_buffer_at_2,
            spawn_at_3,
            despawn_at_3,
            spawn_buffer_at_3,
            spawn_at_4,
            spawn_buffer_at_4,
            spawn_at_fresh_4,
            despawn_at_fresh_4,
            spawn_buffer_at_fresh_4,
        ] = world_with_resources.spawn_batch();

        // do 1
        world_with_resources.forward([spawn_at_1], [], [], 0, false);
        world_with_resources.assert_entities([(spawn_at_1, true)]);

        // do 2
        world_with_resources.forward([spawn_at_2], [despawn_at_2], [spawn_buffer_at_2], 1, false);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (despawn_at_2, true),
            (spawn_buffer_at_2, true),
        ]);

        // undo 2
        world_with_resources.backward_log([reserve_buffer_at_2]);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (despawn_at_2, true),
            (spawn_buffer_at_2, true),
            (reserve_buffer_at_2, true),
        ]);

        // redo 2
        world_with_resources.forward_log();
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (despawn_at_2, true),
            (spawn_buffer_at_2, true),
            (reserve_buffer_at_2, true),
        ]);

        // do 3
        world_with_resources.forward([spawn_at_3], [despawn_at_3], [spawn_buffer_at_3], 0, false);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (despawn_at_2, false),
            (spawn_buffer_at_2, false),
            (reserve_buffer_at_2, false),
            (spawn_at_3, true),
            (despawn_at_3, true),
            (spawn_buffer_at_3, true),
        ]);

        // do 4
        world_with_resources.forward([spawn_at_4], [], [spawn_buffer_at_4], 0, false);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (spawn_at_4, true),
            (spawn_buffer_at_4, true),
            (despawn_at_3, false),
            (spawn_buffer_at_3, false),
        ]);

        // undo 4
        world_with_resources.backward_log([]);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (spawn_at_4, true),
            (spawn_buffer_at_4, true),
        ]);

        // do fresh 4
        world_with_resources.forward(
            [spawn_at_fresh_4],
            [despawn_at_fresh_4],
            [spawn_buffer_at_fresh_4],
            0,
            false,
        );
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (spawn_at_4, false),        // undone spawn out of log
            (spawn_buffer_at_4, false), // buffer out of log
            (spawn_at_fresh_4, true),
            (despawn_at_fresh_4, true),
            (spawn_buffer_at_fresh_4, true),
        ]);

        // undo 4 again
        world_with_resources.backward_log([]);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (spawn_at_fresh_4, true),
            (despawn_at_fresh_4, true),
            (spawn_buffer_at_fresh_4, true),
        ]);

        // do yet another fresh 4 with clear
        world_with_resources.forward([], [], [], 0, true);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (spawn_at_fresh_4, false), // undone spawn out of log
            (despawn_at_fresh_4, true),
            (spawn_buffer_at_fresh_4, false), // buffer out of log
            (despawn_at_3, false),            // finalized despawn early from 3
            (spawn_buffer_at_3, false),       // buffer out of log
        ]);

        // do 5
        world_with_resources.forward([], [], [], 0, false);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (despawn_at_fresh_4, true),
        ]);

        // do 6
        world_with_resources.forward([], [], [], 0, false);
        world_with_resources.assert_entities([
            (spawn_at_1, true),
            (spawn_at_2, true),
            (spawn_at_3, true),
            (despawn_at_fresh_4, true), // never finalized
        ]);
    }
}
