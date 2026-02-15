use bevy_ecs::{
    entity::Entity,
    hierarchy::{ChildOf, Children},
    related,
    world::World,
};

use crate::undo_redo::{
    IsRevDespawned, RevEntityWorldMut,
    test::{NonLinkedChildren, assert_undo_redo_finalize},
};

fn rev_with_children<const FORWARD_FINALIZE: bool>() {
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
        |world, meta_past_len| {
            let [mut child, mut grandchild] = [Entity::PLACEHOLDER; 2];
            world
                .entity_mut(parent)
                .rev_with_children(meta_past_len, |spawner| {
                    let child_ref = spawner.spawn(related!(NonLinkedChildren[()]));
                    child = child_ref.id();
                    grandchild = child_ref.get::<NonLinkedChildren>().unwrap()[0];
                });
            [child, grandchild]
        },
        |world, children| {
            let [child_ref, grandchild_ref] = world.entity(*children);
            assert_eq!(child_ref.get::<ChildOf>(), None);
            assert!(child_ref.is_rev_despawned());
            assert!(grandchild_ref.is_rev_despawned());
        },
        FORWARD_FINALIZE.then_some(forward_assert),
        |world, children| {
            if FORWARD_FINALIZE {
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
fn rev_with_children_finalize_forward() {
    rev_with_children::<true>();
}

#[test]
fn rev_with_children_finalize_backward() {
    rev_with_children::<false>();
}
