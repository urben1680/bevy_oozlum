use core::ops::{Deref, DerefMut};

use bevy_ecs::{
    bundle::{Bundle, InsertMode, NoBundleEffect},
    change_detection::MaybeLocation,
    entity::Entity,
    error::{HandleError, Result, warn},
    resource::Resource,
    schedule::ScheduleLabel,
    system::{Command, Commands},
    world::{EntityWorldMut, FromWorld, World},
};

use crate::{
    meta::NotLog,
    undo_redo::{
        CommandsAsRev, RevBundle, RevEntityWorld, RevWorld, UndoRedo, entity_commands::RevEntityCommands,
        mark_spawn_empty,
    },
};

pub struct RevCommands<'a>(pub(super) Commands<'a, 'a>);

impl<'a> From<RevCommands<'a>> for Commands<'a, 'a> {
    fn from(value: RevCommands<'a>) -> Self {
        value.0
    }
}

impl CommandsAsRev for Commands<'_, '_> {
    type Out<'a>
        = RevCommands<'a>
    where
        Self: 'a;
    fn as_rev(&mut self, not_log: NotLog) -> Self::Out<'_> {
        RevCommands::new(not_log, self)
    }
}

impl<'a> Deref for RevCommands<'a> {
    type Target = Commands<'a, 'a>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> DerefMut for RevCommands<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> RevCommands<'a> {
    /// Construct `RevCommands` during [`RevDirection::NotLog`](super::RevDirection::NotLog).
    pub fn new(_: NotLog, commands: &'a mut Commands) -> Self {
        Self(commands.reborrow())
    }

