use crate::undo_redo::{
    EntityRevDespawnedError, RevBundle, RevCommands, RevEntityWorldMutInternal,
};
use crate::{meta::MetaPastLen, undo_redo::UndoRedo};
use bevy_ecs::entity::Entity;
use bevy_ecs::relationship::{RelatedSpawnerCommands, Relationship, RelationshipTarget};
use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::Component,
    system::{EntityCommand, EntityCommands, EntityEntryCommands},
    world::{EntityWorldMut, FromWorld},
};

/// Extension trait for [`EntityCommands`] with reversible variants of various methods.
pub trait RevEntityCommands<'w> {
    /// Shorthand method of [`Commands::buffer_undo_redo`](BuffersUndoRedo::buffer_undo_redo) with
    /// applying `undo_redo.redo(&mut self)` immediately. Useful when there is no difference between
    /// doing and redoing.
    fn redo_and_buffer(&mut self, meta_past_len: MetaPastLen, undo_redo: impl UndoRedo);

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with [`Commands::rev_spawn`](super::RevCommands::rev_spawn).
    ///
    /// When possible, use `Commands::rev_spawn` instead.
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        include_unlinked_related: bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::despawn`].
    fn rev_despawn(&mut self, meta_past_len: MetaPastLen);

    /// Reversible version of [`EntityCommands::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::with_child`].
    fn rev_with_child(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_related`].
    fn rev_add_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_children`].
    fn rev_add_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_one_related`].
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::add_child`].
    fn rev_add_child(&mut self, meta_past_len: MetaPastLen, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_all_related`].
    fn rev_detach_all_related<R: Relationship>(&mut self, meta_past_len: MetaPastLen) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_all_children`].
    fn rev_detach_all_children(&mut self, meta_past_len: MetaPastLen) -> &mut Self;

    /// Reversible version of [`EntityCommands::remove_related`].
    fn rev_remove_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_children`].
    fn rev_detach_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::detach_child`].
    fn rev_detach_child(&mut self, meta_past_len: MetaPastLen, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityCommands::replace_related`].
    fn rev_replace_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::replace_children`].
    fn rev_replace_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::despawn_related`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_related<S: RelationshipTarget>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::despawn_children`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_despawn_children(&mut self, meta_past_len: MetaPastLen) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert`].
    fn rev_insert<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert_if`].
    fn rev_insert_if<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert_if_new`].
    fn rev_insert_if_new<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::insert_if_new_and`].
    fn rev_insert_if_new_and<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::remove`]. Let the second generic be inferred as `_`.
    fn rev_remove<B: RevBundle<Marker>, Marker>(&mut self, meta_past_len: MetaPastLen)
    -> &mut Self;

    /// Reversible version of [`EntityCommands::remove_if`]. Let the second generic be inferred as
    /// `_`.
    fn rev_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_despawn`].
    fn rev_try_despawn(&mut self, meta_past_len: MetaPastLen);

    /// Reversible version of [`EntityCommands::try_insert`].
    fn rev_try_insert<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_insert_if`].
    fn rev_try_insert_if<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_insert_if_new`].
    fn rev_try_insert_if_new<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_insert_if_new_and`].
    fn rev_try_insert_if_new_and<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_remove`]. Let the second generic be inferred as
    /// `_`.
    fn rev_try_remove<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self;

    /// Reversible version of [`EntityCommands::try_remove_if`]..Let the second generic be inferred
    /// as `_`.
    fn rev_try_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self;
}

impl<'a> RevEntityCommands<'a> for EntityCommands<'a> {
    fn redo_and_buffer(&mut self, meta_past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.commands_mut()
            .redo_and_buffer(meta_past_len, undo_redo);
    }

    #[track_caller]
    fn rev_mark_spawned(
        &mut self,
        meta_past_len: MetaPastLen,
        include_unlinked_related: bool,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_mark_spawned_with_caller(meta_past_len, include_unlinked_related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_despawn(&mut self, meta_past_len: MetaPastLen) {
        let caller = MaybeLocation::caller();
        self.queue(move |entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_despawn_with_caller(meta_past_len, caller)
                .map(|_| ())
        });
    }

    #[track_caller]
    fn rev_with_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_with_related_with_caller::<R>(meta_past_len, bundle, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_with_child(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_with_child_with_caller(meta_past_len, bundle, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_add_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_related_with_caller::<R>(meta_past_len, related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_add_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_children_with_caller(meta_past_len, children, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        entity: Entity,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_one_related_with_caller::<R>(meta_past_len, entity, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_add_child(&mut self, meta_past_len: MetaPastLen, child: Entity) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_child_with_caller(meta_past_len, child, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_detach_all_related<R: Relationship>(&mut self, meta_past_len: MetaPastLen) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_detach_all_related_with_caller::<R>(meta_past_len, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_detach_all_children(&mut self, meta_past_len: MetaPastLen) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_detach_all_children_with_caller(meta_past_len, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_remove_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_remove_related_with_caller::<R>(meta_past_len, related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_detach_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_detach_children_with_caller(meta_past_len, children, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_detach_child(&mut self, meta_past_len: MetaPastLen, child: Entity) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_detach_child_with_caller(meta_past_len, child, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_replace_related<R: Relationship>(
        &mut self,
        meta_past_len: MetaPastLen,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_replace_related_with_caller::<R>(meta_past_len, related, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_replace_children(
        &mut self,
        meta_past_len: MetaPastLen,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_replace_children_witch_caller(meta_past_len, children, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_despawn_related<S: RelationshipTarget>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_despawn_related_with_caller::<S>(meta_past_len, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_despawn_children(&mut self, meta_past_len: MetaPastLen) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_despawn_children_with_caller(meta_past_len, caller)
                .map(|_| ())
        })
    }

    #[track_caller]
    fn rev_insert<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue(rev_insert(meta_past_len, bundle, InsertMode::Replace))
    }

    #[track_caller]
    fn rev_insert_if<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_insert(meta_past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_insert_if_new<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue(rev_insert(meta_past_len, bundle, InsertMode::Keep))
    }

    #[track_caller]
    fn rev_insert_if_new_and<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_insert_if_new(meta_past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_remove<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self {
        self.queue(rev_remove::<B, _>(meta_past_len))
    }

    #[track_caller]
    fn rev_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_remove::<B, _>(meta_past_len)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_despawn(&mut self, meta_past_len: MetaPastLen) {
        self.queue_silenced(rev_despawn(meta_past_len));
    }

    #[track_caller]
    fn rev_try_insert<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue_silenced(rev_insert(meta_past_len, bundle, InsertMode::Replace))
    }

    #[track_caller]
    fn rev_try_insert_if<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_insert(meta_past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_insert_if_new<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
    ) -> &mut Self {
        self.queue_silenced(rev_insert(meta_past_len, bundle, InsertMode::Keep))
    }

    #[track_caller]
    fn rev_try_insert_if_new_and<Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_insert_if_new(meta_past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_remove<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
    ) -> &mut Self {
        self.queue_silenced(rev_remove::<B, _>(meta_past_len))
    }

    #[track_caller]
    fn rev_try_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        meta_past_len: MetaPastLen,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_remove::<B, _>(meta_past_len)
        } else {
            self
        }
    }
}

/// Extension trait for [`EntityEntryCommands`] with reversible variants of various methods.
pub trait RevEntityEntryCommands<T: Component> {
    /// Reversible version of [`EntityEntryCommands::or_default`].
    fn rev_or_default(&mut self, meta_past_len: MetaPastLen) -> &mut Self
    where
        T: Default;

    /// Reversible version of [`EntityEntryCommands::or_from_world`].
    fn rev_or_from_world(&mut self, meta_past_len: MetaPastLen) -> &mut Self
    where
        T: FromWorld;

    /// Reversible version of [`EntityEntryCommands::or_insert`].
    fn rev_or_insert(&mut self, meta_past_len: MetaPastLen, default: T) -> &mut Self;

    /// Reversible version of [`EntityEntryCommands::or_insert_with`].
    fn rev_or_insert_with(
        &mut self,
        meta_past_len: MetaPastLen,
        default: impl Fn() -> T,
    ) -> &mut Self;

    /// Reversible version of [`EntityEntryCommands::or_try_insert`].
    fn rev_or_try_insert(&mut self, meta_past_len: MetaPastLen, default: T) -> &mut Self;

    /// Reversible version of [`EntityEntryCommands::or_try_insert_with`].
    fn rev_or_try_insert_with(
        &mut self,
        meta_past_len: MetaPastLen,
        default: impl Fn() -> T,
    ) -> &mut Self;
}

impl<T: Component> RevEntityEntryCommands<T> for EntityEntryCommands<'_, T> {
    #[track_caller]
    fn rev_or_default(&mut self, meta_past_len: MetaPastLen) -> &mut Self
    where
        T: Default,
    {
        self.rev_or_insert(meta_past_len, T::default())
    }

    #[track_caller]
    fn rev_or_from_world(&mut self, meta_past_len: MetaPastLen) -> &mut Self
    where
        T: FromWorld,
    {
        self.entity()
            .queue(rev_insert_from_world::<T>(meta_past_len, InsertMode::Keep));
        self
    }

    #[track_caller]
    fn rev_or_insert(&mut self, meta_past_len: MetaPastLen, default: T) -> &mut Self {
        self.entity().rev_insert_if_new(meta_past_len, default);
        self
    }

    #[track_caller]
    fn rev_or_insert_with(
        &mut self,
        meta_past_len: MetaPastLen,
        default: impl Fn() -> T,
    ) -> &mut Self {
        self.rev_or_insert(meta_past_len, default())
    }

    #[track_caller]
    fn rev_or_try_insert(&mut self, meta_past_len: MetaPastLen, default: T) -> &mut Self {
        self.entity().rev_try_insert_if_new(meta_past_len, default);
        self
    }

    #[track_caller]
    fn rev_or_try_insert_with(
        &mut self,
        meta_past_len: MetaPastLen,
        default: impl Fn() -> T,
    ) -> &mut Self {
        self.rev_or_try_insert(meta_past_len, default())
    }
}

/// Extension trait for [`RelatedSpawnerCommands`] with reversible variants of various methods.
pub trait RevRelatedSpawnerCommands {
    /// Reversible version of [`RelatedSpawnerCommands::spawn`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> EntityCommands<'_>;

    /// Reversible version of [`RelatedSpawnerCommands::spawn_empty`].
    ///
    /// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
    /// reversible spawn/despawn.
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityCommands<'_>;
}

impl<R: Relationship> RevRelatedSpawnerCommands for RelatedSpawnerCommands<'_, R> {
    #[track_caller]
    fn rev_spawn(&mut self, meta_past_len: MetaPastLen, bundle: impl Bundle) -> EntityCommands<'_> {
        let target = self.target_entity();
        self.commands_mut()
            .rev_spawn(meta_past_len, (R::from(target), bundle))
    }

    #[track_caller]
    fn rev_spawn_empty(&mut self, meta_past_len: MetaPastLen) -> EntityCommands<'_> {
        let target = self.target_entity();
        self.commands_mut()
            .rev_spawn(meta_past_len, R::from(target))
    }
}

type CmdOut = Result<(), EntityRevDespawnedError>;

/// Reversible version of [`insert`](bevy_ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<Marker>(
    meta_past_len: MetaPastLen,
    bundle: impl RevBundle<Marker>,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        entity_mut.assert_not_rev_despawned()?;
        bundle.rev_insert(meta_past_len, &mut entity_mut, mode, caller);
        Ok(())
    }
}

/// Reversible version of [`insert_from_world`](bevy_ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(
    meta_past_len: MetaPastLen,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        if !(mode == InsertMode::Keep && entity_mut.contains::<T>()) {
            let value = entity_mut.world_scope(|world| T::from_world(world));
            entity_mut
                .rev_insert_with_caller(meta_past_len, value, caller)
                .map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`insert_with`](bevy_ecs::system::entity_command::insert_with).
#[track_caller]
pub fn rev_insert_with<T: Component, F>(
    meta_past_len: MetaPastLen,
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
            entity_mut
                .rev_insert_with_caller(meta_past_len, value, caller)
                .map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`remove`](bevy_ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<T: RevBundle<Marker>, Marker>(
    meta_past_len: MetaPastLen,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        entity_mut
            .rev_remove_with_caller::<T, Marker>(meta_past_len, caller)
            .map(|_| ())
    }
}

/// Reversible version of [`despawn`](bevy_ecs::system::entity_command::despawn).
///
/// See the [`RevDespawned`](super::RevDespawned) documentation to understand the mechanics of
/// reversible spawn/despawn.
#[track_caller]
pub fn rev_despawn(meta_past_len: MetaPastLen) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |entity_mut: EntityWorldMut| entity_mut.rev_despawn_with_caller(meta_past_len, caller)
}
