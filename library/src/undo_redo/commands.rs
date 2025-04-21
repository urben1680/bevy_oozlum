use bevy::ecs::{
    archetype::ArchetypeId,
    bundle::{Bundle, DynamicBundle, InsertMode, NoBundleEffect},
    entity::Entity,
    error::{ErrorContext, Result as CommandResult, warn},
    resource::Resource,
    system::{Command, Commands, EntityCommands, command::insert_batch},
    world::{FromWorld, World, error::TryInsertBatchError},
};

use crate::meta::RevMeta;

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
        let mut entity_commands = self.spawn(bundle);
        let entity = entity_commands.id();
        entity_commands
            .commands_mut()
            .queue(move |world: &mut World| {
                let meta = world
                    .get_resource::<RevMeta>()
                    .expect(RevMeta::EXPECT_IN_WORLD);
                let marker = DespawnAtOutOfLog::new(meta);
                world.buffer_undo_redo(Spawn { entity, marker });
            });
        entity_commands
    }

    fn rev_spawn_empty(&mut self) -> EntityCommands {
        let mut entity_commands = self.spawn_empty();
        let entity = entity_commands.id();
        entity_commands
            .commands_mut()
            .queue(move |world: &mut World| {
                let meta = world
                    .get_resource::<RevMeta>()
                    .expect(RevMeta::EXPECT_IN_WORLD);
                let marker = DespawnAtOutOfLog::new(meta);
                world.buffer_undo_redo(Spawn { entity, marker });
            });
        entity_commands
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
            if let Err(error) = rev_insert_batch(batch, InsertMode::Replace).apply(world) {
                let name = type_name_of_val(&Commands::rev_try_insert_batch_if_new::<I, B>).into();
                warn(error, ErrorContext::Command { name });
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
            if let Err(error) = rev_insert_batch(batch, InsertMode::Keep).apply(world) {
                let name = type_name_of_val(&Commands::rev_try_insert_batch_if_new::<I, B>).into();
                warn(error, ErrorContext::Command { name });
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
        let batch: Vec<_> = batch.into_iter().collect();
        let mut entity_archetype = Vec::with_capacity(batch.len());
        let mut iter = batch.iter().map(|&(entity, _)| entity);
        for entity in &mut iter {
            let archetype_id = world
                .entities()
                .get(entity)
                .map(|location| location.archetype_id);
            match archetype_id {
                None | Some(ArchetypeId::INVALID) => {
                    let entities = [entity]
                        .into_iter()
                        .chain(iter.filter(|entity| {
                            world.entities().get(*entity).is_none_or(|location| {
                                location.archetype_id == ArchetypeId::INVALID
                            })
                        }))
                        .collect();
                    let err = TryInsertBatchError {
                        bundle_type: core::any::type_name::<B>(),
                        entities,
                    };
                    return Err(err.into());
                }
                Some(archetype_id) => entity_archetype.push((entity, archetype_id)),
            }
        }
        for (entity, archetype_id) in entity_archetype {
            pre_insert::<B>(world, entity, archetype_id, insert_mode);
        }
        insert_batch(batch, insert_mode).apply(world)
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
    |world: &mut World| world.rev_remove_resource::<R>()
}

#[cfg(test)]
mod test {
    use crate::{prelude::UndoRedoBuffer, undo_redo::ResourceSwap};

    use super::*;

    #[derive(Resource, PartialEq, Debug, Default)]
    struct TestRes(u8);

    #[test]
    fn rev_init_resource_works() {
        let mut world = World::new();
        world.init_resource::<UndoRedoBuffer>();

        rev_init_resource::<TestRes>().apply(&mut world);
        let mut undo_redo = world
            .resource_mut::<UndoRedoBuffer>()
            .pop_assert_type::<ResourceSwap<TestRes>>();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(0)));
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(0)));

        rev_init_resource::<TestRes>().apply(&mut world);
        assert!(world.resource::<UndoRedoBuffer>().is_empty());
    }

    #[test]
    fn rev_insert_resource_works() {
        let mut world = World::new();
        world.init_resource::<UndoRedoBuffer>();

        rev_insert_resource(TestRes(10)).apply(&mut world);
        let mut undo_redo = world
            .resource_mut::<UndoRedoBuffer>()
            .pop_assert_type::<ResourceSwap<TestRes>>();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));

        rev_insert_resource(TestRes(20)).apply(&mut world);
        let mut undo_redo = world
            .resource_mut::<UndoRedoBuffer>()
            .pop_assert_type::<ResourceSwap<TestRes>>();

        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(20)));
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(20)));
    }

    #[test]
    fn rev_remove_resource_works() {
        let mut world = World::new();
        world.init_resource::<UndoRedoBuffer>();

        world.insert_resource(TestRes(10));
        rev_remove_resource::<TestRes>().apply(&mut world);
        let mut undo_redo = world
            .resource_mut::<UndoRedoBuffer>()
            .pop_assert_type::<ResourceSwap<TestRes>>();

        assert_eq!(world.get_resource::<TestRes>(), None);
        undo_redo.undo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), Some(&TestRes(10)));
        undo_redo.redo(&mut world);
        assert_eq!(world.get_resource::<TestRes>(), None);

        rev_remove_resource::<TestRes>().apply(&mut world);
        assert!(world.resource::<UndoRedoBuffer>().is_empty());
    }
}
