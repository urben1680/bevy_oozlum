use core::ops::{Deref, DerefMut};

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
        AsRev, EntityRevDespawnedError, RevBundle, RevEntityWorld, RevWorld, UndoRedo,
        commands::rev_spawn_inner, relationship::SlimRelationship,
    },
};

type CmdOut = Result<(), EntityRevDespawnedError>;

pub struct RevEntityCommands<'a>(pub(super) EntityCommands<'a>);

impl<'a> From<RevEntityCommands<'a>> for EntityCommands<'a> {
    fn from(value: RevEntityCommands<'a>) -> Self {
        value.0
    }
}

impl<'a> Deref for RevEntityCommands<'a> {
    type Target = EntityCommands<'a>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for RevEntityCommands<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl AsRev for EntityCommands<'_> {
    type Out<'a>
        = RevEntityCommands<'a>
    where
        Self: 'a;
    fn as_rev(&mut self, not_log: NotLog) -> Self::Out<'_> {
        RevEntityCommands::new(not_log, self)
    }
}

impl<'a> RevEntityCommands<'a> {
    /// Construct `RevEntityCommands` during [`RevDirection::NotLog`](super::RevDirection::NotLog).
    pub fn new(_: NotLog, commands: &'a mut EntityCommands) -> Self {
        Self(commands.reborrow())
    }

    /// Returns a [`RevEntityCommands`] with a smaller lifetime.
    ///
    /// This is useful if you have `&mut RevEntityCommands` but need `RevEntityCommands`.
    pub fn reborrow(&mut self) -> RevEntityCommands<'_> {
        RevEntityCommands(self.0.reborrow())
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
    #[track_caller]
    pub fn queue_undo_redo_with_caller(
        &mut self,
        undo_redo: impl UndoRedo,
        caller: MaybeLocation,
    ) -> &mut Self {
        self.commands_mut().queue(move |world: &mut World| {
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
        self.commands_mut().queue(move |world: &mut World| {
            world.redo_and_queue(undo_redo, caller);
        });
        self
    }

    /// Reversible version of [`EntityCommands::entry`].
    pub fn rev_entry<T: Component>(&mut self) -> RevEntityEntryCommands<'_, T> {
        RevEntityEntryCommands(self.0.entry::<T>())
    }

    /// Reversible version of [`EntityCommands::with_related_entities`].
    pub fn rev_with_related_entities<R: Relationship>(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawnerCommands<R>),
    ) -> &mut Self {
        let id = self.id();
        func(&mut RevRelatedSpawnerCommands(RelatedSpawnerCommands::new(
            self.commands(),
            id,
        )));
        self
    }

