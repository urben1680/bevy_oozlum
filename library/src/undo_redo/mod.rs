use core::{
    any::type_name_of_val,
    error::Error,
    fmt::{Debug, Display},
};

use bevy_ecs::{
    change_detection::MaybeLocation,
    entity::Entity,
    resource::Resource,
    system::{Commands, EntityCommands},
    world::{EntityWorldMut, World},
};
use bevy_platform::cell::SyncCell;
use bevy_utils::prelude::DebugName;

use crate::{
    log::{OutOfLog, TransitionsLog, UpdateLog},
    meta::{NotLog, RevDirection, RevMeta},
};

mod commands;
mod entity_commands;
mod entity_world;
mod insert_remove;
mod relationship;
mod spawn_despawn;
mod world;

pub use commands::*;
pub use entity_commands::*;
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

/// Error type that multiple reversible APIs may return.
#[derive(Copy, Clone, Debug)]
pub struct EntityRevDespawnedError {
    /// The entity that was reversibly despawned while some reversible operation was attempted on
    /// it.
    pub entity: Entity,

    /// The calling site of the failed reversible operation. Requires bevy's `track_location`
    /// feature to be active.
    pub caller: MaybeLocation,
}

impl Display for EntityRevDespawnedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "entity {} is marked as reversibly despawned{LOCATION_PREFIX}{}",
            self.entity, self.caller
        )
    }
}

impl Error for EntityRevDespawnedError {}

/// Extension trait for the buffering reversible operations.
pub trait BuffersUndoRedo {
    /// Buffers an [`UndoRedo`] implementor in a resource to be collected by the reversible system's
    /// state.
    #[track_caller]
    fn buffer_undo_redo(&mut self, not_log: NotLog, undo_redo: impl UndoRedo) -> &mut Self {
        self.buffer_undo_redo_with_caller(not_log, undo_redo, MaybeLocation::caller())
    }

    /// Buffers an [`UndoRedo`] implementor in a resource to be collected by the reversible system's
    /// state.
    ///
    /// This will also trigger the [redo logic] at the sync point.
    ///
    /// This shorthand method is useful for when applying the reversible operation is doing the
    /// exact same as it's redo logic.
    ///
    /// [redo logic]: UndoRedo::redo
    #[track_caller]
    fn redo_and_buffer(&mut self, not_log: NotLog, undo_redo: impl UndoRedo) -> &mut Self {
        self.redo_and_buffer_with_caller(not_log, undo_redo, MaybeLocation::caller())
    }

    /// As [`BuffersUndoRedo::buffer_undo_redo`] but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`run_rev_update`](crate::meta::run_rev_update) may return the relevant error in that case.
    fn buffer_undo_redo_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self;

    /// As [`BuffersUndoRedo::redo_and_buffer`] but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`run_rev_update`](crate::meta::run_rev_update) may return the relevant error in that case.
    fn redo_and_buffer_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self;
}

impl BuffersUndoRedo for Commands<'_, '_> {
    fn buffer_undo_redo_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.queue(move |world: &mut World| {
            world.buffer_undo_redo(not_log, undo_redo, caller);
        });
        self
    }

    fn redo_and_buffer_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.queue(move |world: &mut World| {
            world.redo_and_buffer(not_log, undo_redo, caller);
        });
        self
    }
}

impl BuffersUndoRedo for EntityCommands<'_> {
    fn buffer_undo_redo_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.queue(move |mut world: EntityWorldMut| {
            world.buffer_undo_redo(not_log, undo_redo, caller);
        });
        self
    }

    fn redo_and_buffer_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.queue(move |mut world: EntityWorldMut| {
            world.redo_and_buffer(not_log, undo_redo, caller);
        });
        self
    }
}

/// Reversible operations store their [`UndoRedo`] value in this resource so reversible systems can
/// load them after that. This way, these systems can undo and redo them.
#[derive(Resource, Default)]
pub(crate) struct UndoRedoBuffer(Vec<BoxedUndoRedo>);

impl UndoRedoBuffer {
    /// Returns `true` when the buffer is empty, otherwise returns `false`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    #[track_caller]
    pub(crate) fn buffer_undo_redo<T: UndoRedo>(
        &mut self,
        _: NotLog,
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

impl Debug for UndoRedoBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (size_of::<DebugName>(), size_of::<MaybeLocation>()) {
            (0, 0) => write!(
                f,
                "activate bevy's \"debug\" and \"track_location\" features to show more details"
            ),
            (0, _) => write!(
                f,
                "{:?}, activate bevy's \"debug\" feature to show more details",
                self.0
            ),
            (_, 0) => write!(
                f,
                "{:?}, activate bevy's \"track_location\" feature to show more details",
                self.0
            ),
            _ => self.0.fmt(f),
        }
    }
}

struct BoxedUndoRedo {
    undo_redo: SyncCell<Box<dyn UndoRedo>>,
    name: DebugName,
    caller: MaybeLocation,
}

impl Debug for BoxedUndoRedo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if size_of::<DebugName>() == 0 {
            match self.caller.into_option() {
                Some(location) => write!(f, "{location}"),
                None => Ok(()),
            }
        } else {
            match self.caller.into_option() {
                Some(location) => write!(f, "{} from {location}", self.name),
                None => write!(f, "{}", self.name),
            }
        }
    }
}

