use bevy::{
    ecs::{
        error::ignore,
        hierarchy::ChildOf,
        relationship::{
            OrderedRelationshipSourceCollection, RelatedSpawnerCommands, Relationship,
            RelationshipTarget,
        },
        system::{EntityCommand, EntityEntryCommands},
        world::FromWorld,
    },
    ptr::OwningPtr,
};

use super::*;

pub trait RevEntityCommands<'a> {
    /// Reversible version of [`EntityCommands::add_children`].
    fn rev_add_children(&mut self, children: &[Entity]) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::insert_children`].
    fn rev_insert_children(&mut self, index: usize, children: &[Entity])
    -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::add_child`].
    fn rev_add_child(&mut self, child: Entity) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::remove_children`].
    fn rev_remove_children(&mut self, children: &[Entity]) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::with_child`].
    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::with_related`].
    fn rev_with_related<R>(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>
    where
        R: Relationship;

    /// Reversible version of [`EntityCommands::add_related`].
    fn rev_add_related<R>(&mut self, related: &[Entity]) -> &mut EntityCommands<'a>
    where
        R: Relationship;

    /// Reversible version of [`EntityCommands::insert_related`].
    fn rev_insert_related<R>(
        &mut self,
        index: usize,
        related: &[Entity],
    ) -> &mut EntityCommands<'a>
    where
        R: Relationship,
        <<R as Relationship>::RelationshipTarget as RelationshipTarget>::Collection:
            OrderedRelationshipSourceCollection;

    /// Reversible version of [`EntityCommands::add_one_related`].
    fn rev_add_one_related<R>(&mut self, entity: Entity) -> &mut EntityCommands<'a>
    where
        R: Relationship;

    /// Reversible version of [`EntityCommands::remove_related`].
    fn rev_remove_related<R>(&mut self, related: &[Entity]) -> &mut EntityCommands<'a>
    where
        R: Relationship;

    /// Reversible version of [`EntityCommands::despawn_related`].
    fn rev_despawn_related<S>(&mut self) -> &mut EntityCommands<'a>
    where
        S: RelationshipTarget;

    /// Reversible version of [`EntityCommands::insert_recursive`].
    fn rev_insert_recursive<S>(&mut self, bundle: impl Bundle + Clone) -> &mut EntityCommands<'a>
    where
        S: RelationshipTarget;

    /// Reversible version of [`EntityCommands::remove_recursive`].
    fn rev_remove_recursive<S, B>(&mut self) -> &mut EntityCommands<'a>
    where
        S: RelationshipTarget,
        B: Bundle;

    /// Reversible version of [`EntityCommands::insert`].
    fn rev_insert(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::insert_if`].
    fn rev_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool;

    /// Reversible version of [`EntityCommands::insert_if_new`].
    fn rev_insert_if_new(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::insert_if_new_and`].
    fn rev_insert_if_new_and<F>(
        &mut self,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool;

    /// Reversible version of [`EntityCommands::insert_by_id`].
    unsafe fn rev_insert_by_id<T>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static;

    /// Reversible version of [`EntityCommands::try_insert_by_id`].
    unsafe fn rev_try_insert_by_id<T>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static;

    /// Reversible version of [`EntityCommands::try_insert`].
    fn rev_try_insert(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::try_insert_if`].
    fn rev_try_insert_if<F>(
        &mut self,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool;

    /// Reversible version of [`EntityCommands::try_insert_if_new_and`].
    fn rev_try_insert_if_new_and<F>(
        &mut self,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool;

    /// Reversible version of [`EntityCommands::try_insert_if_new`].
    fn rev_try_insert_if_new(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::remove`].
    fn rev_remove<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle;

    /// Reversible version of [`EntityCommands::try_remove`].
    fn rev_try_remove<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle;

    /// Reversible version of [`EntityCommands::remove_with_requires`].
    fn rev_remove_with_requires<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle;

    /// Reversible version of [`EntityCommands::remove_by_id`].
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::clear`].
    fn rev_clear(&mut self) -> &mut EntityCommands<'a>;

    /// Reversible version of [`EntityCommands::despawn`].
    fn rev_despawn(&mut self);

    /// Reversible version of [`EntityCommands::try_despawn`].
    fn rev_try_despawn(&mut self);

    /// Reversible version of [`EntityCommands::retain`].
    fn rev_retain<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle;

    /// Reversible version of [`EntityCommands::clone_and_spawn`].
    fn rev_clone_and_spawn(&mut self) -> EntityCommands<'_>;

    /// Reversible version of [`EntityCommands::clone_components`].
    fn rev_clone_components<B>(&mut self, target: Entity) -> &mut EntityCommands<'a>
    where
        B: Bundle;

    /// Reversible version of [`EntityCommands::move_components`].
    fn rev_move_components<B>(&mut self, target: Entity) -> &mut EntityCommands<'a>
    where
        B: Bundle;
}

impl<'a> RevEntityCommands<'a> for EntityCommands<'a> {
    fn rev_add_children(&mut self, children: &[Entity]) -> &mut EntityCommands<'a> {
        self.rev_add_related::<ChildOf>(children)
    }

    fn rev_insert_children(
        &mut self,
        index: usize,
        children: &[Entity],
    ) -> &mut EntityCommands<'a> {
        self.rev_insert_related::<ChildOf>(index, children)
    }

    fn rev_add_child(&mut self, child: Entity) -> &mut EntityCommands<'a> {
        self.rev_add_one_related::<ChildOf>(child)
    }

    fn rev_remove_children(&mut self, children: &[Entity]) -> &mut EntityCommands<'a> {
        self.rev_remove_related::<ChildOf>(children)
    }

    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a> {
        self.rev_with_related::<ChildOf>(bundle)
    }

    fn rev_with_related<R>(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a>
    where
        R: Relationship,
    {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_with_related::<R>(bundle);
        })
    }

    fn rev_add_related<R>(&mut self, related: &[Entity]) -> &mut EntityCommands<'a>
    where
        R: Relationship,
    {
        let related: Box<[Entity]> = related.into();
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_add_related::<R>(&related);
        })
    }

    fn rev_insert_related<R>(&mut self, index: usize, related: &[Entity]) -> &mut EntityCommands<'a>
    where
        R: Relationship,
        <<R as Relationship>::RelationshipTarget as RelationshipTarget>::Collection:
            OrderedRelationshipSourceCollection,
    {
        let related: Box<[Entity]> = related.into();
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_insert_related::<R>(index, &related);
        })
    }

    fn rev_add_one_related<R>(&mut self, related: Entity) -> &mut EntityCommands<'a>
    where
        R: Relationship,
    {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_add_one_related::<R>(related);
        })
    }

    fn rev_remove_related<R>(&mut self, related: &[Entity]) -> &mut EntityCommands<'a>
    where
        R: Relationship,
    {
        let related: Box<[Entity]> = related.into();
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_remove_related::<R>(&related);
        })
    }

    fn rev_despawn_related<S>(&mut self) -> &mut EntityCommands<'a>
    where
        S: RelationshipTarget,
    {
        self.queue(|mut entity: EntityWorldMut| {
            entity.rev_despawn_related::<S>();
        })
    }

    fn rev_insert_recursive<S>(&mut self, bundle: impl Bundle + Clone) -> &mut EntityCommands<'a>
    where
        S: RelationshipTarget,
    {
        self.queue(|mut entity: EntityWorldMut| {
            entity.rev_insert_recursive::<S>(bundle);
        })
    }

    fn rev_remove_recursive<S, B>(&mut self) -> &mut EntityCommands<'a>
    where
        S: RelationshipTarget,
        B: Bundle,
    {
        self.queue(|mut entity: EntityWorldMut| {
            entity.remove_recursive::<S, B>();
        })
    }

    fn rev_insert(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a> {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_insert(bundle);
        })
    }

    fn rev_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_insert(bundle)
        } else {
            self
        }
    }

    fn rev_insert_if_new(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a> {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_insert_if_new(bundle);
        })
    }

    fn rev_insert_if_new_and<F>(
        &mut self,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_insert_if_new(bundle)
        } else {
            self
        }
    }

    unsafe fn rev_insert_by_id<T>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static,
    {
        self.queue(move |mut entity: EntityWorldMut| {
            OwningPtr::make(value, |ptr| unsafe {
                // SAFETY: todo
                entity.rev_insert_by_id(component_id, ptr);
            })
        })
    }

    unsafe fn rev_try_insert_by_id<T>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut EntityCommands<'a>
    where
        T: Send + 'static,
    {
        self.queue_handled(
            move |mut entity: EntityWorldMut| {
                OwningPtr::make(value, |ptr| unsafe {
                    // SAFETY: todo
                    entity.rev_insert_by_id(component_id, ptr);
                })
            },
            ignore,
        )
    }

    fn rev_try_insert(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a> {
        self.queue_handled(
            move |mut entity: EntityWorldMut| {
                entity.rev_insert(bundle);
            },
            ignore,
        )
    }

    fn rev_try_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_try_insert(bundle)
        } else {
            self
        }
    }

    fn rev_try_insert_if_new_and<F>(
        &mut self,
        bundle: impl Bundle,
        condition: F,
    ) -> &mut EntityCommands<'a>
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_try_insert_if_new(bundle)
        } else {
            self
        }
    }

    fn rev_try_insert_if_new(&mut self, bundle: impl Bundle) -> &mut EntityCommands<'a> {
        self.queue_handled(
            move |mut entity: EntityWorldMut| {
                entity.rev_insert_if_new(bundle);
            },
            ignore,
        )
    }

    fn rev_remove<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(|mut entity: EntityWorldMut| {
            entity.rev_remove::<B>();
        })
    }

    fn rev_try_remove<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue_handled(
            |mut entity: EntityWorldMut| {
                entity.rev_remove::<B>();
            },
            ignore,
        )
    }

    fn rev_remove_with_requires<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(|mut entity: EntityWorldMut| {
            entity.rev_remove_with_requires::<B>();
        })
    }

    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut EntityCommands<'a> {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_remove_by_id(component_id);
        })
    }

    fn rev_clear(&mut self) -> &mut EntityCommands<'a> {
        self.queue(|mut entity: EntityWorldMut| {
            entity.rev_clear();
        })
    }

    fn rev_despawn(&mut self) {
        self.queue(|entity: EntityWorldMut| {
            entity.rev_despawn();
        });
    }

    fn rev_try_despawn(&mut self) {
        self.queue_handled(rev_despawn(), ignore);
    }

    fn rev_retain<B>(&mut self) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(|mut entity: EntityWorldMut| {
            entity.rev_retain::<B>();
        })
    }

    fn rev_clone_and_spawn(&mut self) -> EntityCommands<'_> {
        after_spawn(self.clone_and_spawn())
    }

    fn rev_clone_components<B>(&mut self, target: Entity) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_clone_components::<B>(target);
        })
    }

    fn rev_move_components<B>(&mut self, target: Entity) -> &mut EntityCommands<'a>
    where
        B: Bundle,
    {
        self.queue(move |mut entity: EntityWorldMut| {
            entity.rev_move_components::<B>(target);
        })
    }
}

