use crate::{
    meta::MetaPastLen,
    prelude::UndoRedo,
    undo_redo::{
        EntityRevDespawnedError, RevCommands, RevDespawnCleaner, assert_not_rev_despawned,
        rev_spawn_with_caller, rev_try_clear_with_caller, rev_try_despawn_single_with_caller,
        rev_try_insert_with_caller, rev_try_remove_with_caller, rev_try_retain_with_caller,
    },
};
use bevy_ecs::ptr::OwningPtr;
use bevy_ecs::{
    bundle::{Bundle, InsertMode},
    change_detection::MaybeLocation,
    component::{Component, ComponentId},
    entity::{EntityClonerBuilder, OptIn, OptOut},
    error::ignore,
    system::{EntityCommand, EntityCommands, EntityEntryCommands},
    world::{DeferredWorld, EntityWorldMut, FromWorld},
};
use core::marker::PhantomData;

pub trait RevEntityCommands<'a> {
    fn redo_and_buffer(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo);

    #[track_caller]
    fn make_rev_log_scoped(&mut self, past_len: MetaPastLen) -> &mut Self;

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) relationship related that is not yet supported
    // d) missed by accident!

    /// Reversible version of [`EntityCommands::clear`].
    fn rev_clear(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::clone_and_spawn`].
    fn rev_clone_and_spawn(&mut self, past_len: MetaPastLen) -> EntityCommands<'_>;

    /// Reversible version of [`EntityCommands::clone_and_spawn_with_opt_in`].
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        past_len: MetaPastLen,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> EntityCommands<'_>;

    /// Reversible version of [`EntityCommands::clone_and_spawn_with_opt_out`].
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        past_len: MetaPastLen,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> EntityCommands<'_>;

    // rev_clone_components
    // out of scope

    /// Reversible version of [`EntityCommands::despawn`].
    fn rev_despawn_single(&mut self, past_len: MetaPastLen);

    /// Reversible version of [`EntityCommands::insert`].
    fn rev_insert(&mut self, past_len: MetaPastLen, bundle: impl Bundle)
    -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::insert_by_id`].
    unsafe fn rev_insert_by_id<T>(
        &mut self,
        past_len: MetaPastLen,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static;

    /// Reversible version of [`EntityCommands::insert_if`].
    fn rev_insert_if<F>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool;

    /// Reversible version of [`EntityCommands::insert_if_new`].
    fn rev_insert_if_new(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::insert_if_new_and`].
    fn rev_insert_if_new_and<F>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool;

    // rev_move_components
    // out of scope

    /// Reversible version of [`EntityCommands::remove`].
    fn rev_remove<B: Bundle>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::remove_by_id`].
    fn rev_remove_by_id(
        &mut self,
        past_len: MetaPastLen,
        component_id: ComponentId,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::remove_with_requires`].
    fn rev_remove_with_requires<B: Bundle>(
        &mut self,
        past_len: MetaPastLen,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::retain`].
    fn rev_retain<B>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>
    where
        B: Bundle;

    /// Reversible version of [`EntityCommands::try_despawn`].
    fn rev_try_despawn_single(&mut self, past_len: MetaPastLen);

    /// Reversible version of [`EntityCommands::try_insert`].
    fn rev_try_insert(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::try_insert_by_id`].
    unsafe fn rev_try_insert_by_id<T: Send + 'static>(
        &mut self,
        past_len: MetaPastLen,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::try_insert_if`].
    fn rev_try_insert_if<F: FnOnce() -> bool>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::try_insert_if_new`].
    fn rev_try_insert_if_new(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::try_insert_if_new_and`].
    fn rev_try_insert_if_new_and<F: FnOnce() -> bool>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::try_remove`].
    fn rev_try_remove<B: Bundle>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>;
}

impl<'a> RevEntityCommands<'a> for EntityCommands<'a> {
    fn redo_and_buffer(&mut self, past_len: MetaPastLen, undo_redo: impl UndoRedo) {
        self.commands_mut().redo_and_buffer(past_len, undo_redo);
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        past_len: MetaPastLen,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> EntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let source = self.id();
        let mut clone = self.clone_and_spawn_with_opt_in(config);
        clone.queue(move |mut entity_mut: EntityWorldMut| {
            let clone = entity_mut.id();
            // SAFETY: cannot change entity location as DeferredWorld
            let world: DeferredWorld = unsafe { entity_mut.world_mut().into() };
            assert_not_rev_despawned(world.entity(source))
                .map(|_| rev_spawn_with_caller(world, clone, past_len, caller))
        });
        clone
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        past_len: MetaPastLen,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> EntityCommands<'_> {
        let caller = MaybeLocation::caller();
        let source = self.id();
        let mut clone = self.clone_and_spawn_with_opt_out(config);
        clone.queue(move |mut entity_mut: EntityWorldMut| {
            let clone = entity_mut.id();
            // SAFETY: cannot change entity location as DeferredWorld
            let world: DeferredWorld = unsafe { entity_mut.world_mut().into() };
            assert_not_rev_despawned(world.entity(source))
                .map(|_| rev_spawn_with_caller(world, clone, past_len, caller))
        });
        clone
    }

    #[track_caller]
    fn rev_insert(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a> {
        self.queue(rev_insert(past_len, bundle, InsertMode::Replace))
    }

    #[track_caller]
    fn rev_insert_if<F>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_insert(past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_insert_if_new(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a> {
        self.queue(rev_insert(past_len, bundle, InsertMode::Keep))
    }

    #[track_caller]
    fn rev_insert_if_new_and<F>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_insert_if_new(past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    unsafe fn rev_insert_by_id<T>(
        &mut self,
        past_len: MetaPastLen,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static,
    {
        // SAFETY: todo
        self.queue(unsafe { rev_insert_by_id(past_len, component_id, value, InsertMode::Replace) })
    }

    #[track_caller]
    unsafe fn rev_try_insert_by_id<T>(
        &mut self,
        past_len: MetaPastLen,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static,
    {
        // SAFETY: todo
        self.queue_handled(
            unsafe { rev_insert_by_id(past_len, component_id, value, InsertMode::Replace) },
            ignore,
        )
    }

    #[track_caller]
    fn rev_try_insert(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a> {
        self.queue_handled(rev_insert(past_len, bundle, InsertMode::Replace), ignore)
    }

    #[track_caller]
    fn rev_try_insert_if<F>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_try_insert(past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_insert_if_new_and<F>(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_try_insert_if_new(past_len, bundle)
        } else {
            self
        }
    }

    #[track_caller]
    fn rev_try_insert_if_new(
        &mut self,
        past_len: MetaPastLen,
        bundle: impl Bundle,
    ) -> &mut EntityCommands<'a> {
        self.queue_handled(rev_insert(past_len, bundle, InsertMode::Keep), ignore)
    }

    #[track_caller]
    fn rev_remove<B>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(rev_remove::<B>(past_len))
    }

    #[track_caller]
    fn rev_try_remove<B>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue_handled(rev_remove::<B>(past_len), ignore)
    }

    #[track_caller]
    fn rev_remove_with_requires<B>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(rev_remove_with_requires::<B>(past_len))
    }

    #[track_caller]
    fn rev_remove_by_id(
        &mut self,
        past_len: MetaPastLen,
        component_id: ComponentId,
    ) -> &mut EntityCommands<'a> {
        self.queue(rev_remove_by_id(past_len, component_id))
    }

    #[track_caller]
    fn rev_clear(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a> {
        self.queue(rev_clear(past_len))
    }

    #[track_caller]
    fn rev_despawn_single(&mut self, past_len: MetaPastLen) {
        self.queue(rev_despawn_single(past_len));
    }

    #[track_caller]
    fn rev_try_despawn_single(&mut self, past_len: MetaPastLen) {
        self.queue_handled(rev_despawn_single(past_len), ignore);
    }

    #[track_caller]
    fn make_rev_log_scoped(&mut self, past_len: MetaPastLen) -> &mut Self {
        let caller = MaybeLocation::caller();
        self.queue(move |mut entity_mut: EntityWorldMut| {
            let entity = entity_mut.id();
            entity_mut
                .resource_mut::<RevDespawnCleaner>()
                .log_spawn(entity, caller, past_len);
        });
        self
    }

    #[track_caller]
    fn rev_retain<B>(&mut self, past_len: MetaPastLen) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(rev_retain::<B>(past_len))
    }

    #[track_caller]
    fn rev_clone_and_spawn(&mut self, past_len: MetaPastLen) -> EntityCommands<'_> {
        self.rev_clone_and_spawn_with_opt_out(past_len, |_| ())
    }
}

pub trait RevEntityEntryCommands<T: Component> {
    fn rev_or_default(&mut self, past_len: MetaPastLen) -> &mut Self
    where
        T: Default;

    fn rev_or_from_world(&mut self, past_len: MetaPastLen) -> &mut Self
    where
        T: FromWorld;

    fn rev_or_insert(&mut self, past_len: MetaPastLen, default: T) -> &mut Self;

    fn rev_or_insert_with(&mut self, past_len: MetaPastLen, default: impl Fn() -> T) -> &mut Self;

    fn rev_or_try_insert(&mut self, past_len: MetaPastLen, default: T) -> &mut Self;

    fn rev_or_try_insert_with(
        &mut self,
        past_len: MetaPastLen,
        default: impl Fn() -> T,
    ) -> &mut Self;
}

impl<T: Component> RevEntityEntryCommands<T> for EntityEntryCommands<'_, T> {
    fn rev_or_default(&mut self, past_len: MetaPastLen) -> &mut Self
    where
        T: Default,
    {
        self.rev_or_insert(past_len, T::default())
    }

    fn rev_or_from_world(&mut self, past_len: MetaPastLen) -> &mut Self
    where
        T: FromWorld,
    {
        self.entity()
            .queue(rev_insert_from_world::<T>(past_len, InsertMode::Keep));
        self
    }

    fn rev_or_insert(&mut self, past_len: MetaPastLen, default: T) -> &mut Self {
        self.entity().rev_insert_if_new(past_len, default);
        self
    }

    fn rev_or_insert_with(&mut self, past_len: MetaPastLen, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_insert(past_len, default())
    }

    fn rev_or_try_insert(&mut self, past_len: MetaPastLen, default: T) -> &mut Self {
        self.entity().rev_try_insert_if_new(past_len, default);
        self
    }

    fn rev_or_try_insert_with(
        &mut self,
        past_len: MetaPastLen,
        default: impl Fn() -> T,
    ) -> &mut Self {
        self.rev_or_try_insert(past_len, default())
    }
}

type CmdOut = Result<(), EntityRevDespawnedError>;

/// Reversible version of [`insert`](bevy_ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<B: Bundle>(
    past_len: MetaPastLen,
    bundle: B,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        rev_try_insert_with_caller(
            &mut entity_mut,
            PhantomData::<B>,
            mode,
            |entity_mut| entity_mut.insert(bundle),
            past_len,
            caller,
        )
        .map(|_| ())
    }
}

/// Reversible version of [`insert_by_id`](bevy_ecs::system::entity_command::insert_by_id).
///
/// # Safety
///
/// - [`ComponentId`] must be from the same world as the target entity.
/// - `T` must have the same layout as the one passed during `component_id` creation.
#[track_caller]
pub unsafe fn rev_insert_by_id<T: Send + 'static>(
    past_len: MetaPastLen,
    component_id: ComponentId,
    value: T,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        OwningPtr::make(value, |component| {
            rev_try_insert_with_caller(
                &mut entity_mut,
                component_id,
                mode,
                // SAFETY:
                // - `component_id` safety is ensured by the caller
                // - `ptr` is valid within the `make` block
                |entity_mut| unsafe { entity_mut.insert_by_id(component_id, component) },
                past_len,
                caller,
            )
            .map(|_| ())
        })
    }
}

/// Reversible version of [`insert_from_world`](bevy_ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(
    past_len: MetaPastLen,
    mode: InsertMode,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        if !(mode == InsertMode::Keep && entity_mut.contains::<T>()) {
            let value = entity_mut.world_scope(|world| T::from_world(world));
            rev_try_insert_with_caller(
                &mut entity_mut,
                PhantomData::<T>,
                mode,
                |entity_mut| entity_mut.insert(value),
                past_len,
                caller,
            )
            .map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`insert_with`](bevy_ecs::system::entity_command::insert_with).
#[track_caller]
pub fn rev_insert_with<T: Component, F>(
    past_len: MetaPastLen,
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
            rev_try_insert_with_caller(
                &mut entity_mut,
                PhantomData::<T>,
                mode,
                |entity_mut| entity_mut.insert(value),
                past_len,
                caller,
            )
            .map(|_| ())
        } else {
            Ok(())
        }
    }
}

/// Reversible version of [`remove`](bevy_ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<T: Bundle>(past_len: MetaPastLen) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        rev_try_remove_with_caller::<_, false>(&mut entity_mut, PhantomData::<T>, past_len, caller)
            .map(|_| ())
    }
}

/// Reversible version of [`remove_with_requires`](bevy_ecs::system::entity_command::remove_with_requires).
#[track_caller]
pub fn rev_remove_with_requires<T: Bundle>(past_len: MetaPastLen) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        rev_try_remove_with_caller::<_, true>(&mut entity_mut, PhantomData::<T>, past_len, caller)
            .map(|_| ())
    }
}

/// Reversible version of [`remove_by_id`](bevy_ecs::system::entity_command::remove_by_id).
#[track_caller]
pub fn rev_remove_by_id(
    past_len: MetaPastLen,
    component_id: ComponentId,
) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        rev_try_remove_with_caller::<_, false>(&mut entity_mut, component_id, past_len, caller)
            .map(|_| ())
    }
}

/// Reversible version of [`clear`](bevy_ecs::system::entity_command::clear).
#[track_caller]
pub fn rev_clear(past_len: MetaPastLen) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        rev_try_clear_with_caller(&mut entity_mut, past_len, caller).map(|_| ())
    }
}

/// Reversible version of [`retain`](bevy_ecs::system::entity_command::retain).
#[track_caller]
pub fn rev_retain<T: Bundle>(past_len: MetaPastLen) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |mut entity_mut: EntityWorldMut| {
        rev_try_retain_with_caller(&mut entity_mut, PhantomData::<T>, past_len, caller).map(|_| ())
    }
}

/// Reversible version of [`despawn`](bevy_ecs::system::entity_command::despawn).
#[track_caller]
pub fn rev_despawn_single(past_len: MetaPastLen) -> impl EntityCommand<CmdOut> {
    let caller = MaybeLocation::caller();
    move |entity_mut: EntityWorldMut| {
        rev_try_despawn_single_with_caller(entity_mut, past_len, caller).map(|_| ())
    }
}
