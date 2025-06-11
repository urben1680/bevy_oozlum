use crate::panic_on_error_events;

use super::*;

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
#[require(Required<N>)]
struct Explicit<const N: u8>(u8);

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
struct Required<const N: u8>(u8);

fn setup() -> World {
    panic_on_error_events();
    let mut world = World::new();
    world.init_resource::<UndoRedoBuffer>();
    world.init_resource::<RevRelationship>();
    world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
    world
}

#[test]
fn add_related_with_no_prior_relationship() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let child = world.spawn_empty().id();
    let mut entity = world.spawn_empty();

    let parent = entity.rev_add_related::<ChildOf>(now, &[child]).id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child]))
    );

    buffer.undo(&mut world);
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.get::<ChildOf>(), None);
    assert_eq!(
        parent_ref
            .get::<Children>()
            .and_then(|children| children.first()),
        None
    );

    buffer.redo(&mut world);
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child]))
    );
}

#[test]
fn add_related_with_prior_relationship() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let child1 = world.spawn_empty().id();
    let child2 = world.spawn_empty().id();
    let mut entity = world.spawn_empty();
    entity.add_related::<ChildOf>(&[child1]);

    let parent = entity.rev_add_related::<ChildOf>(now, &[child2]).id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let [child1_ref, child2_ref, parent_ref] = world.entity([child1, child2, parent]);
    assert_eq!(child1_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(child2_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child1, child2]))
    );

    buffer.undo(&mut world);
    let [child1_ref, child2_ref, parent_ref] = world.entity([child1, child2, parent]);
    assert_eq!(child1_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(child2_ref.get::<ChildOf>(), None);
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child1]))
    );

    buffer.redo(&mut world);
    let [child1_ref, child2_ref, parent_ref] = world.entity([child1, child2, parent]);
    assert_eq!(child1_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(child2_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child1, child2]))
    );
}

#[test]
fn rev_clear_without_relationship() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity = world.spawn(Explicit::<1>(1));

    let entity = entity.rev_clear(now).id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit::<1>>(), None);
    assert_eq!(entity_ref.get::<Required::<1>>(), None);

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(entity_ref.get(), Some(&Required::<1>(0)));

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit::<1>>(), None);
    assert_eq!(entity_ref.get::<Required::<1>>(), None);
}

#[test]
fn rev_clear_with_relationship() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let parent = world.spawn_empty().id();
    let mut child = world.spawn(ChildOf(parent));

    let child = child.rev_clear(now).id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.get::<ChildOf>(), None);
    assert_eq!(parent_ref.get::<Children>(), None);

    buffer.undo(&mut world);
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child]))
    );

    buffer.redo(&mut world);
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.get::<ChildOf>(), None);
    assert_eq!(parent_ref.get::<Children>(), None);
}

#[test]
fn rev_clone_and_spawn_works() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let parent = world.spawn_empty().id();
    let mut child_mut = world.spawn((ChildOf(parent), Explicit::<1>(1)));

    let clone = child_mut.rev_clone_and_spawn(now);
    let child = child_mut.id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    let [clone_ref, child_ref, parent_ref] = world.entity([clone, child, parent]);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(clone_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(clone_ref.get(), Some(&Required::<1>(0)));
    assert_eq!(clone_ref.rev_is_despawned(), false);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(child_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(child_ref.get(), Some(&Required::<1>(0)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child, clone]))
    );

    buffer.undo(&mut world);
    let [clone_ref, child_ref, parent_ref] = world.entity([clone, child, parent]);
    assert_eq!(clone_ref.rev_is_despawned(), true);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(child_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(child_ref.get(), Some(&Required::<1>(0)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child]))
    );

    buffer.redo(&mut world);
    let [clone_ref, child_ref, parent_ref] = world.entity([clone, child, parent]);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(clone_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(clone_ref.get(), Some(&Required::<1>(0)));
    assert_eq!(clone_ref.rev_is_despawned(), false);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(child_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(child_ref.get(), Some(&Required::<1>(0)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child, clone]))
    );
}

#[test]
fn rev_clone_components_without_relationship() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let target = world.spawn((Explicit::<2>(20), Required::<2>(20))).id();
    let mut source_mut = world.spawn((
        Explicit::<1>(1),
        Required::<1>(1),
        Explicit::<2>(2),
        Required::<2>(2),
        Explicit::<3>(3),
        Required::<3>(3),
    ));

    source_mut.rev_clone_components::<(Explicit<1>, Explicit<2>)>(now, target);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let target_ref = world.entity(target);
    assert_eq!(target_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(target_ref.get(), Some(&Required::<1>(1)));
    assert_eq!(target_ref.get(), Some(&Explicit::<2>(2)));
    assert_eq!(target_ref.get(), Some(&Required::<2>(20)));
    assert_eq!(target_ref.get::<Explicit<3>>(), None);
    assert_eq!(target_ref.get::<Required<3>>(), None);

    buffer.undo(&mut world);
    let target_ref = world.entity(target);
    assert_eq!(target_ref.get::<Explicit<1>>(), None);
    assert_eq!(target_ref.get::<Required<1>>(), None);
    assert_eq!(target_ref.get(), Some(&Explicit::<2>(20)));
    assert_eq!(target_ref.get(), Some(&Required::<2>(20)));
    assert_eq!(target_ref.get::<Explicit<3>>(), None);
    assert_eq!(target_ref.get::<Required<3>>(), None);

    buffer.redo(&mut world);
    let target_ref = world.entity(target);
    assert_eq!(target_ref.get(), Some(&Explicit::<1>(1)));
    assert_eq!(target_ref.get(), Some(&Required::<1>(1)));
    assert_eq!(target_ref.get(), Some(&Explicit::<2>(2)));
    assert_eq!(target_ref.get(), Some(&Required::<2>(20)));
    assert_eq!(target_ref.get::<Explicit<3>>(), None);
    assert_eq!(target_ref.get::<Required<3>>(), None);
}

// todo rev_clone_components_with_relationship

#[test]
fn rev_despawn_with_relationship() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let parent = world.spawn_empty().id();
    let child_mut = world.spawn(ChildOf(parent));
    let child = child_mut.id();
    child_mut.rev_despawn(now);

    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.rev_is_despawned(), true);
    assert!(
        parent_ref
            .get::<Children>()
            .is_none_or(|children| children.is_empty())
    );

    buffer.undo(&mut world);
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.rev_is_despawned(), false);
    assert_eq!(child_ref.get(), Some(&ChildOf(parent)));
    assert_eq!(
        parent_ref.get(),
        Some(&Children::from_collection_risky(vec![child]))
    );

    buffer.redo(&mut world);
    let [child_ref, parent_ref] = world.entity([child, parent]);
    assert_eq!(child_ref.rev_is_despawned(), true);
    assert!(
        parent_ref
            .get::<Children>()
            .is_none_or(|children| children.is_empty())
    );
}

/*
to test:


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
- rev_replace_related
- rev_replace_related_with_difference
- rev_retain
- rev_take
- rev_with_related
*/
