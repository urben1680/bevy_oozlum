use bevy_ecs::{
    change_detection::MaybeLocation,
    entity::{Entity, EntityNotSpawnedError},
    resource::Resource,
    system::{Commands, EntityCommands},
    world::{DeferredWorld, EntityWorldMut, World},
};
use bevy_platform::cell::SyncCell;
use bevy_utils::prelude::DebugName;
use core::{
    any::type_name_of_val,
    error::Error,
    fmt::{Debug, Display},
};

use crate::{
    log::{OutOfLog, TransitionsLog, UpdateLog},
    meta::{MetaPastLen, RevDirection, RevMeta},
};

//mod commands;
//mod entity_commands;
mod entity_world;
mod insert_remove;
mod relationship;
mod spawn_despawn;
mod world;

//pub use commands::*;
//pub use entity_commands::*;
pub use entity_world::*;
pub use insert_remove::*;
use relationship::*;
pub use spawn_despawn::*;
pub use world::*;

const LOCATION_PREFIX: &str = if size_of::<MaybeLocation>() == 0 {
    ""
} else {
    " from "
};

const fn undo_redo_str<const UNDO: bool>() -> &'static str {
    if UNDO { "undo" } else { "redo" }
}

#[derive(Copy, Clone, Debug)]
pub struct EntityRevDespawnedError {
    pub entity: Entity,
    pub caller: MaybeLocation,
}

impl Display for EntityRevDespawnedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "entity {} is marked as reversibly despawned{LOCATION_PREFIX}{}",
            self.entity, self.caller
        )
    }
}

impl Error for EntityRevDespawnedError {}

#[derive(Debug, Copy, Clone)]
pub enum RevEntityError {
    EntityNotSpawnedError(EntityNotSpawnedError),
    EntityRevDespawnedError(EntityRevDespawnedError),
}

impl From<EntityNotSpawnedError> for RevEntityError {
    fn from(value: EntityNotSpawnedError) -> Self {
        Self::EntityNotSpawnedError(value)
    }
}

impl From<EntityRevDespawnedError> for RevEntityError {
    fn from(value: EntityRevDespawnedError) -> Self {
        Self::EntityRevDespawnedError(value)
    }
}

impl Display for RevEntityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntityNotSpawnedError(inner) => write!(f, "{inner}"),
            Self::EntityRevDespawnedError(inner) => write!(f, "{inner}"),
        }
    }
}

impl Error for RevEntityError {}

// todo: split trait for _deferred variant? try to trigger bugs by using trait as-is in wrong contexts
pub trait BuffersUndoRedo {
    /// Buffers an [`UndoRedo`] implementor in a resource to be collected by the reversible system's state during sync points.
    ///
    /// Logic applied in sync points are in:
    /// - commands
    /// - hooks
    /// - observers
    /// - bundle effects
    /// - [`SystemParam::apply`](bevy_ecs::system::SystemParam::apply)
    /// - [`SystemBuffer::apply`](bevy_ecs::system::SystemBuffer::apply)
    /// - [`System::apply_deferred`](bevy_ecs::system::System::apply_deferred)
    ///
    /// Note that the sync point **must** belong to a reversible system.
    /// todo: lay out situations where this is not true (trigger in non-reversible systems, queue commands in hooks/observers)
    /// The effect should be immediate in the sync point. Because of this, refer the following table for how to call this method:
    ///
    /// | | Sync Point | Non-Observer System |
    /// | - | - | - |
    /// | [`&mut World`](World) | ✅ | ❌ |
    /// | [`EntityWorldMut`] | ✅ | ❌ |
    /// | [`DeferredWorld`] | ✅ | ❌ |
    /// | [`UndoRedoBuffer`] | ✅ | ❌ |
    /// | [`Commands`] | ❌ | ✅ |
    /// | [`EntityCommands`] | ❌ | ✅ |
    #[track_caller]
    fn buffer_undo_redo(&mut self, meta_past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.buffer_undo_redo_with_caller(meta_past_len, undo_redo, MaybeLocation::caller());
    }

    /// As [`BuffersUndoRedo::buffer_undo_redo`] but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`RevMeta::run_rev_update`] may return the relevant error in that case.
    fn buffer_undo_redo_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    );
}

