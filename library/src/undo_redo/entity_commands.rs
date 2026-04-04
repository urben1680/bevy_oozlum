use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::Component,
    entity::Entity,
    hierarchy::{ChildOf, Children},
    relationship::{RelatedSpawnerCommands, Relationship, RelationshipTarget},
    system::{EntityCommand, EntityCommands, EntityEntryCommands},
    world::{EntityWorldMut, FromWorld, World},
};

use crate::{
    meta::NotLog,
    undo_redo::{
        EntityRevDespawnedError, RevBundle, RevEntityWorld, RevWorld, UndoRedo,
        commands::RevCommands, relationship::SlimRelationship,
    },
};

/// Extension trait for [`EntityCommands`] with reversible variants of various methods.
pub trait RevEntityCommands<'w> {
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
    /// commands.queue_undo_redo(not_log, |_: &mut World, direction| {
    ///     match direction {
    ///         UndoRedoDirection::Undo => println!("!dlrow olleh (log)"),
    ///         UndoRedoDirection::Redo => println!("hello world! (log)"),
    ///     }
    /// });
    /// # }
    /// ```
    #[track_caller]
    fn queue_undo_redo(&mut self, not_log: NotLog, undo_redo: impl UndoRedo) -> &mut Self {
        self.queue_undo_redo_with_caller(not_log, undo_redo, MaybeLocation::caller())
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
    fn redo_and_queue(&mut self, not_log: NotLog, undo_redo: impl UndoRedo) -> &mut Self {
        self.redo_and_queue_with_caller(not_log, undo_redo, MaybeLocation::caller())
    }

    /// As [`queue_undo_redo`](Self::queue_undo_redo) but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`run_rev_update`](crate::schedule::run_rev_update) may return the relevant error in that case.
    fn queue_undo_redo_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self;

    /// As [`redo_and_queue`](Self::redo_and_queue) but with explicit [`MaybeLocation`].
    ///
    /// The location can be helpful for identifying non-reversible systems using reversible API.
    /// [`run_rev_update`](crate::schedule::run_rev_update) may return the relevant error in that case.
    fn redo_and_queue_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self;

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`Commands::rev_spawn`](RevCommands::rev_spawn).
    ///
    /// When possible, use `Commands::rev_spawn` instead.
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned(&mut self, not_log: NotLog, include_unlinked_related: bool) -> &mut Self;

    /// Reversible version of [`EntityCommands::despawn`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn(&mut self, not_log: NotLog);

    /// Reversible version of [`EntityCommands::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        bundle: impl Bundle,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::with_child`].
    fn rev_with_child(&mut self, not_log: NotLog, bundle: impl Bundle) -> &mut Self;

    // fn rev_with_related_entities not needed: use RevRelatedSpawnerCommands

    // fn rev_with_children not needed: use RevRelatedSpawnerCommands

    /// Reversible version of [`EntityCommands::add_related`].
    fn rev_add_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_children`].
    fn rev_add_children(
        &mut self,
        not_log: NotLog,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_one_related`].
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        entity: Entity,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_child`].
    fn rev_add_child(&mut self, not_log: NotLog, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_all_related`].
    fn rev_detach_all_related<R: Relationship>(&mut self, not_log: NotLog) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_all_children`].
    fn rev_detach_all_children(&mut self, not_log: NotLog) -> &mut Self;

    /// Reversible version of [`EntityCommands::remove_related`].
    fn rev_remove_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_children`].
    fn rev_detach_children(
        &mut self,
        not_log: NotLog,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_child`].
    fn rev_detach_child(&mut self, not_log: NotLog, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityCommands::replace_related`].
    fn rev_replace_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::replace_children`].
    fn rev_replace_children(
        &mut self,
        not_log: NotLog,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::despawn_related`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, not_log: NotLog) -> &mut Self;

    /// Reversible version of [`EntityCommands::despawn_children`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_children(&mut self, not_log: NotLog) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert`].
    fn rev_insert<Marker>(&mut self, not_log: NotLog, bundle: impl RevBundle<Marker>) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert_if`].
    fn rev_insert_if<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert_if_new`].
    fn rev_insert_if_new<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert_if_new_and`].
    fn rev_insert_if_new_and<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::remove`]. Let the second generic be inferred as `_`.
    fn rev_remove<B: RevBundle<Marker>, Marker>(&mut self, not_log: NotLog) -> &mut Self;

    /// Reversible version of [`EntityCommands::remove_if`]. Let the second generic be inferred as
    /// `_`.
    fn rev_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_despawn`].
    fn rev_try_despawn(&mut self, not_log: NotLog);

    /// Reversible version of [`EntityCommands::try_insert`].
    fn rev_try_insert<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_insert_if`].
    fn rev_try_insert_if<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_insert_if_new`].
    fn rev_try_insert_if_new<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_insert_if_new_and`].
    fn rev_try_insert_if_new_and<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_remove`]. Let the second generic be inferred as
    /// `_`.
    fn rev_try_remove<B: RevBundle<Marker>, Marker>(&mut self, not_log: NotLog) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_remove_if`]..Let the second generic be inferred
    /// as `_`.
    fn rev_try_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;
}

