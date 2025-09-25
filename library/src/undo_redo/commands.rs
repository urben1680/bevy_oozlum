use crate::{
    meta::NonLogNow,
    prelude::UndoRedo,
    undo_redo::{
        RevDespawnCleaner, RevWorld, rev_init_resource_with_caller,
        rev_insert_resource_with_caller, rev_remove_resource_with_caller,
        rev_spawn_batch_with_caller, rev_spawn_despawn_with_caller, rev_spawn_empty_inner,
    },
};
use bevy_ecs::{
    bundle::{Bundle, DynamicBundle, NoBundleEffect},
    change_detection::MaybeLocation,
    entity::Entity,
    resource::Resource,
    system::{Command, Commands, EntityCommands},
    world::{EntityWorldMut, FromWorld, World},
};

pub trait RevCommands {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    fn rev_log_scope(&mut self, now: NonLogNow, entity: Entity);

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`Commands::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, now: NonLogNow);

    // rev_insert_batch
    // no efficient algorithm found yet

    // rev_insert_batch_if_new
    // no efficient algorithm found yet

    /// Reversible version of [`Commands::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, now: NonLogNow, resource: R);

    /// Reversible version of [`Commands::remove_resource`].
    fn rev_remove_resource<R: Resource>(&mut self, now: NonLogNow);

    /// Reversible version of [`Commands::spawn`].
    fn rev_spawn<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> EntityCommands;

    /// Reversible version of [`Commands::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, batch: I)
    where
        I: IntoIterator + Send + Sync + 'static,
        <I as IntoIterator>::Item: Bundle,
        <<I as IntoIterator>::Item as DynamicBundle>::Effect: NoBundleEffect;

    /// Reversible version of [`Commands::spawn_empty`].
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityCommands;

    // rev_try_insert_batch
    // no efficient algorithm found yet

    // rev_try_insert_batch_if_new
    // no efficient algorithm found yet
}

impl RevCommands for Commands<'_, '_> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        self.queue(move |world: &mut World| world.redo_and_buffer(now, undo_redo));
    }

    #[track_caller]
    fn rev_log_scope(&mut self, now: NonLogNow, entity: Entity) {
        let caller = MaybeLocation::caller();
        self.queue(move |world: &mut World| {
            world
                .resource_mut::<RevDespawnCleaner>()
                .log_spawn(entity, caller, now);
        });
    }

    #[track_caller]
    fn rev_init_resource<R: Resource + FromWorld>(&mut self, now: NonLogNow) {
        self.queue(rev_init_resource::<R>(now))
    }

    #[track_caller]
    fn rev_insert_resource<R: Resource>(&mut self, now: NonLogNow, resource: R) {
        self.queue(rev_insert_resource(now, resource))
    }

    #[track_caller]
    fn rev_remove_resource<R: Resource>(&mut self, now: NonLogNow) {
        self.queue(rev_remove_resource::<R>(now))
    }

    #[track_caller]
    fn rev_spawn<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> EntityCommands {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.spawn(bundle);
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            rev_spawn_despawn_with_caller::<true>(&mut entity_mut, now, caller);
        });
        entity_cmds
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, now: NonLogNow) -> EntityCommands {
        let caller = MaybeLocation::caller();
        let mut entity_cmds = self.spawn_empty();
        entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
            rev_spawn_empty_inner(&mut entity_mut, now, caller)
        });
        entity_cmds
    }

    #[track_caller]
    fn rev_spawn_batch<I>(&mut self, now: NonLogNow, batch: I)
    where
        I: IntoIterator + Send + Sync + 'static,
        <I as IntoIterator>::Item: Bundle,
        <<I as IntoIterator>::Item as DynamicBundle>::Effect: NoBundleEffect,
    {
        self.queue(rev_spawn_batch(now, batch));
    }
}

/// Reversible version of [`spawn_batch`](bevy_ecs::system::command::spawn_batch).
#[track_caller]
pub fn rev_spawn_batch<I>(now: NonLogNow, bundles_iter: I) -> impl Command
where
    I: IntoIterator + Send + Sync + 'static,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        rev_spawn_batch_with_caller(world, now, bundles_iter, caller);
    }
}

/// Reversible version of [`init_resource`](bevy_ecs::system::command::init_resource).
#[track_caller]
pub fn rev_init_resource<R: Resource + FromWorld>(now: NonLogNow) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        rev_init_resource_with_caller::<R>(world, now, caller);
    }
}

/// Reversible version of [`insert_resource`](bevy_ecs::system::command::insert_resource).
#[track_caller]
pub fn rev_insert_resource<R: Resource>(now: NonLogNow, resource: R) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        rev_insert_resource_with_caller(world, now, resource, caller);
    }
}

/// Reversible version of [`remove_resource`](bevy_ecs::system::command::remove_resource).
pub fn rev_remove_resource<R: Resource>(now: NonLogNow) -> impl Command {
    let caller = MaybeLocation::caller();
    move |world: &mut World| {
        rev_remove_resource_with_caller::<R, _>(world, now, |_| (), caller);
    }
}
