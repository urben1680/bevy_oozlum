use bevy_ecs::hierarchy::ChildOf;

use crate::undo_redo::test::{NonLinkedChildOf, assert_undo_redo_finalize};

use super::*;

#[derive(Clone, Copy, Debug)]
struct Hierarchy<R, T> {
    root: R,
    rev_spawned_linked: T,
    rev_spawned_unlinked: T,
    rev_despawned_linked: T,
    rev_despawned_unlinked: T,
}

impl Hierarchy<Entity, Entity> {
    fn new(world: &mut World, bundle: impl Bundle) -> Self {
        let rev_despawned = RevDespawned(MaybeLocation::caller());
        let root = world.spawn(bundle).id();
        let rev_spawned_linked = world.spawn(ChildOf(root)).id();
        let rev_spawned_unlinked = world.spawn(NonLinkedChildOf(root)).id();
        let rev_despawned_linked = world.spawn((ChildOf(root), rev_despawned)).id();
        let rev_despawned_unlinked = world.spawn((NonLinkedChildOf(root), rev_despawned)).id();
        Self {
            root,
            rev_spawned_linked,
            rev_spawned_unlinked,
            rev_despawned_linked,
            rev_despawned_unlinked,
        }
    }
    fn assert_rev_despawned(self, world: &World, despawned: Hierarchy<bool, bool>) {
        let [
            root,
            rev_spawned_linked,
            rev_spawned_unlinked,
            rev_despawned_linked,
            rev_despawned_unlinked,
        ] = world
            .get_entity([
                self.root,
                self.rev_spawned_linked,
                self.rev_spawned_unlinked,
                self.rev_despawned_linked,
                self.rev_despawned_unlinked,
            ])
            .unwrap();

        assert_eq!(root.is_rev_despawned(), despawned.root,);
        assert_eq!(
            rev_spawned_linked.is_rev_despawned(),
            despawned.rev_spawned_linked,
        );
        assert_eq!(
            rev_spawned_unlinked.is_rev_despawned(),
            despawned.rev_spawned_unlinked,
        );
        assert_eq!(
            rev_despawned_linked.is_rev_despawned(),
            despawned.rev_despawned_linked,
        );
        assert_eq!(
            rev_despawned_unlinked.is_rev_despawned(),
            despawned.rev_despawned_unlinked,
        );
    }
    fn assert_despawned(self, world: &World, despawned: Hierarchy<bool, bool>) {
        assert_eq!(world.get_entity(self.root).is_err(), despawned.root,);
        assert_eq!(
            world.get_entity(self.rev_spawned_linked).is_err(),
            despawned.rev_spawned_linked,
        );
        assert_eq!(
            world.get_entity(self.rev_spawned_unlinked).is_err(),
            despawned.rev_spawned_unlinked,
        );
        // do not assert previously rev_despawn entities being present because that is undefined.
        // entities that were already marked as despawned would have been finalized already anyway.
    }
}

impl Hierarchy<Entity, Hierarchy<Entity, Entity>> {
    fn new_nested(world: &mut World) -> Self {
        let rev_despawned = RevDespawned(MaybeLocation::caller());
        let root = world.spawn_empty().id();
        let rev_spawned_linked = Hierarchy::new(world, ChildOf(root));
        let rev_spawned_unlinked = Hierarchy::new(world, NonLinkedChildOf(root));
        let rev_despawned_linked = Hierarchy::new(world, (ChildOf(root), rev_despawned));
        let rev_despawned_unlinked = Hierarchy::new(world, (NonLinkedChildOf(root), rev_despawned));
        Self {
            root,
            rev_spawned_linked,
            rev_spawned_unlinked,
            rev_despawned_linked,
            rev_despawned_unlinked,
        }
    }
    fn assert_rev_despawned_nested(
        self,
        world: &World,
        despawned: Hierarchy<bool, Hierarchy<bool, bool>>,
    ) {
        assert_eq!(
            world.get_entity(self.root).unwrap().is_rev_despawned(),
            despawned.root
        );
        self.rev_spawned_linked
            .assert_rev_despawned(world, despawned.rev_spawned_linked);
        self.rev_spawned_unlinked
            .assert_rev_despawned(world, despawned.rev_spawned_unlinked);
        self.rev_despawned_linked
            .assert_rev_despawned(world, despawned.rev_despawned_linked);
        self.rev_despawned_unlinked
            .assert_rev_despawned(world, despawned.rev_despawned_unlinked);
    }
    fn assert_despawned_nested(
        self,
        world: &World,
        despawned: Hierarchy<bool, Hierarchy<bool, bool>>,
    ) {
        assert_eq!(world.get_entity(self.root).is_err(), despawned.root);
        self.rev_spawned_linked
            .assert_despawned(world, despawned.rev_spawned_linked);
        self.rev_spawned_unlinked
            .assert_despawned(world, despawned.rev_spawned_unlinked);
        // do not assert previously rev_despawn entities being present because that is undefined.
        // entities that were already marked as despawned would have been finalized already anyway.
    }
}

impl Hierarchy<bool, bool> {
    fn new_expected_despawned(root: bool, linked: bool, unlinked: bool) -> Self {
        Self {
            root,
            rev_spawned_linked: linked,
            rev_spawned_unlinked: unlinked,
            rev_despawned_linked: true, // previously rev_despawn entity remains unchanged
            rev_despawned_unlinked: true, // previously rev_despawn entity remains unchanged
        }
    }
    fn new_expected_unchanged_despawned() -> Self {
        Self {
            root: true,                   // previously rev_despawn entity remains unchanged
            rev_spawned_linked: false, // child of previously rev_despawn entity remains unchanged
            rev_spawned_unlinked: false, // child of previously rev_despawn entity remains unchanged
            rev_despawned_linked: true, // previously rev_despawn entity remains unchanged
            rev_despawned_unlinked: true, // previously rev_despawn entity remains unchanged
        }
    }
}

