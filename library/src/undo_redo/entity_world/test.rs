use bevy_ecs::{
    change_detection::MaybeLocation,
    entity::Entity,
    hierarchy::{ChildOf, Children},
    related,
    relationship::RelationshipTarget,
    world::World,
};

use crate::undo_redo::{
    IsRevDespawned, RevEntityWorld,
    test::{UnlinkedChildren, assert_undo_redo, assert_undo_redo_finalize},
};

fn rev_with_child_or_children(forward_finalize: bool) {
    let mut world = World::new();
    let parent = world.spawn_empty().id();

    let forward_assert = |world: &mut World, children: &mut [Entity; 2]| {
        let [child_ref, grandchild_ref] = world.entity(*children);
        assert_eq!(child_ref.get::<ChildOf>(), Some(&ChildOf(parent)));
        assert!(!child_ref.is_rev_despawned());
        assert!(!grandchild_ref.is_rev_despawned());
    };

    assert_undo_redo_finalize(
        &mut world,
        |world, not_log| {
            let mut parent_mut = world.entity_mut(parent);
            parent_mut
                .rev_with_related_with_caller::<ChildOf>(
                    not_log,
                    related!(UnlinkedChildren[()]),
                    MaybeLocation::caller(),
                )
                .unwrap();
            let child = parent_mut.get::<Children>().unwrap().iter().next().unwrap();
            let child_ref = parent_mut.world().entity(child);
            let grandchild = child_ref.get::<UnlinkedChildren>().unwrap()[0];
            [child, grandchild]
        },
        |world, children| {
            let [child_ref, grandchild_ref] = world.entity(*children);
            assert_eq!(child_ref.get::<ChildOf>(), None);
            assert!(child_ref.is_rev_despawned());
            assert!(grandchild_ref.is_rev_despawned());
        },
        forward_finalize.then_some(forward_assert),
        |world, children| {
            if forward_finalize {
                forward_assert(world, children)
            } else {
                let &mut [child, grandchild] = children;
                assert_eq!(world.get::<Children>(parent), None);
                assert!(world.get_entity(child).is_err());
                assert!(world.get_entity(grandchild).is_err());
            }
        },
    );
}

#[test]
fn rev_with_child_finalize_forward() {
    rev_with_child_or_children(true);
}

#[test]
fn rev_with_child_finalize_backward() {
    rev_with_child_or_children(false);
}

fn rev_add_child_or_children(one: bool) {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child = world.spawn(ChildOf(parent)).id();

    assert_undo_redo(
        &mut world,
        |world, not_log| {
            let new_child = world.spawn_empty().id();
            let mut parent_mut = world.entity_mut(parent);
            if one {
                parent_mut
                    .rev_add_one_related_with_caller::<ChildOf>(
                        not_log,
                        new_child,
                        MaybeLocation::caller(),
                    )
                    .unwrap();
            } else {
                parent_mut
                    .rev_add_related_with_caller::<ChildOf>(
                        not_log,
                        [new_child],
                        MaybeLocation::caller(),
                    )
                    .unwrap();
            }
            new_child
        },
        |world, new_child| {
            assert_eq!(world.get::<ChildOf>(*new_child), None);
            assert_eq!(world.get::<ChildOf>(child), Some(&ChildOf(parent)));
        },
        |world, new_child| {
            assert_eq!(world.get::<ChildOf>(*new_child), Some(&ChildOf(parent)));
            assert_eq!(world.get::<ChildOf>(child), Some(&ChildOf(parent)));
        },
    );
}

#[test]
fn rev_add_child() {
    rev_add_child_or_children(true);
}

#[test]
fn rev_add_children() {
    rev_add_child_or_children(false);
}

#[test]
fn rev_detach_all_children() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child = world.spawn(ChildOf(parent)).id();

    assert_undo_redo(
        &mut world,
        |world, not_log| {
            let mut parent_mut = world.entity_mut(parent);
            parent_mut
                .rev_detach_all_related_with_caller::<ChildOf>(not_log, MaybeLocation::caller())
                .unwrap();
        },
        |world, _| {
            assert_eq!(world.get::<ChildOf>(child), Some(&ChildOf(parent)));
        },
        |world, _| {
            assert_eq!(world.get::<ChildOf>(child), None);
        },
    );
}

#[test]
fn rev_detach_child() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child1 = world.spawn(ChildOf(parent)).id();
    let child2 = world.spawn(ChildOf(parent)).id();

    let forward_assert = |world: &mut World, _: &mut ()| {
        assert_eq!(world.get::<ChildOf>(child1), Some(&ChildOf(parent)));
        assert_eq!(world.get::<ChildOf>(child2), None);
    };

    assert_undo_redo(
        &mut world,
        |world, not_log| {
            let mut parent_mut = world.entity_mut(parent);
            parent_mut
                .rev_remove_related_with_caller::<ChildOf>(
                    not_log,
                    [child2],
                    MaybeLocation::caller(),
                )
                .unwrap();
        },
        |world, _| {
            assert_eq!(world.get::<ChildOf>(child1), Some(&ChildOf(parent)));
            assert_eq!(world.get::<ChildOf>(child2), Some(&ChildOf(parent)));
        },
        forward_assert,
    );
}

fn rev_despawn_children(forward_finalize: bool) {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child = world.spawn(ChildOf(parent)).id();
    let grandchild = world.spawn(ChildOf(child)).id();

    let backward_assert = |world: &mut World, _: &mut ()| {
        let [child_ref, grandchild_ref] = world.entity([child, grandchild]);
        assert!(!child_ref.is_rev_despawned());
        assert!(!grandchild_ref.is_rev_despawned());
    };

    assert_undo_redo_finalize(
        &mut world,
        |world, not_log| {
            world
                .entity_mut(parent)
                .rev_despawn_related_with_caller::<Children>(not_log, MaybeLocation::caller())
                .unwrap();
        },
        backward_assert,
        forward_finalize.then_some(|world: &mut World, _: &mut ()| {
            let [child_ref, grandchild_ref] = world.entity([child, grandchild]);
            assert!(child_ref.is_rev_despawned());
            assert!(grandchild_ref.is_rev_despawned());
        }),
        |world, unused| {
            if forward_finalize {
                assert_eq!(world.get::<Children>(parent), None);
                assert!(world.get_entity(child).is_err());
                assert!(world.get_entity(grandchild).is_err());
            } else {
                backward_assert(world, unused)
            }
        },
    );
}

#[test]
fn rev_despawn_children_finalize_forward() {
    rev_despawn_children(true);
}

#[test]
fn rev_despawn_children_finalize_backward() {
    rev_despawn_children(false);
}