impl BuffersUndoRedo for Commands<'_, '_> {
    fn buffer_undo_redo_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        self.queue(move |world: &mut World| {
            world.buffer_undo_redo_with_caller(meta_past_len, undo_redo, caller);
        });
    }
}

impl BuffersUndoRedo for EntityCommands<'_> {
    fn buffer_undo_redo_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_undo_redo_with_caller(meta_past_len, undo_redo, caller);
        });
    }
}

impl BuffersUndoRedo for World {
    fn buffer_undo_redo_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        DeferredWorld::buffer_undo_redo_with_caller(
            &mut self.into(),
            meta_past_len,
            undo_redo,
            caller,
        );
    }
}

impl BuffersUndoRedo for EntityWorldMut<'_> {
    fn buffer_undo_redo_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        // SAFETY: Only resources are accessed, entity location remains unchanged
        let world = unsafe { self.world_mut() };
        world.buffer_undo_redo_with_caller(meta_past_len, undo_redo, caller);
    }
}

impl BuffersUndoRedo for DeferredWorld<'_> {
    fn buffer_undo_redo_with_caller(
        &mut self,
        meta_past_len: MetaPastLen,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) {
        debug_assert!(self.get_resource::<RevMeta>().is_some_and(|meta| {
            meta.get_running_direction()
                .is_some_and(|direction| match direction {
                    RevDirection::Forward {
                        meta_past_len: actual,
                    } => actual == meta_past_len,
                    _ => false,
                })
        }));
        self.resource_mut::<UndoRedoBuffer>()
            .buffer_undo_redo(meta_past_len, caller, undo_redo);
    }
}

/// For usages in _observer_ systems. Regular reversible systems should use commands or &mut World.
///
/// Commands and hooks can buffer [`UndoRedo`] implementors via [`&mut World`](World)/[`DeferredWorld`] instead.
///
/// Do not remove or overwrite this resource.
#[derive(Resource, Default, Debug)] // todo: wrap in private resource
pub(crate) struct UndoRedoBuffer(Vec<BoxedUndoRedo>);

impl UndoRedoBuffer {
    /// Returns `true` when the buffer is empty, otherwise returns `false`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    #[track_caller]
    pub(crate) fn buffer_undo_redo<T: UndoRedo>(
        &mut self,
        _: MetaPastLen,
        caller: MaybeLocation,
        undo_redo: T,
    ) {
        let name = type_name_of_val(&undo_redo);
        let boxed = BoxedUndoRedo {
            undo_redo: SyncCell::new(Box::new(undo_redo)),
            name: DebugName::borrowed(name),
            caller,
        };
        self.0.push(boxed);
    }
}

#[cfg(test)]
impl UndoRedo for UndoRedoBuffer {
    fn undo(&mut self, world: &mut World) {
        for boxed in self.0.iter_mut().rev() {
            boxed.undo_redo.get().undo(world)
        }
    }
    fn redo(&mut self, world: &mut World) {
        for boxed in self.0.iter_mut() {
            boxed.undo_redo.get().redo(world)
        }
    }
}

struct BoxedUndoRedo {
    undo_redo: SyncCell<Box<dyn UndoRedo>>,
    name: DebugName,
    caller: MaybeLocation,
}

impl Debug for BoxedUndoRedo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.caller.into_option() {
            Some(location) => write!(f, "{} from {location}", self.name),
            None => write!(f, "{}", self.name),
        }
    }
}

pub trait UndoRedo: Send + 'static {
    fn undo(&mut self, world: &mut World);
    fn redo(&mut self, world: &mut World);
}

/// Second argument for closures implementing [`UndoRedo`].
///
/// ```
/// # use bevy_ecs::world::World;
/// # use bevy_oozlum::prelude::*;
/// # let RevDirection::Forward { meta_past_len } = RevDirection::BackwardLog else {
/// #     return;
/// # };
/// # let mut world = World::new();
/// # let mut commands = world.commands();
/// commands.buffer_undo_redo(meta_past_len, |world: &mut World, direction| {
///     match direction {
///         UndoRedoDirection::Undo => {
///             // undo logic
///         },
///         UndoRedoDirection::Redo => {
///             // redo logic
///         }
///     }
/// })
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoRedoDirection {
    /// Apply undo logic when this is matched.
    Undo,

    /// Apply redo logic when this is matched
    Redo,
}

