use bevy::ecs::{
    bundle::{Bundle, DynamicBundle, InsertMode, NoBundleEffect},
    entity::Entity,
    error::{ErrorContext, Result as CommandResult, warn},
    resource::Resource,
    system::{Command, Commands, EntityCommands},
    world::{FromWorld, World},
};

use super::*;

pub trait RevCommands {
    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`Commands::init_resource`].
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);

    /// Reversible version of [`Commands::insert_batch`].
    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect;

    /// Reversible version of [`Commands::insert_batch_if_new`].
    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect;

    /// Reversible version of [`Commands::insert_resource`].
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);

    /// Reversible version of [`Commands::remove_resource`].
    fn rev_remove_resource<R: Resource>(&mut self);

    /// Reversible version of [`Commands::spawn`].
    fn rev_spawn<T: Bundle>(&mut self, bundle: T) -> EntityCommands;

    /// Reversible version of [`Commands::spawn_batch`].
    fn rev_spawn_batch<I>(&mut self, batch: I)
    where
        I: IntoIterator + Send + Sync + 'static,
        <I as IntoIterator>::Item: Bundle,
        <<I as IntoIterator>::Item as DynamicBundle>::Effect: NoBundleEffect;

    /// Reversible version of [`Commands::spawn_empty`].
    fn rev_spawn_empty(&mut self) -> EntityCommands;

    /// Reversible version of [`Commands::try_insert_batch`].
    fn rev_try_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect;

    /// Reversible version of [`Commands::try_insert_batch_if_new`].
    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect;
}

impl RevCommands for Commands<'_, '_> {
    fn rev_spawn<T: Bundle>(&mut self, bundle: T) -> EntityCommands {
        after_spawn(self.spawn(bundle))
    }

    fn rev_spawn_empty(&mut self) -> EntityCommands {
        after_spawn(self.spawn_empty())
    }

    fn rev_spawn_batch<I>(&mut self, batch: I)
    where
        I: IntoIterator + Send + Sync + 'static,
        <I as IntoIterator>::Item: Bundle,
        <<I as IntoIterator>::Item as DynamicBundle>::Effect: NoBundleEffect,
    {
        self.queue(move |world: &mut World| {
            world.rev_spawn_batch(batch);
        })
    }

    fn rev_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect,
    {
        self.queue(move |world: &mut World| {
            world.rev_insert_batch(batch);
        })
    }

    fn rev_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect,
    {
        self.queue(move |world: &mut World| {
            world.rev_insert_batch_if_new(batch);
        })
    }

    fn rev_try_insert_batch<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect,
    {
        self.queue(move |world: &mut World| {
            if let Err(error) = world.rev_try_insert_batch(batch) {
                let name = type_name_of_val(&Commands::rev_try_insert_batch::<I, B>).into();
                warn(error.into(), ErrorContext::Command { name });
            }
        })
    }

    fn rev_try_insert_batch_if_new<I, B>(&mut self, batch: I)
    where
        I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
        B: Bundle,
        <B as DynamicBundle>::Effect: NoBundleEffect,
    {
        self.queue(move |world: &mut World| {
            if let Err(error) = world.rev_try_insert_batch_if_new(batch) {
                let name = type_name_of_val(&Commands::rev_try_insert_batch_if_new::<I, B>).into();
                warn(error.into(), ErrorContext::Command { name });
            }
        })
    }

    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        self.queue(rev_init_resource::<R>())
    }

    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        self.queue(rev_insert_resource(resource))
    }

    fn rev_remove_resource<R: Resource>(&mut self) {
        self.queue(rev_remove_resource::<R>())
    }
}

/// Reversible version of [`spawn_batch`](bevy::ecs::system::command::spawn_batch).
#[track_caller]
pub fn rev_spawn_batch<I>(bundles_iter: I) -> impl Command
where
    I: IntoIterator + Send + Sync + 'static,
    I::Item: Bundle<Effect: NoBundleEffect>,
{
    |world: &mut World| {
        world.rev_spawn_batch(bundles_iter);
    }
}

/// Reversible version of [`insert_batch`].
#[track_caller]
pub fn rev_insert_batch<I, B>(batch: I, insert_mode: InsertMode) -> impl Command<CommandResult>
where
    I: IntoIterator<Item = (Entity, B)> + Send + Sync + 'static,
    B: Bundle<Effect: NoBundleEffect>,
{
    move |world: &mut World| {
        match insert_mode {
            InsertMode::Replace => world.rev_try_insert_batch(batch),
            InsertMode::Keep => world.rev_try_insert_batch_if_new(batch),
        }
        .map_err(Into::into)
    }
}

/// Reversible version of [`init_resource`](bevy::ecs::system::command::init_resource).
#[track_caller]
pub fn rev_init_resource<R: Resource + FromWorld>() -> impl Command {
    |world: &mut World| {
        world.rev_init_resource::<R>();
    }
}

/// Reversible version of [`insert_resource`](bevy::ecs::system::command::insert_resource).
#[track_caller]
pub fn rev_insert_resource<R: Resource>(resource: R) -> impl Command {
    |world: &mut World| {
        world.rev_insert_resource(resource);
    }
}

/// Reversible version of [`remove_resource`](bevy::ecs::system::command::remove_resource).
pub fn rev_remove_resource<R: Resource>() -> impl Command {
    |world: &mut World| {
        world.rev_remove_resource::<R, _>(|_| ());
    }
}
