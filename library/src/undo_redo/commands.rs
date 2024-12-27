use bevy::ecs::{
    system::{Commands, Resource},
    world::{FromWorld, World},
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
