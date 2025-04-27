use crate::panic_on_error_events;

use super::*;

fn setup() -> World {
    panic_on_error_events();
    let mut world = World::new();
    world.init_resource::<UndoRedoBuffer>();
    world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
    world
}