    /// Reversible version of [`EntityCommands::with_children`].
    pub fn rev_with_children(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawnerCommands<ChildOf>),
    ) -> &mut Self {
        self.rev_with_related_entities(func);
        self
    }

    /// Helper method to mark an entity as reversibly spawned. Useful when the actual spawn is
    /// hidden and cannot be done with
    /// [`RevCommands::rev_spawn`](super::commands::RevCommands::rev_spawn).
    ///
    /// When possible, use `Commands::rev_spawn` instead.
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_mark_spawned(&mut self, include_unlinked_related: bool) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_mark_spawned(include_unlinked_related, caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::despawn`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_despawn(&mut self) {
        let caller = MaybeLocation::caller();
        self.queue(move |entity_world_mut: EntityWorldMut| {
            entity_world_mut.rev_despawn(caller).map(|_| ())
        });
    }

    /// Reversible version of [`EntityCommands::with_related`].
    #[track_caller]
    pub fn rev_with_related<R: Relationship>(&mut self, bundle: impl Bundle) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_with_related::<R>(bundle, caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::with_child`].
    #[track_caller]
    pub fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self {
        self.rev_with_related::<ChildOf>(bundle)
    }

    /// Reversible version of [`EntityCommands::add_related`].
    #[track_caller]
    pub fn rev_add_related<R: Relationship>(
        &mut self,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_related::<R>(related, caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::add_children`].
    #[track_caller]
    pub fn rev_add_children(
        &mut self,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        self.rev_add_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityCommands::add_one_related`].
    #[track_caller]
    pub fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_add_one_related::<R>(entity, caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::add_child`].
    #[track_caller]
    pub fn rev_add_child(&mut self, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(child)
    }

    /// Reversible version of [`EntityCommands::detach_all_related`].
    #[track_caller]
    pub fn rev_detach_all_related<R: Relationship>(&mut self) -> &mut Self {
        #[allow(clippy::let_unit_value)]
        let _ = R::ASSERT; // may contain non-default extra data
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_detach_all_related::<R>(caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::detach_all_children`].
    #[track_caller]
    pub fn rev_detach_all_children(&mut self) -> &mut Self {
        self.rev_detach_all_related::<ChildOf>()
    }

    /// Reversible version of [`EntityCommands::remove_related`].
    #[track_caller]
    pub fn rev_remove_related<R: Relationship>(
        &mut self,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        #[allow(clippy::let_unit_value)]
        let _ = R::ASSERT; // may contain non-default extra data
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_remove_related::<R>(related, caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::detach_children`].
    #[track_caller]
    pub fn rev_detach_children(
        &mut self,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        self.rev_remove_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityCommands::detach_child`].
    #[track_caller]
    pub fn rev_detach_child(&mut self, child: Entity) -> &mut Self {
        self.rev_remove_related::<ChildOf>([child])
    }

    /// Reversible version of [`EntityCommands::replace_related`].
    #[track_caller]
    pub fn rev_replace_related<R: Relationship>(
        &mut self,
        related: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        #[allow(clippy::let_unit_value)]
        let _ = R::ASSERT; // may contain non-default extra data
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_replace_related::<R>(related, caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::replace_children`].
    #[track_caller]
    pub fn rev_replace_children(
        &mut self,
        children: impl AsRef<[Entity]> + Send + 'static,
    ) -> &mut Self {
        self.rev_replace_related::<ChildOf>(children)
    }

    /// Reversible version of [`EntityCommands::despawn_related`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_world_mut: EntityWorldMut| {
            entity_world_mut
                .rev_despawn_related::<S>(caller)
                .map(|_| ())
        });
        self
    }

    /// Reversible version of [`EntityCommands::despawn_children`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_despawn_children(&mut self) -> &mut Self {
        self.rev_despawn_related::<Children>()
    }

    /// Reversible version of [`EntityCommands::insert`].
    #[track_caller]
    pub fn rev_insert<Marker>(&mut self, bundle: impl RevBundle<Marker>) -> &mut Self {
        self.queue(rev_insert_inner(
            bundle,
            InsertMode::Replace,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityCommands::insert_if`].
    #[track_caller]
    pub fn rev_insert_if<Marker>(
        &mut self,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_insert(bundle)
        } else {
            self
        }
    }

    /// Reversible version of [`EntityCommands::insert_if_new`].
    #[track_caller]
    pub fn rev_insert_if_new<Marker>(&mut self, bundle: impl RevBundle<Marker>) -> &mut Self {
        self.queue(rev_insert_inner(
            bundle,
            InsertMode::Keep,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityCommands::insert_if_new_and`].
    #[track_caller]
    pub fn rev_insert_if_new_and<Marker>(
        &mut self,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_insert_if_new(bundle)
        } else {
            self
        }
    }

    /// Reversible version of [`EntityCommands::remove`]. Let the second generic be inferred as `_`.
    #[track_caller]
    pub fn rev_remove<B: RevBundle<Marker>, Marker>(&mut self) -> &mut Self {
        self.queue(rev_remove_inner::<B, _>(MaybeLocation::caller()));
        self
    }

    /// Reversible version of [`EntityCommands::remove_if`]. Let the second generic be inferred as
    /// `_`.
    #[track_caller]
    pub fn rev_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_remove::<B, _>()
        } else {
            self
        }
    }

    /// Reversible version of [`EntityCommands::try_despawn`].
    #[track_caller]
    pub fn rev_try_despawn(&mut self) {
        self.queue_silenced(rev_despawn_inner(MaybeLocation::caller()));
    }

    /// Reversible version of [`EntityCommands::try_insert`].
    #[track_caller]
    pub fn rev_try_insert<Marker>(&mut self, bundle: impl RevBundle<Marker>) -> &mut Self {
        self.queue_silenced(rev_insert_inner(
            bundle,
            InsertMode::Replace,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityCommands::try_insert_if`].
    #[track_caller]
    pub fn rev_try_insert_if<Marker>(
        &mut self,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_insert(bundle)
        } else {
            self
        }
    }

    /// Reversible version of [`EntityCommands::try_insert_if_new`].
    #[track_caller]
    pub fn rev_try_insert_if_new<Marker>(&mut self, bundle: impl RevBundle<Marker>) -> &mut Self {
        self.queue_silenced(rev_insert_inner(
            bundle,
            InsertMode::Keep,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityCommands::try_insert_if_new_and`].
    #[track_caller]
    pub fn rev_try_insert_if_new_and<Marker>(
        &mut self,
        bundle: impl RevBundle<Marker>,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_insert_if_new(bundle)
        } else {
            self
        }
    }

    /// Reversible version of [`EntityCommands::try_remove`]. Let the second generic be inferred as
    /// `_`.
    #[track_caller]
    pub fn rev_try_remove<B: RevBundle<Marker>, Marker>(&mut self) -> &mut Self {
        self.queue_silenced(rev_remove_inner::<B, _>(MaybeLocation::caller()));
        self
    }

    /// Reversible version of [`EntityCommands::try_remove_if`]. Let the second generic be inferred
    /// as `_`.
    #[track_caller]
    pub fn rev_try_remove_if<B: RevBundle<Marker>, Marker>(
        &mut self,
        condition: impl FnOnce() -> bool,
    ) -> &mut Self {
        if condition() {
            self.rev_try_remove::<B, _>()
        } else {
            self
        }
    }
}

pub struct RevEntityEntryCommands<'a, T>(EntityEntryCommands<'a, T>);

impl<'a, T> From<RevEntityEntryCommands<'a, T>> for EntityEntryCommands<'a, T> {
    fn from(value: RevEntityEntryCommands<'a, T>) -> Self {
        value.0
    }
}

impl<'a, T> Deref for RevEntityEntryCommands<'a, T> {
    type Target = EntityEntryCommands<'a, T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for RevEntityEntryCommands<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Component> AsRev for EntityEntryCommands<'_, T> {
    type Out<'a>
        = RevEntityEntryCommands<'a, T>
    where
        Self: 'a;
    fn as_rev(&mut self, not_log: NotLog) -> Self::Out<'_> {
        RevEntityEntryCommands::new(not_log, self)
    }
}

impl<'a, T: Component> RevEntityEntryCommands<'a, T> {
    /// Construct `RevEntityEntryCommands` during
    /// [`RevDirection::NotLog`](super::RevDirection::NotLog).
    pub fn new(_: NotLog, _commands: &'a mut EntityEntryCommands<T>) -> Self {
        todo!() // needs reborrow
    }

    /// Returns a [`RevEntityEntryCommands`] with a smaller lifetime.
    ///
    /// This is useful if you have `&mut RevEntityEntryCommands` but need `RevEntityEntryCommands`.
    pub fn reborrow(&mut self) -> RevEntityEntryCommands<'_, T> {
        todo!() // needs reborrow
    }

    /// Reversible version of [`EntityEntryCommands::or_default`].
    #[track_caller]
    pub fn rev_or_default(&mut self) -> &mut Self
    where
        T: Default,
    {
        self.rev_or_insert(T::default())
    }

    /// Reversible version of [`EntityEntryCommands::or_from_world`].
    #[track_caller]
    pub fn rev_or_from_world(&mut self) -> &mut Self
    where
        T: FromWorld,
    {
        self.entity().queue(rev_insert_from_world_inner::<T>(
            InsertMode::Keep,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityEntryCommands::or_insert`].
    #[track_caller]
    pub fn rev_or_insert(&mut self, default: T) -> &mut Self {
        self.entity().queue(rev_insert_inner(
            default,
            InsertMode::Keep,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityEntryCommands::or_insert_with`].
    #[track_caller]
    pub fn rev_or_insert_with(&mut self, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_insert(default())
    }

    /// Reversible version of [`EntityEntryCommands::or_try_insert`].
    #[track_caller]
    pub fn rev_or_try_insert(&mut self, default: T) -> &mut Self {
        self.entity().queue_silenced(rev_insert_inner(
            default,
            InsertMode::Keep,
            MaybeLocation::caller(),
        ));
        self
    }

    /// Reversible version of [`EntityEntryCommands::or_try_insert_with`].
    #[track_caller]
    pub fn rev_or_try_insert_with(&mut self, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_try_insert(default())
    }
}

pub struct RevRelatedSpawnerCommands<'a, R: Relationship>(RelatedSpawnerCommands<'a, R>);

impl<'a, R: Relationship> From<RevRelatedSpawnerCommands<'a, R>> for RelatedSpawnerCommands<'a, R> {
    fn from(value: RevRelatedSpawnerCommands<'a, R>) -> Self {
        value.0
    }
}

impl<'a, R: Relationship> Deref for RevRelatedSpawnerCommands<'a, R> {
    type Target = RelatedSpawnerCommands<'a, R>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<R: Relationship> DerefMut for RevRelatedSpawnerCommands<'_, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<R: Relationship> AsRev for RelatedSpawnerCommands<'_, R> {
    type Out<'a>
        = RevRelatedSpawnerCommands<'a, R>
    where
        Self: 'a;
    fn as_rev(&mut self, not_log: NotLog) -> Self::Out<'_> {
        RevRelatedSpawnerCommands::new(not_log, self)
    }
}

impl<'a, R: Relationship> RevRelatedSpawnerCommands<'a, R> {
    /// Construct `RevRelatedSpawnerCommands` during
    /// [`RevDirection::NotLog`](super::RevDirection::NotLog).
    pub fn new(_: NotLog, _commands: &'a mut RelatedSpawnerCommands<R>) -> Self {
        todo!() // needs reborrow
    }

    /// Returns a [`RevRelatedSpawnerCommands`] with a smaller lifetime.
    ///
    /// This is useful if you have `&mut RevRelatedSpawnerCommands` but need
    /// `RevRelatedSpawnerCommands`.
    pub fn reborrow(&mut self) -> RevRelatedSpawnerCommands<'_, R> {
        todo!() // needs reborrow
    }

    /// Reversible version of [`RelatedSpawnerCommands::spawn`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_spawn(&mut self, bundle: impl Bundle) -> RevEntityCommands<'_> {
        let target = self.target_entity();
        rev_spawn_inner(
            self.commands_mut(),
            (R::from(target), bundle),
            MaybeLocation::caller(),
        )
    }

    /// Reversible version of [`RelatedSpawnerCommands::spawn_empty`].
    ///
    /// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
    /// reversible spawn/despawn.
    #[track_caller]
    pub fn rev_spawn_empty(&mut self) -> RevEntityCommands<'_> {
        let target = self.target_entity();
        rev_spawn_inner(
            self.commands_mut(),
            R::from(target),
            MaybeLocation::caller(),
        )
    }
}

/// Reversible version of [`insert`](bevy_ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<Marker>(
    _: NotLog,
    bundle: impl RevBundle<Marker>,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    rev_insert_inner(bundle, mode, MaybeLocation::caller())
}

fn rev_insert_inner<Marker>(
    bundle: impl RevBundle<Marker>,
    mode: InsertMode,
    caller: MaybeLocation,
) -> impl EntityCommand<CmdOut> {
    move |mut entity_mut: EntityWorldMut| {
        entity_mut.assert_not_rev_despawned()?;
        bundle.rev_insert(&mut entity_mut, mode, caller);
        Ok(())
    }
}

/// Reversible version of [`insert_from_world`](bevy_ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(
    _: NotLog,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    rev_insert_from_world_inner::<T>(mode, MaybeLocation::caller())
}

fn rev_insert_from_world_inner<T: Component + FromWorld>(
    mode: InsertMode,
    caller: MaybeLocation,
) -> impl EntityCommand<CmdOut> {
    move |mut entity_mut: EntityWorldMut| {
        if !(mode == InsertMode::Keep && entity_mut.contains::<T>()) {
            let value = entity_mut.world_scope(|world| T::from_world(world));
            entity_mut.rev_insert(value, caller).map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`insert_with`](bevy_ecs::system::entity_command::insert_with).
#[track_caller]
pub fn rev_insert_with<T: Component, F>(
    _: NotLog,
    component_fn: F,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut>
where
    F: FnOnce() -> T + Send + 'static,
{
    rev_insert_with_inner(component_fn, mode, MaybeLocation::caller())
}

fn rev_insert_with_inner<T: Component, F>(
    component_fn: F,
    mode: InsertMode,
    caller: MaybeLocation,
) -> impl EntityCommand<CmdOut>
where
    F: FnOnce() -> T + Send + 'static,
{
    move |mut entity_mut: EntityWorldMut| {
        if !(mode == InsertMode::Keep && entity_mut.contains::<T>()) {
            let value = component_fn();
            entity_mut.rev_insert(value, caller).map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`remove`](bevy_ecs::system::entity_command::remove). Let the second
/// generic be inferred as `_`.
#[track_caller]
pub fn rev_remove<T: RevBundle<Marker>, Marker>(_: NotLog) -> impl EntityCommand<CmdOut> {
    rev_remove_inner::<T, _>(MaybeLocation::caller())
}

fn rev_remove_inner<T: RevBundle<Marker>, Marker>(
    caller: MaybeLocation,
) -> impl EntityCommand<CmdOut> {
    move |mut entity_mut: EntityWorldMut| entity_mut.rev_remove::<T, Marker>(caller).map(|_| ())
}

/// Reversible version of [`despawn`](bevy_ecs::system::entity_command::despawn).
///
/// See the [`undo_redo`](crate::undo_redo) module documentation to understand the mechanics of
/// reversible spawn/despawn.
#[track_caller]
pub fn rev_despawn(_: NotLog) -> impl EntityCommand<CmdOut> {
    rev_despawn_inner(MaybeLocation::caller())
}

fn rev_despawn_inner(caller: MaybeLocation) -> impl EntityCommand<CmdOut> {
    move |entity_mut: EntityWorldMut| entity_mut.rev_despawn(caller)
}