impl<'a> RevEntityCommands<'a> for EntityCommands<'a> {
    fn queue_undo_redo_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.commands_mut().queue(move |world: &mut World| {
            world.queue_undo_redo(not_log, undo_redo, caller);
        });
        self
    }

    fn redo_and_queue_with_caller(
        &mut self,
        not_log: NotLog,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.commands_mut().queue(move |world: &mut World| {
            world.redo_and_queue(not_log, undo_redo, caller);
        });
        self
    }

    #[track_caller]
    fn rev_mark_spawned(&mut self, not_log: NotLog, include_unlinked_related: bool) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_mark_spawned(not_log, include_unlinked_related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_despawn(&mut self, not_log: NotLog) {
        let caller = MaybeLocation::caller();
        self.queue(move |entity_world_mut: EntityWorldMut| {
            entity_world_mut.rev_despawn(not_log, caller).map(|_| ())
        });
    }

    #[track_caller]
    fn rev_with_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        bundle: impl Bundle,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_with_related::<R>(not_log, bundle, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_with_child(&mut self, not_log: NotLog, bundle: impl Bundle) -> &mut Self {
        self.rev_with_related::<ChildOf>(not_log, bundle)
    }

    #[track_caller]
    fn rev_add_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_related::<R>(not_log, related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_add_children(
        &mut self,
        not_log: NotLog,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        self.rev_add_related::<ChildOf>(not_log, children)
    }

    #[track_caller]
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        entity: Entity,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_one_related::<R>(not_log, entity, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_add_child(&mut self, not_log: NotLog, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(not_log, child)
    }

    #[track_caller]
    fn rev_detach_all_related<R: Relationship>(&mut self, not_log: NotLog) -> &mut Self {
        #[allow(clippy::let_unit_value)]
        let _ = R::ASSERT; // may contain non-default extra data
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_detach_all_related::<R>(not_log, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_detach_all_children(&mut self, not_log: NotLog) -> &mut Self {
        self.rev_detach_all_related::<ChildOf>(not_log)
    }

    #[track_caller]
    fn rev_remove_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        #[allow(clippy::let_unit_value)]
        let _ = R::ASSERT; // may contain non-default extra data
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_remove_related::<R>(not_log, related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_detach_children(
        &mut self,
        not_log: NotLog,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        self.rev_remove_related::<ChildOf>(not_log, children)
    }

    #[track_caller]
    fn rev_detach_child(&mut self, not_log: NotLog, child: Entity) -> &mut Self {
        self.rev_remove_related::<ChildOf>(not_log, [child])
    }

    #[track_caller]
    fn rev_replace_related<R: Relationship>(
        &mut self,
        not_log: NotLog,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        #[allow(clippy::let_unit_value)]
        let _ = R::ASSERT; // may contain non-default extra data
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_replace_related::<R>(not_log, related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_replace_children(
        &mut self,
        not_log: NotLog,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        self.rev_replace_related::<ChildOf>(not_log, children)
    }

    #[track_caller]
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, not_log: NotLog) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_despawn_related::<S>(not_log, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_despawn_children(&mut self, not_log: NotLog) -> &mut Self {
        self.rev_despawn_related::<Children>(not_log)
    }

    #[track_caller]
    fn rev_insert<Marker>(&mut self, not_log: NotLog, bundle: impl RevBundle<Marker>) -> &mut Self {
        self.queue(rev_insert(not_log, bundle, InsertMode::Replace))
    }

    #[track_caller]
    fn rev_insert_if<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_insert(not_log, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_insert_if_new<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue(rev_insert(not_log, bundle, InsertMode::Keep))
    }

    #[track_caller]
    fn rev_insert_if_new_and<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_insert_if_new(not_log, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_remove<B: RevBundle<Marker>, Marker>(&mut self, not_log: NotLog) -> &mut Self {
        self.queue(rev_remove::<B, _>(not_log))
    }

    #[track_caller]
    fn rev_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_remove::<B, _>(not_log)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, not_log: NotLog) {
        self.queue_silenced(rev_despawn(not_log));
    }

    #[track_caller]
    fn rev_try_insert<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue_silenced(rev_insert(not_log, bundle, InsertMode::Replace))
    }

    #[track_caller]
    fn rev_try_insert_if<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_insert(not_log, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_insert_if_new<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue_silenced(rev_insert(not_log, bundle, InsertMode::Keep))
    }

    #[track_caller]
    fn rev_try_insert_if_new_and<Marker>(
        &mut self,
        not_log: NotLog,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_insert_if_new(not_log, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_remove<B: RevBundle<Marker>, Marker>(&mut self, not_log: NotLog) -> &mut Self {
        self.queue_silenced(rev_remove::<B, _>(not_log))
    }

    #[track_caller]
    fn rev_try_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        not_log: NotLog,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_remove::<B, _>(not_log)
        } else {
            self
        }
    }
}

/// Extension trait for [`EntityEntryCommands`] with reversible variants of various methods.
pub trait RevEntityEntryCommands<T: Component> {
    /// Reversible version of [`EntityEntryCommands::or_default`].
    fn rev_or_default(&mut self, not_log: NotLog) -> &mut Self
    where
        T: Default;

    /// Reversible version of [`EntityEntryCommands::or_from_world`].
    fn rev_or_from_world(&mut self, not_log: NotLog) -> &mut Self
    where
        T: FromWorld;

    /// Reversible version of [`EntityEntryCommands::or_insert`].
    fn rev_or_insert(&mut self, not_log: NotLog, default: T) -> &mut Self;

    /// Reversible version of [`EntityEntryCommands::or_insert_with`].
    fn rev_or_insert_with(&mut self, not_log: NotLog, default: impl Fn() -> T) -> &mut Self;

    /// Reversible version of [`EntityEntryCommands::or_try_insert`].
    fn rev_or_try_insert(&mut self, not_log: NotLog, default: T) -> &mut Self;

    /// Reversible version of [`EntityEntryCommands::or_try_insert_with`].
    fn rev_or_try_insert_with(&mut self, not_log: NotLog, default: impl Fn() -> T) -> &mut Self;
}

impl<T: Component> RevEntityEntryCommands<T> for EntityEntryCommands<'_, T> {
    #[track_caller]
    fn rev_or_default(&mut self, not_log: NotLog) -> &mut Self
    where
        T: Default,
    {
        self.rev_or_insert(not_log, T::default())
    }

    #[track_caller]
    fn rev_or_from_world(&mut self, not_log: NotLog) -> &mut Self
    where
        T: FromWorld,
    {
        self.entity()
            .queue(rev_insert_from_world::<T>(not_log, InsertMode::Keep));
        self
    }

    #[track_caller]
    fn rev_or_insert(&mut self, not_log: NotLog, default: T) -> &mut Self {
        self.entity().rev_insert_if_new(not_log, default);
        self
    }

    #[track_caller]
    fn rev_or_insert_with(&mut self, not_log: NotLog, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_insert(not_log, default())
    }

    #[track_caller]
    fn rev_or_try_insert(&mut self, not_log: NotLog, default: T) -> &mut Self {
        self.entity().rev_try_insert_if_new(not_log, default);
        self
    }

    #[track_caller]
    fn rev_or_try_insert_with(&mut self, not_log: NotLog, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_try_insert(not_log, default())
    }
}

/// Extension trait for [`RelatedSpawnerCommands`] with reversible variants of various methods.
pub trait RevRelatedSpawnerCommands {
    /// Reversible version of [`RelatedSpawnerCommands::spawn`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn(&mut self, not_log: NotLog, bundle: impl Bundle) -> EntityCommands<'_>;

    /// Reversible version of [`RelatedSpawnerCommands::spawn_empty`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn_empty(&mut self, not_log: NotLog) -> EntityCommands<'_>;
}

impl<R: Relationship> RevRelatedSpawnerCommands for RelatedSpawnerCommands<'_, R> {
    #[track_caller]
    fn rev_spawn(&mut self, not_log: NotLog, bundle: impl Bundle) -> EntityCommands<'_> {
        let target = self.target_entity();
        self.commands_mut()
            .rev_spawn(not_log, (R::from(target), bundle))
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, not_log: NotLog) -> EntityCommands<'_> {
        let target = self.target_entity();
        self.commands_mut().rev_spawn(not_log, R::from(target))
    }
}

type CmdOut = Result<(), EntityRevDespawnedError>;

/// Reversible version of [`insert`](bevy_ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<Marker>(
    not_log: NotLog,
    bundle: impl RevBundle<Marker>,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        entity_mut.assert_not_rev_despawned()?;
        bundle.rev_insert(not_log, &mut entity_mut, mode, caller);
        Ok(())
    }
}

/// Reversible version of [`insert_from_world`](bevy_ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(
    not_log: NotLog,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        if !(mode == InsertMode::Keep && entity_mut.contains::<T>()) {
            let value = entity_mut.world_scope(|world| T::from_world(world));
            entity_mut.rev_insert(not_log, value, caller).map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`insert_with`](bevy_ecs::system::entity_command::insert_with).
#[track_caller]
pub fn rev_insert_with<T: Component, F>(
    not_log: NotLog,
    component_fn: F,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut>
where
    F: FnOnce() -> T + Send + 'static,
{
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        if !(mode == InsertMode::Keep && entity_mut.contains::<T>()) {
            let value = component_fn();
            entity_mut.rev_insert(not_log, value, caller).map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`remove`](bevy_ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<T: RevBundle<Marker>, Marker>(not_log: NotLog) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        entity_mut
            .rev_remove::<T, Marker>(not_log, caller)
            .map(|_| ())
    }
}

/// Reversible version of [`despawn`](bevy_ecs::system::entity_command::despawn).
///
/// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
/// reversible spawn/despawn.
#[track_caller]
pub fn rev_despawn(not_log: NotLog) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |entity_mut: EntityWorldMut| entity_mut.rev_despawn(not_log, caller)
}
