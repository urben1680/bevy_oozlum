use core::{any::TypeId, hash::BuildHasher};

use bevy_ecs::{
    change_detection::{CheckChangeTicks, MaybeLocation, Tick},
    component::ComponentId,
    error::BevyError,
    query::FilteredAccessSet,
    schedule::{BoxedCondition, InternedSystemSet, SystemCondition},
    system::{
        IntoSystem, ReadOnlySystem, RunSystemError, System, SystemIn, SystemParamValidationError,
        SystemStateFlags,
    },
    world::{DeferredWorld, World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_platform::{
    collections::{
        HashMap,
        hash_map::{Entry, RawEntryMut},
    },
    hash::{FixedState, PassHash},
};
use bevy_utils::prelude::DebugName;

use crate::{
    log::{TransitionLog, UpdateLog},
    meta::{RevDirection, RevMeta},
};

/// Wrap a `condition` into a reversivle `BoxedCondition` that logs the system output at
/// [`RevDirection::NotLog`] and traverses the log during [log directions](RevDirection::is_log),
/// bypassing the inner system.
pub(super) fn into_rev_condition<Marker>(
    condition: impl SystemCondition<Marker>,
) -> BoxedCondition {
    let condition = RevCondition {
        condition: IntoSystem::into_system(condition),
        meta_id: Default::default(),
        logs: Default::default(),
    };
    Box::new(condition)
}

/// Condition wrapper to turn it into a reversible condition.
struct RevCondition<T> {
    /// Wrapped condition that was passed to
    /// [`IntoRevScheduleConfigs::rev_run_if`](super::IntoRevScheduleConfigs::rev_run_if).
    condition: T,

    /// [`ComponentId`] of [`RevMeta`].
    meta_id: Option<ComponentId>,

    logs: ConditionLogs,
}

impl<T: ReadOnlySystem<In = (), Out = bool>> System for RevCondition<T> {
    type In = ();
    type Out = bool;
    fn name(&self) -> DebugName {
        self.condition.name()
    }
    fn type_id(&self) -> TypeId {
        self.condition.type_id()
    }
    fn flags(&self) -> SystemStateFlags {
        self.condition.flags()
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
        let mut access = self.condition.initialize(world);
        let meta_id = world.register_resource::<RevMeta>();
        self.meta_id = Some(meta_id);
        access.add_unfiltered_resource_read(meta_id);
        access
    }
    unsafe fn validate_param_unsafe(
        &mut self,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // RevMeta validation happens in run_unsafe as its failing verification is never a skipping
        // error but an error that must be handled
        unsafe {
            // SAFETY: in `initialize` T returned the required access to make this safe
            self.condition.validate_param_unsafe(world)
        }
    }
    unsafe fn run_unsafe(
        &mut self,
        (): SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Result<bool, RunSystemError> {
        let ptr = unsafe {
            // SAFETY:
            // - Registered read access to resource in `initialize`
            // - T cannot have write access on the resource because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        };
        let meta = ptr
            .map(|ptr| unsafe {
                // SAFETY: `Self::meta_id` is the id of `RevMeta`, so Ptr is its erased pointee type
                ptr.deref::<RevMeta>()
            })
            .ok_or_else(|| RunSystemError::Failed("`RevMeta` is missing but required".into()))?;
        let direction = meta.get_running_direction().ok_or_else(|| {
            RunSystemError::Failed(
                "`RevMeta::get_running_direction` did not return the required `RevDirection".into(),
            )
        })?;

        match direction {
            RevDirection::NotLog(_) => {
                let result = unsafe {
                    // SAFETY:
                    // - in `initialize` T returned the required access to make this safe
                    // - T is readonly so the meta reference is allowed to exist while T runs
                    self.condition.run_unsafe((), world)
                };

                self.logs.insert(meta, &result)?;

                result
            }
            RevDirection::ForwardLog => self.logs.get(meta, true),
            RevDirection::BackwardLog => self.logs.get(meta, false),
        }
    }
    fn apply_deferred(&mut self, _world: &mut World) {
        unreachable!("reversible conditions do not get their deferred parameters applied")
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("reversible conditions are not used as observers")
    }
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.condition.default_system_sets()
    }
    fn check_change_tick(&mut self, change_tick: CheckChangeTicks) {
        self.condition.check_change_tick(change_tick);
    }
    fn get_last_run(&self) -> Tick {
        self.condition.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.condition.set_last_run(last_run);
    }
}

// SAFETY:
// - T is `ReadOnlySystem`
// - RevCondition additionally only reads `RevMeta` resource
unsafe impl<T: ReadOnlySystem<In = (), Out = bool>> ReadOnlySystem for RevCondition<T> {}

#[derive(Default)]
struct ConditionLogs {
    /// A `bool`-like log for frames in which [`Self::condition`] returned `Ok(true)`.
    ok_true_log: UpdateLog,

    failed_log: Option<Box<FailedLogs>>,
}

impl ConditionLogs {
    fn insert(
        &mut self,
        meta: &RevMeta,
        result: &Result<bool, RunSystemError>,
    ) -> Result<(), RunSystemError> {
        match result {
            Ok(false) | Err(RunSystemError::Skipped(_)) => Ok(()),
            Ok(true) => {
                self.ok_true_log
                    .forward_past_len_with_caller(meta, MaybeLocation::new(None));
                Ok(())
            }
            Err(RunSystemError::Failed(failed)) => {
                self.failed_log.get_or_insert_default().insert(meta, failed)
            }
        }
    }
    fn get(&mut self, meta: &RevMeta, forward: bool) -> Result<bool, RunSystemError> {
        match self.failed_log.as_mut() {
            None if forward => Ok(self
                .ok_true_log
                .forward_log_with_caller(meta, MaybeLocation::new(None))),
            None => Ok(self
                .ok_true_log
                .backward_log_with_caller(meta, MaybeLocation::new(None))),
            Some(failed_log) => failed_log.get(&mut self.ok_true_log, meta, forward),
        }
    }
}

#[derive(Default)]
struct FailedLogs {
    err_failed_log: UpdateLog,

    failed_log: TransitionLog<u64>,

    failed_cache: FailedCache,
}

impl FailedLogs {
    fn insert(&mut self, meta: &RevMeta, failed: &BevyError) -> Result<(), RunSystemError> {
        // insert log value if vacant, increase usages if occupied
        let key = self.failed_cache.insert_get_key(failed);

        // log hash as transition, reduce usages for drained, remove if unused
        let past_len = self.err_failed_log.forward_past_len(meta);
        let mut drain = self.failed_log.forward_push(meta, past_len, key);
        let keys = drain.all();

        for hash in keys {
            if let Entry::Occupied(mut occupied) = self.failed_cache.0.entry(hash) {
                let usages = &mut occupied.get_mut().usages;
                match usages.checked_sub(1) {
                    Some(reduced) => *usages = reduced,
                    None => {
                        occupied.remove();
                    }
                }
            };
        }

        Ok(())
    }
    fn get(
        &mut self,
        ok_true_log: &mut UpdateLog,
        meta: &RevMeta,
        forward: bool,
    ) -> Result<bool, RunSystemError> {
        let failed_log_result = if forward {
            if ok_true_log.forward_log(meta) {
                return Ok(true);
            }
            if !self.err_failed_log.forward_log(meta) {
                return Ok(false);
            }
            self.failed_log.forward_log(meta)
        } else {
            if ok_true_log.backward_log(meta) {
                return Ok(true);
            }
            if !self.err_failed_log.backward_log(meta) {
                return Ok(false);
            }
            self.failed_log.backward_log(meta)
        };

        let err = match failed_log_result {
            Ok(key) => self.failed_cache.get(*key).into(),
            Err(out_of_log) => out_of_log.into(),
        };

        Err(RunSystemError::Failed(err))
    }
}

#[derive(Default)]
struct FailedCache(HashMap<u64, Failed, PassHash>);

impl FailedCache {
    fn get(&self, key: u64) -> &str {
        self.0
            .get(&key)
            .map(|Failed { string, .. }| string.as_str())
            .unwrap_or("reversible condition could not load logged error")
    }
    fn insert_get_key(&mut self, failed: &BevyError) -> u64 {
        let string = failed.to_string();
        let hash_state = FixedState::default();
        let hash = hash_state.hash_one(&string);
        let entry = self.0.raw_entry_mut().from_key_hashed_nocheck(hash, &hash);
        match entry {
            RawEntryMut::Vacant(entry) => {
                entry.insert_hashed_nocheck(hash, hash, Failed { string, usages: 0 });
            }
            RawEntryMut::Occupied(entry) => {
                entry.into_mut().usages += 1;
            }
        };

        hash
    }
}

struct Failed {
    string: String,
    usages: usize,
}

#[cfg(test)]
mod test {
    use crate::meta::RevQueue;

    use super::*;

    struct MetaAndLogs {
        meta: RevMeta,
        logs: ConditionLogs,
    }

    impl MetaAndLogs {
        fn new() -> Self {
            Self {
                meta: RevMeta::new(u64::MAX, false),
                logs: Default::default(),
            }
        }
        fn forward(&mut self, result: Result<bool, RunSystemError>) {
            self.meta.set_queue(RevQueue::RunForward);
            self.meta.update_ref(Ok(true), |meta, _| {
                self.logs.insert(meta, &result).unwrap();
            });
        }
        fn noop_forward(&mut self) {
            self.meta.set_queue(RevQueue::RunForward);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        fn forward_log(&mut self, expected: Result<bool, &str>) {
            self.meta.set_queue(RevQueue::RunForwardLog);
            self.log(true, expected);
        }
        fn backward_log(&mut self, expected: Result<bool, &str>) {
            self.meta.set_queue(RevQueue::RunBackwardLog);
            self.log(false, expected);
        }
        fn log(&mut self, forward: bool, expected: Result<bool, &str>) {
            self.meta.update_ref(Ok(true), |meta, _| {
                match (self.logs.get(meta, forward), expected) {
                    (Ok(actual), Ok(expected)) => assert_eq!(actual, expected),
                    (Err(RunSystemError::Failed(actual)), Err(expected)) => {
                        let mut actual = actual.to_string();
                        actual.truncate(expected.len());
                        assert_eq!(actual, expected);
                    }
                    (actual, expected) => panic!("{actual:?} not equal to {expected:?}"),
                }
            });
        }
        fn forward_clear(mut self, err: &'static str) {
            assert!(
                self.logs
                    .failed_log
                    .as_ref()
                    .is_some_and(|failed_logs| failed_logs.failed_cache.0.len() > 1)
            );
            self.meta.set_queue(RevQueue::ClearThenRunForward);
            self.meta.update_ref(Ok(true), |meta, _| {
                self.logs
                    .insert(meta, &Err(RunSystemError::Failed(err.into())))
                    .unwrap();
            });
            let failed_logs = self.logs.failed_log.unwrap();
            let mut values = failed_logs.failed_cache.0.values();
            let mut actual = values.next().unwrap().string.clone();
            actual.truncate(err.len());
            assert_eq!(actual, err);
            assert!(values.next().is_none());
        }
    }

    #[test]
    fn traverses_log() {
        let skipped_skipped =
            RunSystemError::Skipped(SystemParamValidationError::skipped::<()>("unreachable"));
        let skipped_failed =
            RunSystemError::Skipped(SystemParamValidationError::invalid::<()>("unreachable"));
        let failed = |msg: &str| RunSystemError::Failed(msg.into());

        let mut meta_and_log = MetaAndLogs::new();

        meta_and_log.forward(Err(failed("first error")));
        meta_and_log.forward(Ok(true));
        meta_and_log.forward(Err(skipped_skipped));
        meta_and_log.forward(Err(skipped_failed));
        meta_and_log.forward(Ok(false));
        meta_and_log.noop_forward();
        meta_and_log.forward(Err(failed("second error")));

        meta_and_log.backward_log(Err("second error"));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(true));
        meta_and_log.backward_log(Err("first error"));

        meta_and_log.forward_log(Err("first error"));
        meta_and_log.forward_log(Ok(true));
        meta_and_log.forward_log(Ok(false));
        meta_and_log.forward_log(Ok(false));
        meta_and_log.forward_log(Ok(false));
        meta_and_log.forward_log(Ok(false));
        meta_and_log.forward_log(Err("second error"));

        meta_and_log.backward_log(Err("second error"));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(false));

        meta_and_log.forward(Err(failed("third error")));

        meta_and_log.backward_log(Err("third error"));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(false));
        meta_and_log.backward_log(Ok(true));
        meta_and_log.backward_log(Err("first error"));

        meta_and_log.forward_log(Err("first error"));
        meta_and_log.forward_log(Ok(true));
        meta_and_log.forward_log(Ok(false));
        meta_and_log.forward_log(Ok(false));
        meta_and_log.forward_log(Err("third error"));

        meta_and_log.forward_clear("fourth error");
    }
}