impl<F: FnMut(&mut World, UndoRedoDirection) + Send + 'static> UndoRedo for F {
    fn undo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Undo)
    }
    fn redo(&mut self, world: &mut World) {
        self(world, UndoRedoDirection::Redo)
    }
}

impl<T: UndoRedo> UndoRedo for Vec<T> {
    fn undo(&mut self, world: &mut World) {
        for x in self.iter_mut().rev() {
            x.undo(world);
        }
    }
    fn redo(&mut self, world: &mut World) {
        for x in self.iter_mut() {
            x.undo(world);
        }
    }
}

impl<T: UndoRedo, const N: usize> UndoRedo for [T; N] {
    fn undo(&mut self, world: &mut World) {
        for x in self.iter_mut().rev() {
            x.undo(world);
        }
    }
    fn redo(&mut self, world: &mut World) {
        for x in self.iter_mut() {
            x.undo(world);
        }
    }
}

impl<T: UndoRedo> UndoRedo for Box<[T]> {
    fn undo(&mut self, world: &mut World) {
        for x in self.iter_mut().rev() {
            x.undo(world);
        }
    }
    fn redo(&mut self, world: &mut World) {
        for x in self.iter_mut() {
            x.undo(world);
        }
    }
}

#[derive(Default, Debug)]
pub(crate) struct UndoRedoLog {
    update_log: UpdateLog,
    undo_redo_log: TransitionsLog<DebugHidden>,
}

struct DebugHidden(SyncCell<Box<dyn UndoRedo>>);

impl Debug for DebugHidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "_")
    }
}

#[derive(Debug)]
pub(crate) enum UndoRedoLogError {
    RevMetaMissing,
    UndoRedoBufferMissing {
        now: u64,
    },
    RevDirectionMismatch {
        now: u64,
        expected_forward: bool,
        direction: Option<RevDirection>,
    },
    OutOfLog {
        now: u64,
        direction: RevDirection,
        err: OutOfLog,
    },
}

impl Display for UndoRedoLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaMissing => write!(f, "RevMeta is missing"),
            Self::UndoRedoBufferMissing { now } => write!(
                f,
                "UndoRedoBuffer was removed at frame {now} but is needed to update the UndoRedo log"
            ),
            Self::RevDirectionMismatch {
                now,
                expected_forward,
                direction,
            } => {
                let actual = match direction {
                    Some(direction) => format!("{direction}"),
                    None => "none".to_string(),
                };
                let expected = if *expected_forward {
                    "forward"
                } else {
                    "backward"
                };
                write!(
                    f,
                    "RevDirection is {actual} when it was expected to be {expected} at frame {now} before the update of the UndoRedo log"
                )
            }
            Self::OutOfLog {
                now,
                direction,
                err,
            } => match err.0.into_option() {
                None => write!(
                    f,
                    "UndoRedo log is in an invalid state at frame {now} during {direction}"
                ),
                Some(location) => write!(
                    f,
                    "UndoRedo log is in an invalid state at frame {now} during {direction} at {location}"
                ),
            },
        }
    }
}

impl Error for UndoRedoLogError {}

