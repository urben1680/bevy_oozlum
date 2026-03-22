use bevy_ecs::{
    bundle::{Bundle, InsertMode, NoBundleEffect},
    change_detection::MaybeLocation,
    entity::Entity,
    error::{HandleError, Result, warn},
    resource::Resource,
    schedule::ScheduleLabel,
    system::{Command, Commands, EntityCommands},
    world::{EntityWorldMut, FromWorld, World},
};

use crate::{
    meta::NotLog,
    undo_redo::{RevBundle, RevEntityWorld, RevWorld, mark_spawn_empty},
};

/// Extension trait for [`Commands`] with reversible variants of various methods.
pub trait RevCommands {
    /// Reversible version of [`Commands::run_schedule`].
    fn rev_run_schedule(&mut self, not_log: NotLog, label: impl ScheduleLabel);

    /// Reversible version of [`Commands::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, not_log: NotLog);

    /// Reversible version of [`Commands::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, not_log: NotLog, resource: R);

    /// Reversible version of [`Commands::remove_resource`].
    fn rev_remove_resource<R: Resource>(&mut self, not_log: NotLog);

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`Commands::rev_spawn`].
    ///
    /// When possible, use `Commands::rev_spawn` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned(&mut self, not_log: NotLog, entity: Entity, include_unlinked_related: bool);

    /// Helper method to mark a spawned batch as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`Commands::rev_spawn_batch`].
    ///
    /// When possible, use `Commands::rev_spawn_batch` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned_batch(
        &mut self,
        not_log: NotLog,
        entities: impl AsRef<[Entity]> + Send + 'static,
        include_unlinked_related: bool,
    );

    /// Command to reversibly despawn an entity.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn(&mut self, not_log: NotLog, entity: Entity);

    /// Command to reversibly despawn multiple entities.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_batch(
        &mut self,
        not_log: NotLog,
        entities: impl AsRef<[Entity]> + Send + 'static,
    );

    /// Reversible version of [`Commands::spawn`].
    fn rev_spawn<T: Bundle>(&mut self, not_log: NotLog, bundle: T) -> EntityCommands<'_>;

    /// Reversible version of [`Commands::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, not_log: NotLog, batch: I)
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static;

    /// Reversible version of [`Commands::spawn_empty`].
    fn rev_spawn_empty(&mut self, not_log: NotLog) -> EntityCommands<'_>;

    /// Reversible version of [`Commands::insert_batch`].
    fn rev_insert_batch<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;

    /// Reversible version of [`Commands::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;

    /// Reversible version of [`Commands::try_insert_batch`].
    fn rev_try_insert_batch<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;

    /// Reversible version of [`Commands::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;
}