    /// Returns a [`RevCommands`] with a smaller lifetime.
    ///
    /// This is useful if you have `&mut RevCommands` but need `RevCommands`.
    pub fn reborrow(&mut self) -> RevCommands<'_> {
        RevCommands(self.0.reborrow())
    }

    /// Queues an [`UndoRedo`] implementor in a resource to be collected by the reversible system's
    /// state.
    ///
    /// **Note** that the non-log operation that is related to this should also be done in a
    /// command to ensure the order of operations is correctly reversed at undo.
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy_oozlum::prelude::*;
    /// # fn system(not_log: NotLog, mut commands: Commands) {
    /// // Wrong: having the non-log operation happen in the system
    /// // println!("hello world!")
    ///
    /// // Correct: having the non-log operation happen in a command, just like the undo/redo
    /// commands.queue(|_: &mut World| println!("hello world!"));
    /// commands.as_rev(not_log).queue_undo_redo(|_: &mut World, direction| {
    ///     match direction {
    ///         UndoRedoDirection::Undo => println!("!dlrow olleh (log)"),
    ///         UndoRedoDirection::Redo => println!("hello world! (log)"),
    ///     }
    /// });
    /// # }
    /// ```
    #[track_caller]
    pub fn queue_undo_redo(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.queue_undo_redo_with_caller(undo_redo, MaybeLocation::caller())
    }

    /// As [`queue_undo_redo`](Self::queue_undo_redo) but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`run_rev_update`](crate::schedule::run_rev_update) may return the relevant error in that case.
    pub fn queue_undo_redo_with_caller(
        &mut self,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.0.queue(move |world: &mut World| {
            world.queue_undo_redo(undo_redo, caller);
        });
        self
    }

    /// Queues an [`UndoRedo`] implementor in a resource to be collected by the reversible system's
    /// state.
    ///
    /// This will also trigger the [redo logic] at the sync point.
    ///
    /// This shorthand method is useful for when applying the reversible operation is doing the
    /// exact same as it's redo logic.
    ///
    /// [redo logic]: UndoRedo::redo
    #[track_caller]
    pub fn redo_and_queue(&mut self, undo_redo: impl UndoRedo) -> &mut Self {
        self.redo_and_queue_with_caller(undo_redo, MaybeLocation::caller())
    }

    /// As [`redo_and_queue`](Self::redo_and_queue) but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`run_rev_update`](crate::schedule::run_rev_update) may return the relevant error in that case.
    pub fn redo_and_queue_with_caller(
        &mut self,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.0.queue(move |world: &mut World| {
            world.redo_and_queue(undo_redo, caller);
        });
        self
    }

    /// Reversible version of [`Commands::run_schedule`].
    #[track_caller]
    pub fn rev_run_schedule(&mut self, label: impl ScheduleLabel) {
        self.0
            .queue(rev_run_schedule_inner(label, MaybeLocation::caller()).handle_error_with(warn));
    }

    /// Reversible version of [`Commands::init_resource`].
    #[track_caller]
    pub fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        self.0
            .queue(rev_init_resource_inner::<R>(MaybeLocation::caller()))
    }

    /// Reversible version of [`Commands::insert_resource`].
    #[track_caller]
    pub fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        self.0
            .queue(rev_insert_resource_inner(resource, MaybeLocation::caller()))
    }

    /// Reversible version of [`Commands::remove_resource`].
    #[track_caller]
    pub fn rev_remove_resource<R: Resource>(&mut self) {
        self.0
            .queue(rev_remove_resource_inner::<R>(MaybeLocation::caller()))
    }

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`RevCommands::rev_spawn`].
    ///
    /// When possible, use `Commands::rev_spawn` instead.
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_mark_spawned(&mut self, entity: Entity, include_unlinked_related: bool) {
        let caller = MaybeLocation::caller();
        self.0.queue(move |world: &mut World| {
            world.rev_mark_spawned(entity, include_unlinked_related, caller);
        });
    }

    /// Helper method to mark a spawned batch as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`RevCommands::rev_spawn_batch`].
    ///
    /// When possible, use `Commands::rev_spawn_batch` instead.
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_mark_spawned_batch(
        &mut self,
        entities: impl AsRef<[Entity]> + Send + 'static,
        include_unlinked_related: bool,
    ) {
        let caller = MaybeLocation::caller();
        self.0.queue(move |world: &mut World| {
            world.rev_mark_spawned_batch(entities.as_ref(), include_unlinked_related, caller);
        });
    }

    /// Command to reversibly despawn an entity.
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_despawn(&mut self, entity: Entity) {
        let caller = MaybeLocation::caller();
        self.0.queue(move |world: &mut World| {
            world.rev_despawn(entity, caller);
        });
    }

    /// Command to reversibly despawn multiple entities.
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_despawn_batch(&mut self, entities: impl AsRef<[Entity]> + Send + 'static) {
        let caller = MaybeLocation::caller();
        self.0.queue(move |world: &mut World| {
            world.rev_despawn_batch(entities.as_ref(), caller);
        });
    }

    /// Reversible version of [`Commands::spawn`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_spawn<T: Bundle>(&mut self, bundle: T) -> RevEntityCommands<'_> {
        rev_spawn_inner(&mut self.0, bundle, MaybeLocation::caller())
    }

    /// Reversible version of [`Commands::spawn_empty`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_spawn_empty(&mut self) -> RevEntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.0.spawn_empty();
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            mark_spawn_empty(&mut entity_mut, caller);
        });
        RevEntityCommands(entity_cmds)
    }

    /// Reversible version of [`Commands::spawn_batch`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_spawn_batch<I>(&mut self, batch: I)
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
    {
        self.0
            .queue(rev_spawn_batch_inner(batch, MaybeLocation::caller()));
    }

    /// Reversible version of [`Commands::insert_batch`].
    #[track_caller]
    pub fn rev_insert_batch<I, B, Marker>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.0.queue(rev_insert_batch_inner(
            iter,
            InsertMode::Replace,
            MaybeLocation::caller(),
        ));
    }

    /// Reversible version of [`Commands::insert_batch_if_new`].
    #[track_caller]
    pub fn rev_insert_batch_if_new<I, B, Marker>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.0.queue(rev_insert_batch_inner(
            iter,
            InsertMode::Keep,
            MaybeLocation::caller(),
        ));
    }

    /// Reversible version of [`Commands::try_insert_batch`].
    #[track_caller]
    pub fn rev_try_insert_batch<I, B, Marker>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.0.queue_handled(
            rev_insert_batch_inner(iter, InsertMode::Replace, MaybeLocation::caller()),
            warn,
        );
    }

    /// Reversible version of [`Commands::try_insert_batch_if_new`].
    #[track_caller]
    pub fn rev_try_insert_batch_if_new<I, B, Marker>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.0.queue_handled(
            rev_insert_batch_inner(iter, InsertMode::Keep, MaybeLocation::caller()),
            warn,
        );
    }
}

