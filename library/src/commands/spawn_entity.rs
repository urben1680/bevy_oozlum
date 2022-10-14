use bevy::prelude::{Bundle, Entity, World};

use super::{CommandAction, PresetFunctions, ReversibleCommand, ReversibleCommandInitialized};

pub struct SpawnEntity<T: Bundle> {
    data: T,
}

impl<T: Bundle> SpawnEntity<T> {
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

impl<T: Bundle> ReversibleCommand for SpawnEntity<T> {
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>> {
        Some(Box::new(SpawnEntityInitialized {
            entity: world.spawn().insert_bundle(self.data).id(),
        }))
    }
}

pub struct SpawnEntityInitialized {
    entity: Entity,
}

impl ReversibleCommandInitialized for SpawnEntityInitialized {
    fn undo(&mut self, world: &mut World) {
        Self::entity(world, CommandAction::Undo, false, self.entity);
    }
    fn redo(&mut self, world: &mut World) {
        Self::entity(world, CommandAction::Redo, false, self.entity);
    }
    fn redo_finalize(self: Box<Self>, world: &mut World) {
        Self::entity(world, CommandAction::RedoFinalize, false, self.entity);
    }
    fn undo_finalize(self: Box<Self>, world: &mut World) {
        Self::entity(world, CommandAction::UndoFinalize, false, self.entity);
    }
}
