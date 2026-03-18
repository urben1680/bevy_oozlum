use bevy_ecs::{
    bundle::{Bundle, InsertMode, NoBundleEffect},
    change_detection::MaybeLocation,
    entity::Entity,
    error::{Result, warn},
    resource::Resource,
    system::{Command, Commands, EntityCommands},
    world::{EntityWorldMut, FromWorld, World},
};

use crate::{
    meta::MetaPastLen,
    undo_redo::{RevBundle, RevEntityWorldMutInternal, RevWorldInternal, mark_spawn_empty},
};

/// Extension trait for [`Commands`] with reversible variants of various methods.
pub trait RevCommands {
    /// Reversible version of [`Commands::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, meta_past_len: MetaPastLen);

    /// Reversible version of [`Commands::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen, resource: R);

    /// Reversible version of [`Commands::remove_resource`].
    fn rev_remove_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen);

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`Commands::rev_spawn`].
    ///
    /// When possible, use `Commands::rev_spawn` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
    );

    /// Helper method to mark a spawned batch as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`Commands::rev_spawn_batch`].
    ///
    /// When possible, use `Commands::rev_spawn_batch` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: impl AsRef<[Entity]> + Send + 'static,
        include_unlinked_related: bool,
    );

    /// Command to reversibly despawn an entity.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn(&mut self, meta_past_len: MetaPastLen, entity: Entity);

    /// Command to reversibly despawn multiple entities.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: impl AsRef<[Entity]> + Send + 'static,
    );

    /// Reversible version of [`Commands::spawn`].
    fn rev_spawn<T: Bundle>(&mut self, meta_past_len: MetaPastLen, bundle: T)
    -> EntityCommands<'_>;

    /// Reversible version of [`Commands::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, meta_past_len: MetaPastLen, batch: I)
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static;

    /// Reversible version of [`Commands::spawn_empty`].
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityCommands<'_>;

    /// Reversible version of [`Commands::insert_batch`].
    fn rev_insert_batch<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;

    /// Reversible version of [`Commands::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;

    /// Reversible version of [`Commands::try_insert_batch`].
    fn rev_try_insert_batch<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;

    /// Reversible version of [`Commands::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>;
}

impl RevCommands for Commands<'_, '_> {
    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, meta_past_len: MetaPastLen) {
        self.queue(rev_init_resource::<R>(meta_past_len))
    }

    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen, resource: R) {
        self.queue(rev_insert_resource(meta_past_len, resource))
    }

    #[track_caller]
    fn rev_remove_resource<R: Resource>(&mut self, meta_past_len: MetaPastLen) {
        self.queue(rev_remove_resource::<R>(meta_past_len))
    }

    #[track_caller]
    fn rev_spawn<T: Bundle>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: T,
    ) -> EntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.spawn(bundle);
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            entity_mut
                .rev_mark_spawned_with_caller(meta_past_len, true, caller)
                .unwrap();
        });
        entity_cmds
    }

    #[track_caller]
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
        include_unlinked_related: bool,
    ) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_mark_spawned_with_caller(
                meta_past_len,
                entity,
                include_unlinked_related,
                caller,
            );
        });
    }

    #[track_caller]
    fn rev_mark_spawned_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: impl AsRef<[Entity]> + Send + 'static,
        include_unlinked_related: bool,
    ) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_mark_spawned_batch_with_caller(
                meta_past_len,
                entities.as_ref(),
                include_unlinked_related,
                caller,
            );
        });
    }

    #[track_caller]
    fn rev_despawn(&mut self, meta_past_len: MetaPastLen, entity: Entity) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_despawn_with_caller(meta_past_len, entity, caller);
        });
    }

    #[track_caller]
    fn rev_despawn_batch(
        &mut self,
        meta_past_len: MetaPastLen,
        entities: impl AsRef<[Entity]> + Send + 'static,
    ) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world.rev_despawn_batch_with_caller(meta_past_len, entities.as_ref(), caller);
        });
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.spawn_empty();
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            mark_spawn_empty(meta_past_len, &mut entity_mut, caller);
        });
        entity_cmds
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, meta_past_len: MetaPastLen, batch: I)
    where
        I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
    {
        self.queue(rev_spawn_batch(meta_past_len, batch));
    }

    #[track_caller]
    fn rev_insert_batch<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue(rev_insert_batch(meta_past_len, iter, InsertMode::Replace));
    }

    #[track_caller]
    fn rev_insert_batch_if_new<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue(rev_insert_batch(meta_past_len, iter, InsertMode::Keep));
    }

    #[track_caller]
    fn rev_try_insert_batch<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue_handled(
            rev_insert_batch(meta_past_len, iter, InsertMode::Replace),
            warn,
        );
    }

    #[track_caller]
    fn rev_try_insert_batch_if_new<I, B, Marker>(&mut self, meta_past_len: MetaPastLen, iter: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: RevBundle<Marker>,
    {
        self.queue_handled(
            rev_insert_batch(meta_past_len, iter, InsertMode::Keep),
            warn,
        );
    }
}

/// Reversible version of [`spawn_batch`](bevy_ecs::system::command::spawn_batch).
#[track_caller]
pub fn rev_spawn_batch<I>(meta_past_len: MetaPastLen, bundles_iter: I) -> impl Command
where
    I: IntoIterator<Item: Bundle<Effect: NoBundleEffect>> + Send + 'static,
{
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_spawn_batch_with_caller(meta_past_len, bundles_iter, caller);
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
    meta_past_len: MetaPastLen,
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
                        entity_mut.rev_insert_with_caller(meta_past_len, bundle, caller)
                    }
                    InsertMode::Keep => {
                        entity_mut.rev_insert_if_new_with_caller(meta_past_len, bundle, caller)
                    }
                }
                .map(|_| ())
            })
            .map_err(Into::into)
    }
}

/// Reversible version of [`init_resource`](bevy_ecs::system::command::init_resource).
#[track_caller]
pub fn rev_init_resource<R: Resource + FromWorld>(meta_past_len: MetaPastLen) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_init_resource_with_caller::<R>(meta_past_len, caller);
    }
}

/// Reversible version of [`insert_resource`](bevy_ecs::system::command::insert_resource).
#[track_caller]
pub fn rev_insert_resource<R: Resource>(meta_past_len: MetaPastLen, resource: R) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_insert_resource_with_caller(meta_past_len, resource, caller);
    }
}

/// Reversible version of [`remove_resource`](bevy_ecs::system::command::remove_resource).
pub fn rev_remove_resource<R: Resource>(meta_past_len: MetaPastLen) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        world.rev_remove_resource_with_caller::<R, _>(meta_past_len, |_| (), caller);
    }
}