impl RevCommands for Commands<'_, '_> {
    #[track_caller]
    fn rev_run_schedule(&mut self, not_log: NotLog, label: impl ScheduleLabel) {
        self.queue(rev_run_schedule(not_log, label).handle_error_with(warn));
    }

    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, not_log: NotLog) {
        self.queue(rev_init_resource::<R>(not_log))
    }

    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, not_log: NotLog, resource: R) {
        self.queue(rev_insert_resource(not_log, resource))
    }

    #[track_caller]
    fn rev_remove_resource<R: Resource>(&mut self, not_log: NotLog) {
        self.queue(rev_remove_resource::<R>(not_log))
    }

    #[track_caller]
    fn rev_spawn<T: Bundle>(&mut self, not_log: NotLog, bundle: T) -> EntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.spawn(bundle);
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            entity_mut
                .rev_mark_spawned_with_caller(not_log, true, caller)
                .unwrap();
        });
        entity_cmds
    }

    #[track_caller]
    fn rev_mark_spawned(
        &mut self,
        not_log: NotLog,
        entity: Entity,
        include_unlinked_related: bool,
    ) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_mark_spawned_with_caller(not_log, entity, include_unlinked_related, caller);
        });
    }

    #[track_caller]
    fn rev_mark_spawned_batch(
        &mut self,
        not_log: NotLog,
        entities: impl AsRef<[Entity]> + Send + 'static,
        include_unlinked_related: bool,
    ) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_mark_spawned_batch_with_caller(
                not_log,
                entities.as_ref(),
                include_unlinked_related,
                caller,
            );
        });
    }

    #[track_caller]
    fn rev_despawn(&mut self, not_log: NotLog, entity: Entity) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_despawn_with_caller(not_log, entity, caller);
        });
    }

    #[track_caller]
    fn rev_despawn_batch(
        &mut self,
        not_log: NotLog,
        entities: impl AsRef<[Entity]> + Send + 'static,
    ) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_despawn_batch_with_caller(not_log, entities.as_ref(), caller);
        });
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, not_log: NotLog) -> EntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.spawn_empty();
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            mark_spawn_empty(not_log, &mut entity_mut, caller);
        });
        entity_cmds
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, not_log: NotLog, batch: I)
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
    {
        self.queue(rev_spawn_batch(not_log, batch));
    }

    #[track_caller]
    fn rev_insert_batch<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue(rev_insert_batch(not_log, iter, InsertMode::Replace));
    }

    #[track_caller]
    fn rev_insert_batch_if_new<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue(rev_insert_batch(not_log, iter, InsertMode::Keep));
    }

    #[track_caller]
    fn rev_try_insert_batch<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue_handled(rev_insert_batch(not_log, iter, InsertMode::Replace), warn);
    }

    #[track_caller]
    fn rev_try_insert_batch_if_new<I, B, Marker>(&mut self, not_log: NotLog, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue_handled(rev_insert_batch(not_log, iter, InsertMode::Keep), warn);
    }
}

/// Reversible version of [`run_schedule`](bevy_ecs::system::command::run_schedule).
#[track_caller]
pub fn rev_run_schedule(not_log: NotLog, label: impl ScheduleLabel) -> impl Command<Result> {
    let caller = MaybeLocation::caller();
    move |world: &mut World| -> Result {
        world.rev_try_run_schedule_with_caller(not_log, label, caller)?;
        Ok(())
    }
}

/// Reversible version of [`spawn_batch`](bevy_ecs::system::command::spawn_batch).
#[track_caller]
pub fn rev_spawn_batch<I>(not_log: NotLog, bundles_iter: I) -> impl Command
where
    I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
{
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_spawn_batch_with_caller(not_log, bundles_iter, caller);
    }
}

/// Reversible version of [`spawn_batch`](bevy_ecs::system::command::insert_batch).
///
/// If any entities do not exist in the world or are reversibly despawned, this command will return
/// a [`TryRevInsertBatchError`](super::TryRevInsertBatchError).
///
/// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
/// reversible spawn/despawn.
#[track_caller]
pub fn rev_insert_batch<I, B, Marker>(
    not_log: NotLog,
    iter: I,
    insert_mode: InsertMode,
) -> impl Command<Result>
where
    I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
    B: RevBundle<Marker>,
{
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world
            .rev_try_insert_batch_inner(iter, |mut entity_mut, bundle| {
                match insert_mode {
                    InsertMode::Replace => {
                        entity_mut.rev_insert_with_caller(not_log, bundle, caller)
                    }
                    InsertMode::Keep => {
                        entity_mut.rev_insert_if_new_with_caller(not_log, bundle, caller)
                    }
                }
                .map(|_| ())
            })
            .map_err(Into::into)
    }
}

/// Reversible version of [`init_resource`](bevy_ecs::system::command::init_resource).
#[track_caller]
pub fn rev_init_resource<R: Resource + FromWorld>(not_log: NotLog) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_init_resource_with_caller::<R>(not_log, caller);
    }
}

/// Reversible version of [`insert_resource`](bevy_ecs::system::command::insert_resource).
#[track_caller]
pub fn rev_insert_resource<R: Resource>(not_log: NotLog, resource: R) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_insert_resource_with_caller(not_log, resource, caller);
    }
}

/// Reversible version of [`remove_resource`](bevy_ecs::system::command::remove_resource).
pub fn rev_remove_resource<R: Resource>(not_log: NotLog) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_remove_resource_with_caller::<R, _>(not_log, |_| (), caller);
    }
}
