use bevy::ecs::{
    result::Error,
    bundle::Bundle, component::ComponentId, entity::{Entity, EntityCloneBuilder}, resource::Resource, system::{entity_command::CommandWithEntity, Commands, EntityCommand, EntityCommands}, world::{FromWorld, World}
};

use super::{BuffersUndoRedo, UndoRedo};

pub trait RevCommands {
    fn rev_init_resource<R: Resource + FromWorld>(&mut self);
    fn rev_insert_resource<R: Resource>(&mut self, resource: R);
    fn rev_remove_resource<R: Resource>(&mut self);
}

impl RevCommands for Commands<'_, '_> {
    fn rev_init_resource<R: Resource + FromWorld>(&mut self) {
        self.queue(|world: &mut World| {
            if !world.contains_resource::<R>() {
                world.init_resource::<R>();
                world.buffer_undo_redo(ResourceSwap::<R>(None));
            }
        })
    }
    fn rev_insert_resource<R: Resource>(&mut self, resource: R) {
        self.queue(|world: &mut World| {
            let initiialized = ResourceSwap(world.remove_resource::<R>());
            world.insert_resource(resource);
            world.buffer_undo_redo(initiialized);
        })
    }
    fn rev_remove_resource<R: Resource>(&mut self) {
        self.queue(|world: &mut World| {
            if let Some(resource) = world.remove_resource::<R>() {
                world.buffer_undo_redo(ResourceSwap(Some(resource)));
            }
        })
    }
}
struct ResourceSwap<R: Resource>(Option<R>);

impl<R: Resource> UndoRedo for ResourceSwap<R> {
    fn undo(&mut self, world: &mut World) {
        match world.get_resource_mut::<R>() {
            Some(mut r1) => match self.0.as_mut() {
                Some(r2) => core::mem::swap(&mut *r1, r2),
                None => self.0 = world.remove_resource::<R>(),
            },
            None => {
                if let Some(r2) = self.0.take() {
                    world.insert_resource(r2)
                }
            }
        }
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world)
    }
}

pub trait RevEntityCommands {
    /// Reversible version of [`EntityCommands::insert`].
    fn rev_insert(&mut self, bundle: impl Bundle) -> &mut Self;
    /// Reversible version of [`EntityCommands::insert_if`].
    fn rev_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self 
    where
        F: FnOnce() -> bool;
    /// Reversible version of [`EntityCommands::insert_if_new`].
    fn rev_insert_if_new(&mut self, bundle: impl Bundle) -> &mut Self;
    /// Reversible version of [`EntityCommands::insert_if_new_and`].
    fn rev_insert_if_new_and<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self
    where
        F: FnOnce() -> bool;
    /// Reversible version of [`EntityCommands::insert_by_id`].
    unsafe fn rev_insert_by_id<T: Send + 'static>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut Self;
    /// Reversible version of [`EntityCommands::try_insert_by_id`].
    unsafe fn rev_try_insert_by_id<T: Send + 'static>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut Self;
    /// Reversible version of [`EntityCommands::try_insert`].
    fn rev_try_insert(&mut self, bundle: impl Bundle) -> &mut Self;
    /// Reversible version of [`EntityCommands::try_insert_if`].
    fn rev_try_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self
    where
        F: FnOnce() -> bool;
    /// Reversible version of [`EntityCommands::try_insert_if_new_and`].
    fn rev_try_insert_if_new_and<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self
    where
        F: FnOnce() -> bool;
    /// Reversible version of [`EntityCommands::try_insert_if_new`].
    fn rev_try_insert_if_new(&mut self, bundle: impl Bundle) -> &mut Self;
    /// Reversible version of [`EntityCommands::remove`].
    fn rev_remove<T>(&mut self) -> &mut Self
    where
        T: Bundle;
    /// Reversible version of [`EntityCommands::try_remove`].
    fn rev_try_remove<T>(&mut self) -> &mut Self
    where
        T: Bundle;
    /// Reversible version of [`EntityCommands::remove_with_requires`].
    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self;
    /// Reversible version of [`EntityCommands::remove_by_id`].
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self;
    /// Reversible version of [`EntityCommands::clear`].
    fn rev_clear(&mut self) -> &mut Self;
    /// Reversible version of [`EntityCommands::despawn`].
    fn rev_despawn(&mut self);
    /// Reversible version of [`EntityCommands::despawn_recursive`].
    fn rev_despawn_recursive(&mut self);
    /// Reversible version of [`EntityCommands::try_despawn`].
    fn rev_try_despawn(&mut self);
    /// Reversible version of [`EntityCommands::queue`].
    fn rev_queue<C: EntityCommand<T> + CommandWithEntity<M>, T, M>(
        &mut self,
        command: C,
    ) -> &mut Self;
    /// Reversible version of [`EntityCommands::queue_handled`].
    fn rev_queue_handled<C: EntityCommand<T> + CommandWithEntity<M>, T, M>(
        &mut self,
        command: C,
        error_handler: fn(&mut World, Error),
    ) -> &mut Self;
    /// Reversible version of [`EntityCommands::retain`].
    fn rev_retain<T>(&mut self) -> &mut Self
    where
        T: Bundle;
    /// Reversible version of [`EntityCommands::clone_with`].
    fn rev_clone_with(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityCloneBuilder) + Send + Sync + 'static,
    ) -> &mut Self;
    /// Reversible version of [`EntityCommands::clone_and_spawn`].
    fn rev_clone_and_spawn(&mut self) -> EntityCommands<'_>;
    /// Reversible version of [`EntityCommands::clone_and_spawn_with`].
    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityCloneBuilder) + Send + Sync + 'static,
    ) -> EntityCommands<'_>;
    /// Reversible version of [`EntityCommands::clone_components`].
    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;
    /// Reversible version of [`EntityCommands::move_components`].
    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;
}

