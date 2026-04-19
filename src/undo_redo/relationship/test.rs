use bevy_ecs::hierarchy::ChildOf;

use crate::undo_redo::{
    RevWorld,
    test::{UnlinkedChildOf, assert_undo_redo},
};

use super::*;

fn test_children(include_unlinked_related: bool) {
    let mut world = World::new();
    let root = world.spawn_empty().id();
    let linked = world.spawn(ChildOf(root)).id();
    let unlinked = world.spawn(UnlinkedChildOf(linked)).id();
    let mut entities_set = EntityHashSet::new();

    add_children(
        &world,
        world.get_entity(root).unwrap(),
        &mut entities_set,
        include_unlinked_related,
    );

    assert!(!entities_set.contains(&root));
    assert!(entities_set.contains(&linked));
    assert_eq!(entities_set.contains(&unlinked), include_unlinked_related);
}

fn test_add_remove_related<const ADD: bool>() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child = if ADD {
        world.spawn_empty().id()
    } else {
        world.spawn(ChildOf(parent)).id()
    };

    assert_undo_redo(
        &mut world,
        |world, _| {
            world.queue_undo_redo(
                AddRemoveRelated::<ChildOf, _, ADD>::new(parent, [child], MaybeLocation::caller()),
                MaybeLocation::caller(),
            );
        },
        |world, _| {
            assert_eq!(
                world.get::<ChildOf>(child),
                (!ADD).then_some(&ChildOf(parent))
            );
        },
        |world, _| {
            assert_eq!(world.get::<ChildOf>(child), ADD.then_some(&ChildOf(parent)));
        },
    );
}

#[test]
fn add_children_linked_only() {
    test_children(false);
}

#[test]
fn add_children_linked_and_unlinked() {
    test_children(true);
}

#[test]
fn add_related() {
    test_add_remove_related::<true>();
}

#[test]
fn remove_related() {
    test_add_remove_related::<false>();
}