pub trait RevRelatedSpawnerCommands {
    /// Reversible version of [`RelatedSpawnerCommands::spawn`].
    fn rev_spawn(&mut self, bundle: impl Bundle) -> EntityCommands<'_>;

    /// Reversible version of [`RelatedSpawnerCommands::spawn_empty`].
    fn rev_spawn_empty(&mut self) -> EntityCommands<'_>;
}

impl<'w, R: Relationship> RevRelatedSpawnerCommands for RelatedSpawnerCommands<'w, R> {
    fn rev_spawn(&mut self, bundle: impl Bundle) -> EntityCommands<'_> {
        let target = self.target_entity();
        let mut entity_commands = self.commands_mut().rev_spawn((R::from(target), bundle));
        let entity = entity_commands.id();
        entity_commands.buffer_undo_redo(InsertRelationship {
            entity,
            target: [target],
            _marker: PhantomData::<R>,
        });
        entity_commands
    }

    fn rev_spawn_empty(&mut self) -> EntityCommands<'_> {
        self.rev_spawn(())
    }
}

pub trait RevEntityEntryCommands<T: Component> {
    fn rev_or_default(&mut self) -> &mut Self
    where
        T: Default;

    fn rev_or_from_world(&mut self) -> &mut Self
    where
        T: FromWorld;