impl RevEntityCommands for EntityCommands<'_> {
    fn rev_insert(&mut self, bundle: impl Bundle) -> &mut Self {
        todo!()
    }
    fn rev_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self 
    where
        F: FnOnce() -> bool
    {
        if condition() {
            self.rev_insert(bundle)
        } else {
            self
        }
    }
    fn rev_insert_if_new(&mut self, bundle: impl Bundle) -> &mut Self {
        todo!()
    }
    fn rev_insert_if_new_and<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_insert_if_new(bundle)
        } else {
            self
        }
    }
    unsafe fn rev_insert_by_id<T: Send + 'static>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut Self {
        todo!()
    }
    unsafe fn rev_try_insert_by_id<T: Send + 'static>(
        &mut self,
        component_id: ComponentId,
        value: T,
    ) -> &mut Self {
        todo!()
    }
    fn rev_try_insert(&mut self, bundle: impl Bundle) -> &mut Self {
        todo!()
    }
    fn rev_try_insert_if<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_try_insert(bundle)
        } else {
            self
        }
    }
    fn rev_try_insert_if_new_and<F>(&mut self, bundle: impl Bundle, condition: F) -> &mut Self
    where
        F: FnOnce() -> bool,
    {
        if condition() {
            self.rev_try_insert_if_new(bundle)
        } else {
            self
        }
    }
    fn rev_try_insert_if_new(&mut self, bundle: impl Bundle) -> &mut Self {
        todo!()
    }
    fn rev_remove<T>(&mut self) -> &mut Self
    where
        T: Bundle {
        todo!()
    }
    fn rev_try_remove<T>(&mut self) -> &mut Self
    where
        T: Bundle {
        todo!()
    }
    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        todo!()
    }
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        todo!()
    }
    fn rev_clear(&mut self) -> &mut Self {
        todo!()
    }
    fn rev_despawn(&mut self) {
        todo!()
    }
    fn rev_despawn_recursive(&mut self) {
        todo!()
    }
    fn rev_try_despawn(&mut self) {
        todo!()
    }
    fn rev_queue<C: EntityCommand<T> + CommandWithEntity<M>, T, M>(
        &mut self,
        command: C,
    ) -> &mut Self {
        todo!()
    }
    fn rev_queue_handled<C: EntityCommand<T> + CommandWithEntity<M>, T, M>(
        &mut self,
        command: C,
        error_handler: fn(&mut World, Error),
    ) -> &mut Self {
        todo!()
    }
    fn rev_retain<T>(&mut self) -> &mut Self
    where
        T: Bundle {
        todo!()
    }
    fn rev_clone_with(
        &mut self,
        target: Entity,
        config: impl FnOnce(&mut EntityCloneBuilder) + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }
    fn rev_clone_and_spawn(&mut self) -> EntityCommands<'_> {
        todo!()
    }
    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityCloneBuilder) + Send + Sync + 'static,
    ) -> EntityCommands<'_> {
        todo!()
    }
    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        todo!()
    }
    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        todo!()
    }
}