/// Reversible version of [`spawn_batch`](bevy_ecs::system::command::insert_batch).
///
/// If any entities do not exist in the world or are reversibly despawned, this command will return
/// an error.
///
/// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
/// reversible spawn/despawn.
#[track_caller]
pub fn rev_insert_batch<I, B, Marker>(
    _: NotLog,
    iter: I,
    insert_mode: InsertMode,
) -> impl Command<Result>
where
    I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
    B: RevBundle<Marker>,
{
    rev_insert_batch_inner(iter, insert_mode, MaybeLocation::caller())
}

fn rev_insert_batch_inner<I, B, Marker>(
    iter: I,
    insert_mode: InsertMode,
    caller: MaybeLocation,
) -> impl Command<Result>
where
    I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
    B: RevBundle<Marker>,
{
    move |world: &mut World| {
        world
            .rev_try_insert_batch_inner(iter, |mut entity_mut, bundle| {
                match insert_mode {
                    InsertMode::Replace => entity_mut.rev_insert(bundle, caller),
                    InsertMode::Keep => entity_mut.rev_insert_if_new(bundle, caller),
                }
                .map(|_| ())
            })
            .map_err(Into::into)
    }
}

/// Reversible version of [`init_resource`](bevy_ecs::system::command::init_resource).
#[track_caller]
pub fn rev_init_resource<R: Resource + FromWorld>(_: NotLog) -> impl Command {
    rev_init_resource_inner::<R>(MaybeLocation::caller())
}

fn rev_init_resource_inner<R: Resource + FromWorld>(caller: MaybeLocation) -> impl Command {
    move |world: &mut World| {
        world.rev_init_resource::<R>(caller);
    }
}

/// Reversible version of [`insert_resource`](bevy_ecs::system::command::insert_resource).
#[track_caller]
pub fn rev_insert_resource<R: Resource>(_: NotLog, resource: R) -> impl Command {
    rev_insert_resource_inner(resource, MaybeLocation::caller())
}

fn rev_insert_resource_inner<R: Resource>(resource: R, caller: MaybeLocation) -> impl Command {
    move |world: &mut World| {
        world.rev_insert_resource(resource, caller);
    }
}

/// Reversible version of [`remove_resource`](bevy_ecs::system::command::remove_resource).
#[track_caller]
pub fn rev_remove_resource<R: Resource>(_: NotLog) -> impl Command {
    rev_remove_resource_inner::<R>(MaybeLocation::caller())
}

fn rev_remove_resource_inner<R: Resource>(caller: MaybeLocation) -> impl Command {
    move |world: &mut World| {
        world.rev_remove_resource::<R, _>(|_| (), caller);
    }
}

/// Reversible version of [`spawn_batch`](bevy_ecs::system::command::spawn_batch).
pub fn rev_spawn_batch<I>(_: NotLog, bundles_iter: I) -> impl Command
where
    I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
{
    rev_spawn_batch_inner(bundles_iter, MaybeLocation::caller())
}

fn rev_spawn_batch_inner<I>(bundles_iter: I, caller: MaybeLocation) -> impl Command
where
    I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
{
    move |world: &mut World| {
        world.rev_spawn_batch(bundles_iter, caller);
    }
}

/// Reversible version of [`run_schedule`](bevy_ecs::system::command::run_schedule).
pub fn rev_run_schedule(_: NotLog, label: impl ScheduleLabel) -> impl Command<Result> {
    rev_run_schedule_inner(label, MaybeLocation::caller())
}

fn rev_run_schedule_inner(
    label: impl ScheduleLabel,
    caller: MaybeLocation,
) -> impl Command<Result> {
    move |world: &mut World| -> Result {
        world.rev_try_run_schedule(label, caller)?;
        Ok(())
    }
}

pub(super) fn rev_spawn_inner<'a, T: Bundle>(
    commands: &'a mut Commands,
    bundle: T,
    caller: MaybeLocation,
) -> RevEntityCommands<'a> {
    let mut entity_cmds = commands.spawn(bundle);
    entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
        entity_mut.rev_mark_spawned(true, caller).map(|_| ())
    });
    RevEntityCommands(entity_cmds)
}