    fn rev_or_insert(&mut self, default: T) -> &mut Self;

    fn rev_or_insert_with(&mut self, default: impl Fn() -> T) -> &mut Self;

    fn rev_or_try_insert(&mut self, default: T) -> &mut Self;

    fn rev_or_try_insert_with(&mut self, default: impl Fn() -> T) -> &mut Self;
}

impl<T: Component> RevEntityEntryCommands<T> for EntityEntryCommands<'_, T> {
    fn rev_or_default(&mut self) -> &mut Self
    where
        T: Default,
    {
        self.rev_or_insert(T::default())
    }

    fn rev_or_from_world(&mut self) -> &mut Self
    where
        T: FromWorld,
    {
        self.entity()
            .queue(rev_insert_from_world::<T>(InsertMode::Keep));
        self
    }

    fn rev_or_insert(&mut self, default: T) -> &mut Self {
        self.entity().rev_insert_if_new(default);
        self
    }

    fn rev_or_insert_with(&mut self, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_insert(default())
    }

    fn rev_or_try_insert(&mut self, default: T) -> &mut Self {
        self.entity().rev_try_insert_if_new(default);
        self
    }

    fn rev_or_try_insert_with(&mut self, default: impl Fn() -> T) -> &mut Self {
        self.rev_or_try_insert(default())
    }
}

