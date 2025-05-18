use crate::panic_on_error_events;

use super::*;

fn setup() -> World {
    panic_on_error_events();
    let mut world = World::new();
    world.init_resource::<UndoRedoBuffer>();
    world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
    world
}

/*
to test:

- rev_add_related
- rev_clear
- rev_clone_and_spawn
- rev_clone_components
- rev_despawn
- rev_despawn_related
- rev_insert
- rev_insert_by_id (if it gets its own impl)
- rev_insert_by_ids
- rev_insert_if_new
- rev_insert_recursive
- rev_insert_related
- rev_move_components
- rev_remove
- rev_remove_by_id (if it gets its own impl)
- rev_remove_by_ids
- rev_remove_recursive
- rev_remove_related
- rev_remove_with_requires
- rev_retain
- rev_take
*/