/// Trait that all reversible (deferred) operations use to define how to undo and redo them.
/// They are stored in the system's state that did them, including indirectly via commands, hooks or
/// observers.
///
/// Custom `FnMut(&mut World, UndoRedoDirection)` closures can be buffered via [`BuffersUndoRedo`].
/// See [`UndoRedoDirection`] docs.
pub trait UndoRedo: Send + 'static {
    /// Undo the reversible operation during [`RevDirection::BackwardLog`].
    fn undo(&mut self, world: &mut World);

    /// Redo the reversible operation during [`RevDirection::ForwardLog`].
    fn redo(&mut self, world: &mut World);
}

/// Second argument for closures implementing [`UndoRedo`].
///
/// ```
/// # use bevy_ecs::world::World;
/// # use bevy_oozlum::prelude::*;
/// # let RevDirection::NotLog(not_log) = RevDirection::BackwardLog else {
/// #     return;
/// # };
/// # let mut world = World::new();
/// # let mut commands = world.commands();
/// commands.buffer_undo_redo(not_log, |world: &mut World, direction| {
///     match direction {
///         UndoRedoDirection::Undo => {
///             // undo logic
///         },
///         UndoRedoDirection::Redo => {
///             // redo logic
///         }
///     }
/// });
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoRedoDirection {
    /// Apply undo logic when this is matched.
    Undo,

    /// Apply redo logic when this is matched.
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

/// Part of the state of reversible systems to undo/redo reversible operations.
#[derive(Default, Debug)]
pub(crate) struct UndoRedoLog {
    update_log: UpdateLog,
    undo_redo_log: TransitionsLog<DebugHidden>,
}

struct DebugHidden(SyncCell<Box<dyn UndoRedo>>);

impl Debug for DebugHidden {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "_")
    }
}

#[derive(Debug)]
pub(crate) enum UndoRedoLogError {
    RevMetaMissing,
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
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RevMetaMissing => write!(f, "RevMeta is missing"),
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
    /// At [`RevDirection::NotLog`], load applied reversible operations from [`UndoRedoBuffer`],
    /// if any.
    ///
    /// At [`RevDirection::ForwardLog`], redo reversible operations, if any.
    pub(crate) fn forward(&mut self, world: &mut World) -> Result<(), UndoRedoLogError> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(UndoRedoLogError::RevMetaMissing)?;

        let now = meta.now();
        match meta.get_running_direction() {
            Some(RevDirection::NotLog(_)) => {
                // UndoRedoBuffer may not exist if no reversible commands were buffered yet
                world.try_resource_scope::<UndoRedoBuffer, _>(|world, mut buffer| {
                    if !buffer.0.is_empty() {
                        let meta = world.resource::<RevMeta>();
                        let not_log = self
                            .update_log
                            .forward_past_len_with_caller(meta, MaybeLocation::new(None));
                        let buffers = buffer.0.drain(..).map(|boxed| DebugHidden(boxed.undo_redo));
                        self.undo_redo_log.forward_extend(meta, not_log, buffers);
                    }
                });
                Ok(())
            }
            Some(RevDirection::ForwardLog) => {
                if self
                    .update_log
                    .forward_log_with_caller(meta, MaybeLocation::new(None))
                {
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

    /// At [`RevDirection::BackwardLog`], redo reversible operations, if any.
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

        if self
            .update_log
            .backward_log_with_caller(meta, MaybeLocation::new(None))
        {
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
    use core::ops::Deref;

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
        forward: impl FnOnce(&mut World, NotLog) -> T,
        backward_log: impl FnOnce(&mut World, &mut T),
        forward_log: impl FnOnce(&mut World, &mut T),
    ) {
        assert_undo_redo_finalize(world, forward, backward_log, Some(forward_log), |_, _| {})
    }

    pub(super) fn assert_undo_redo_finalize<T>(
        world: &mut World,
        forward: impl FnOnce(&mut World, NotLog) -> T,
        backward_log: impl FnOnce(&mut World, &mut T),
        forward_log: Option<impl FnOnce(&mut World, &mut T)>,
        finalize: impl FnOnce(&mut World, &mut T),
    ) {
        use crate::meta::RevQueue;

        crate::panic_on_error_events();
        world.register_disabling_component::<RevDespawned>();
        let mut meta = RevMeta::default();
        let mut state = None;

        // forward
        meta = meta
            .update(|mut meta, direction| {
                assert_eq!(direction, RevDirection::NOT_LOG_MIN);
                meta.set_queue(RevQueue::RunBackwardLog);
                world.insert_resource(meta);
                state = Some(forward(world, direction.past_len()));
                finalize_despawns(world).unwrap();
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
                finalize_despawns(world).unwrap();
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
                    finalize_despawns(world).unwrap();
                    forward_log(world, state.as_mut().unwrap());
                    world.remove_resource()
                })
                .unwrap();
        }

        // finalize
        meta.update(|meta, _| {
            world.insert_resource(meta);
            finalize_despawns(world).unwrap();
            finalize(world, state.as_mut().unwrap());
            world.remove_resource()
        })
        .unwrap();
    }
}