pub(super) fn after_spawn(mut entity_commands: EntityCommands) -> EntityCommands {
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

/// Reversible version of [`insert`](bevy::ecs::system::entity_command::insert).
#[track_caller]
pub fn rev_insert<B: Bundle>(bundle: B, mode: InsertMode) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        match mode {
            InsertMode::Keep => entity.rev_insert_if_new(bundle),
            InsertMode::Replace => entity.rev_insert(bundle),
        };
    }
}

/// Reversible version of [`insert_by_id`](bevy::ecs::system::entity_command::insert_by_id).
///
/// # Safety
///
/// - [`ComponentId`] must be from the same world as the target entity.
/// - `T` must have the same layout as the one passed during `component_id` creation.
pub unsafe fn rev_insert_by_id<T: Component + Send + 'static>(
    component_id: ComponentId,
    value: T,
    mode: InsertMode,
) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        if entity.contains_id(component_id) && mode == InsertMode::Keep {
            return;
        }
        OwningPtr::make(value, |ptr| unsafe {
            // SAFETY: user promised fulfilling the contract in this command's docs
            entity.rev_insert_by_id(component_id, ptr);
        })
    }
}

/// Reversible version of [`insert_from_world`](bevy::ecs::system::entity_command::insert_from_world).
#[track_caller]
pub fn rev_insert_from_world<T: Component + FromWorld>(mode: InsertMode) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        let value = entity.world_scope(|world| T::from_world(world));
        match mode {
            InsertMode::Keep => entity.insert_if_new(value),
            InsertMode::Replace => entity.insert(value),
        };
    }
}

/// Reversible version of [`remove`](bevy::ecs::system::entity_command::remove).
#[track_caller]
pub fn rev_remove<B: Bundle>() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_remove::<B>();
    }
}

/// Reversible version of [`remove_with_requires`](bevy::ecs::system::entity_command::remove_with_requires).
#[track_caller]
pub fn rev_remove_with_requires<B: Bundle>() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_remove_with_requires::<B>();
    }
}

/// Reversible version of [`remove_by_id`](bevy::ecs::system::entity_command::remove_by_id).
#[track_caller]
pub fn rev_remove_by_id(component_id: ComponentId) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        entity.rev_remove_by_id(component_id);
    }
}

/// Reversible version of [`clear`](bevy::ecs::system::entity_command::clear).
#[track_caller]
pub fn rev_clear() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_clear();
    }
}

/// Reversible version of [`retain`](bevy::ecs::system::entity_command::retain).
#[track_caller]
pub fn rev_retain<B: Bundle>() -> impl EntityCommand {
    |mut entity: EntityWorldMut| {
        entity.rev_retain::<B>();
    }
}

/// Reversible version of [`despawn`](bevy::ecs::system::entity_command::despawn).
#[track_caller]
pub fn rev_despawn() -> impl EntityCommand {
    |entity: EntityWorldMut| {
        entity.rev_despawn();
    }
}

/// Reversible version of [`clone_components`](bevy::ecs::system::entity_command::clone_components).
#[track_caller]
pub fn rev_clone_components<B: Bundle>(target: Entity) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        entity.rev_clone_components::<B>(target);
    }
}

/// Reversible version of [`move_components`](bevy::ecs::system::entity_command::move_components).
#[track_caller]
pub fn rev_move_components<B: Bundle>(target: Entity) -> impl EntityCommand {
    move |mut entity: EntityWorldMut| {
        entity.rev_move_components::<B>(target);
    }
}
