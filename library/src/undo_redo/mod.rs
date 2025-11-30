use bevy_ecs::{
    bundle::{Bundle, BundleFromComponents},
    change_detection::{DetectChangesMut, MaybeLocation},
    entity::{Entity, EntityDoesNotExistError},
    resource::Resource,
    system::{Commands, EntityCommands},
    world::{DeferredWorld, EntityWorldMut, World},
};
use bevy_log::warn_once;
use bevy_platform::cell::SyncCell;
use bevy_utils::prelude::DebugName;
use core::{
    any::type_name_of_val,
    error::Error,
    fmt::{Debug, Display},
    hash::Hash,
};

use crate::{
    log::{OutOfLog, TransitionsLog, UpdateLog},
    meta::{MetaPastLen, RevDirection, RevMeta},
};

mod commands;
mod entity_commands;
mod entity_world;
mod spawn_despawn;
mod world;

pub use commands::*;
pub use entity_commands::*;
pub use entity_world::*;
pub use spawn_despawn::*;
pub use world::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EntityRevDespawnedError {
    pub entity: Entity,
    //pub location: MaybeLocation, todo: implement with https://github.com/bevyengine/bevy/issues/20494
}

impl Display for EntityRevDespawnedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "entity {} is marked as reversibly despawned",
            self.entity
        )
    }
}

impl Error for EntityRevDespawnedError {}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RevEntityError {
    EntityDoesNotExistError(EntityDoesNotExistError),
    EntityRevDespawnedError(EntityRevDespawnedError),
}

impl From<EntityDoesNotExistError> for RevEntityError {
    fn from(value: EntityDoesNotExistError) -> Self {
        Self::EntityDoesNotExistError(value)
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
            Self::EntityDoesNotExistError(inner) => write!(f, "{inner}"),
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
    fn buffer_undo_redo(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo);
}

impl BuffersUndoRedo for Commands<'_, '_> {
    #[track_caller]
    fn buffer_undo_redo(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.queue(move |world: &mut World| {
            world.buffer_undo_redo(past_len, undo_redo);
        });
    }
}

impl BuffersUndoRedo for EntityCommands<'_> {
    #[track_caller]
    fn buffer_undo_redo(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_undo_redo(past_len, undo_redo);
        });
    }
}

impl BuffersUndoRedo for World {
    #[track_caller]
    fn buffer_undo_redo(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        DeferredWorld::buffer_undo_redo(&mut self.into(), past_len, undo_redo);
    }
}

impl BuffersUndoRedo for EntityWorldMut<'_> {
    #[track_caller]
    fn buffer_undo_redo(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        // SAFETY: Only resources are accessed, entity location remains unchanged
        let world = unsafe { self.world_mut() };
        world.buffer_undo_redo(past_len, undo_redo);
    }
}

impl BuffersUndoRedo for DeferredWorld<'_> {
    #[track_caller]
    fn buffer_undo_redo(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        debug_assert!(self.get_resource::<RevMeta>().is_some_and(|meta| {
            meta.get_running_direction()
                .is_some_and(|direction| match direction {
                    RevDirection::Forward {
                        meta_past_len: actual,
                    } => actual == past_len,
                    _ => false,
                })
        }));
        self.resource_mut::<UndoRedoBuffer>()
            .buffer_undo_redo(past_len, undo_redo);
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
    pub(crate) fn buffer_undo_redo(&mut self, _: MetaPastLen, undo_redo: impl UndoRedo) {
        let name = type_name_of_val(&undo_redo);
        let boxed = BoxedUndoRedo {
            undo_redo: SyncCell::new(Box::new(undo_redo)),
            name: DebugName::borrowed(name),
            caller: MaybeLocation::caller(),
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

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoRedoDirection {
    Undo,
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
                        let past_len = self.update_log.forward_past_len(meta);
                        let buffers = buffer.0.drain(..).map(|boxed| DebugHidden(boxed.undo_redo));
                        self.undo_redo_log.forward_extend(meta, past_len, buffers);
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Resource)]
pub enum RevOp {
    // todo: assert in tests
    RevSchedule {
        direction: RevDirection,
    },
    Buffer {
        direction: RevDirection,
        buffer: Entity,
    },
    FinalDespawn {
        buffer: bool,
    },
}

impl RevOp {
    pub(crate) fn scope(mut self, world: &mut World, c: impl FnOnce(&mut World)) {
        match world.get_resource_mut::<Self>() {
            None => {
                world.insert_resource(self);
                c(world);
                world.remove_resource::<Self>();
            }
            Some(mut other) => {
                core::mem::swap(&mut self, other.bypass_change_detection());
                if matches!(*other, Self::FinalDespawn { .. }) {
                    warn_once!(
                        "a reversible buffering happens at the time of finalizing despawns, this may be unintentional"
                    );
                }
                c(world);
                world.insert_resource(self);
            }
        }
    }
}

struct Take<B> {
    bundle: Option<B>,
    entity: Entity,
}

impl<B: Bundle + BundleFromComponents> UndoRedo for Take<B> {
    fn undo(&mut self, world: &mut World) {
        world
            .entity_mut(self.entity)
            .insert(self.bundle.take().expect("todo"));
    }
    fn redo(&mut self, world: &mut World) {
        self.bundle = world.entity_mut(self.entity).take::<B>();
    }
}

struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> UndoRedo for ResourceSwap<R> {
    fn undo(&mut self, world: &mut World) {
        match world.get_resource_mut::<R>() {
            Some(mut r1) => match self.0.as_mut() {
                Some(r2) => core::mem::swap(&mut *r1, r2),
                None => self.0 = world.remove_resource::<R>(),
            },
            None => {
                if let Some(r2) = self.0.take() {
                    world.insert_resource(r2)
                }
            }
        }
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world)
    }
}

struct SpawnEmpty {
    entity: Entity,
    caller: MaybeLocation,
}

impl UndoRedo for SpawnEmpty {
    fn undo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).insert(RevDespawned);
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).remove::<RevDespawned>();
    }
}

fn rev_spawn_empty_inner(
    entity_mut: &mut EntityWorldMut,
    past_len: MetaPastLen,
    caller: MaybeLocation,
) {
    let entity = entity_mut.id();
    entity_mut.buffer_undo_redo(past_len, SpawnEmpty { entity, caller });
    entity_mut
        .resource_mut::<RevDespawnCleaner>()
        .log_spawn(entity, caller, past_len);
}

#[cfg(test)]
mod test {
    // todo
}