impl UndoRedoLog {
    pub(crate) fn forward(&mut self, world: &mut World) -> Result<(), UndoRedoLogError> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogError::RevMetaMissing)?;

        let now = meta.now();
        match meta.get_running_direction() {
            Some(RevDirection::Forward { .. }) => world
                .try_resource_scope::<UndoRedoBuffer, _>(|world, mut buffer| {
                    if !buffer.0.is_empty() {
                        let meta = world.resource::<RevMeta>();
                        let meta_past_len = self.update_log.forward_past_len(meta);
                        let buffers = buffer.0.drain(..).map(|boxed| DebugHidden(boxed.undo_redo));
                        self.undo_redo_log
                            .forward_extend(meta, meta_past_len, buffers);
                    }
                })
                .ok_or(UndoRedoLogError::UndoRedoBufferMissing { now }),
            Some(RevDirection::ForwardLog) => {
                if self.update_log.forward_log(meta) {
                    let iter = self
                        .undo_redo_log
                        .forward_log(meta)
                        .map_err(|err| UndoRedoLogError::OutOfLog {
                            now,
                            direction: RevDirection::ForwardLog,
                            err,
                        })?
                        .map(|cell| cell.0.get());
                    for command in iter {
                        command.redo(world);
                    }
                }
                Ok(())
            }
            direction => Err(UndoRedoLogError::RevDirectionMismatch {
                now,
                expected_forward: true,
                direction,
            }),
        }
    }
    pub(crate) fn backward(&mut self, world: &mut World) -> Result<(), UndoRedoLogError> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogError::RevMetaMissing)?;

        let now = meta.now();
        let direction = meta.get_running_direction();
        if direction != Some(RevDirection::BackwardLog) {
            return Err(UndoRedoLogError::RevDirectionMismatch {
                now,
                expected_forward: false,
                direction,
            });
        }

        if self.update_log.backward_log(meta) {
            let iter = self
                .undo_redo_log
                .backward_log(meta)
                .map_err(|err| UndoRedoLogError::OutOfLog {
                    now,
                    direction: RevDirection::BackwardLog,
                    err,
                })?
                .map(|cell| cell.0.get())
                .rev();
            for command in iter {
                command.undo(world);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use bevy_ecs::component::Component;

    use super::*;

    #[derive(Component)]
    #[relationship(relationship_target = UnlinkedChildren)]
    pub(super) struct UnlinkedChildOf(pub Entity);

    #[derive(Component)]
    #[relationship_target(relationship = UnlinkedChildOf)]
    pub(super) struct UnlinkedChildren(Vec<Entity>);

    impl Deref for UnlinkedChildren {
        type Target = [Entity];
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    pub(super) fn assert_undo_redo<T>(
        world: &mut World,
        forward: impl FnOnce(&mut World, MetaPastLen) -> T,
        backward_log: impl FnOnce(&mut World, &mut T),
        forward_log: impl FnOnce(&mut World, &mut T),
    ) {
        assert_undo_redo_finalize(world, forward, backward_log, Some(forward_log), |_, _| {})
    }

    pub(super) fn assert_undo_redo_finalize<T>(
        world: &mut World,
        forward: impl FnOnce(&mut World, MetaPastLen) -> T,
        backward_log: impl FnOnce(&mut World, &mut T),
        forward_log: Option<impl FnOnce(&mut World, &mut T)>,
        finalize: impl FnOnce(&mut World, &mut T),
    ) {
        use crate::meta::RevQueue;
        use core::num::NonZeroU64;

        crate::panic_on_error_events();
        world.init_resource::<UndoRedoBuffer>();
        world.register_disabling_component::<RevDespawned>();
        let mut meta = RevMeta::new(NonZeroU64::MIN, false);
        let mut state = None;

        // forward
        meta = meta
            .update(|mut meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_MIN);
                meta.set_queue(RevQueue::RunBackwardLog);
                world.insert_resource(meta);
                state = Some(forward(world, direction.meta_past_len()));
                update_spawn_despawn(world).unwrap();
                world.remove_resource()
            })
            .unwrap();

        // backward log
        meta = meta
            .update(|mut meta, direction| {
                assert_eq!(direction, RevDirection::BackwardLog);
                let queue = if forward_log.is_some() {
                    RevQueue::RunForwardLog
                } else {
                    RevQueue::RunForward
                };
                meta.set_queue(queue);
                world.insert_resource(meta);
                world.resource_scope::<UndoRedoBuffer, _>(|world, mut buffer| buffer.undo(world));
                update_spawn_despawn(world).unwrap();
                backward_log(world, state.as_mut().unwrap());
                world.remove_resource()
            })
            .unwrap();

        if let Some(forward_log) = forward_log {
            // forward log
            meta = meta
                .update(|mut meta, direction| {
                    assert_eq!(direction, RevDirection::ForwardLog);
                    meta.set_queue(RevQueue::RunForward);
                    world.insert_resource(meta);
                    world.resource_scope::<UndoRedoBuffer, _>(|world, mut buffer| {
                        buffer.redo(world)
                    });
                    update_spawn_despawn(world).unwrap();
                    forward_log(world, state.as_mut().unwrap());
                    world.remove_resource()
                })
                .unwrap();
        }

        // finalize
        meta.update(|meta, _| {
            world.insert_resource(meta);
            update_spawn_despawn(world).unwrap();
            finalize(world, state.as_mut().unwrap());
            world.remove_resource()
        })
        .unwrap();
    }
}