impl Hierarchy<bool, Hierarchy<bool, bool>> {
    fn new_expected_despawned_nested(root: bool, linked: bool, unlinked: bool) -> Self {
        Self {
            root,
            rev_spawned_linked: Hierarchy::new_expected_despawned(linked, linked, unlinked),
            rev_spawned_unlinked: Hierarchy::new_expected_despawned(unlinked, unlinked, unlinked),
            rev_despawned_linked: Hierarchy::new_expected_unchanged_despawned(),
            rev_despawned_unlinked: Hierarchy::new_expected_unchanged_despawned(),
        }
    }
}

fn test<const SPAWN: bool>(
    include_unlinked_related: bool,
    as_entity_world_mut: bool,
    forward: Hierarchy<bool, Hierarchy<bool, bool>>,
    backward: Hierarchy<bool, Hierarchy<bool, bool>>,
    forward_finalized: bool,
) {
    let mut world = World::new();
    let hierarchy = Hierarchy::new_nested(&mut world);
    assert_undo_redo_finalize(
        &mut world,
        |world, meta_past_len| {
            let caller = MaybeLocation::caller();
            if as_entity_world_mut {
                let mut root = world.entity_mut(hierarchy.root);
                let success = mark_entity::<SPAWN>(
                    meta_past_len,
                    &mut root,
                    include_unlinked_related,
                    caller,
                );
                assert!(success);
            } else {
                mark_entities::<SPAWN>(
                    meta_past_len,
                    world,
                    &[hierarchy.root],
                    include_unlinked_related,
                    caller,
                );
            }
            update_spawn_despawn(world).unwrap();
            hierarchy.assert_rev_despawned_nested(world, forward);
        },
        |world| {
            update_spawn_despawn(world).unwrap();
            hierarchy.assert_rev_despawned_nested(world, backward);
        },
        forward_finalized.then_some(|world: &mut World| {
            update_spawn_despawn(world).unwrap();
            hierarchy.assert_rev_despawned_nested(world, forward);
        }),
        |world| {
            update_spawn_despawn(world).unwrap();
            let finalized = if forward_finalized { forward } else { backward };
            hierarchy.assert_despawned_nested(world, finalized);
        },
    );
}

#[test]
fn spawn_linked_only_entity_finalize_forward() {
    test::<true>(
        false,
        true,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, false),
        true,
    );
}

#[test]
fn spawn_linked_only_entity_finalize_backward() {
    test::<true>(
        false,
        true,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, false),
        false,
    );
}

#[test]
fn spawn_linked_only_entities_finalize_forward() {
    test::<true>(
        false,
        false,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, false),
        true,
    );
}

#[test]
fn spawn_linked_only_entities_finalize_backward() {
    test::<true>(
        false,
        false,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, false),
        false,
    );
}

#[test]
fn despawn_linked_only_entity_finalize_forward() {
    test::<false>(
        false,
        true,
        Hierarchy::new_expected_despawned_nested(true, true, false),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        true,
    );
}

#[test]
fn despawn_linked_only_entity_finalize_backward() {
    test::<false>(
        false,
        true,
        Hierarchy::new_expected_despawned_nested(true, true, false),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        false,
    );
}

#[test]
fn despawn_linked_only_entities_finalize_forward() {
    test::<false>(
        false,
        false,
        Hierarchy::new_expected_despawned_nested(true, true, false),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        true,
    );
}

#[test]
fn despawn_linked_only_entities_finalize_backward() {
    test::<false>(
        false,
        false,
        Hierarchy::new_expected_despawned_nested(true, true, false),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        false,
    );
}

#[test]
fn spawn_linked_and_unlinked_entity_finalize_forward() {
    test::<true>(
        true,
        true,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, true),
        true,
    );
}

#[test]
fn spawn_linked_and_unlinked_entity_finalize_backward() {
    test::<true>(
        true,
        true,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, true),
        false,
    );
}

#[test]
fn spawn_linked_and_unlinked_entities_finalize_forward() {
    test::<true>(
        true,
        false,
        Hierarchy::new_expected_despawned_nested(false, false, false),
        Hierarchy::new_expected_despawned_nested(true, true, true),
        true,
    );
}

#[test]
fn despawn_linked_and_unlinked_entity_finalize_forward() {
    test::<false>(
        true,
        true,
        Hierarchy::new_expected_despawned_nested(true, true, true),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        true,
    );
}

#[test]
fn despawn_linked_and_unlinked_entity_finalize_backward() {
    test::<false>(
        true,
        true,
        Hierarchy::new_expected_despawned_nested(true, true, true),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        false,
    );
}

#[test]
fn despawn_linked_and_unlinked_entities_finalize_forward() {
    test::<false>(
        true,
        false,
        Hierarchy::new_expected_despawned_nested(true, true, true),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        true,
    );
}

#[test]
fn despawn_linked_and_unlinked_entities_finalize_backward() {
    test::<false>(
        true,
        false,
        Hierarchy::new_expected_despawned_nested(true, true, true),
        Hierarchy::new_expected_despawned_nested(false, false, false),
        false,
    );
}
