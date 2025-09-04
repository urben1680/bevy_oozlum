use crate::{
    meta::RevMeta, panic_on_error_events, undo_redo::RevIsDespawned, undo_redo::UndoRedoBuffer,
};

use super::*;

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
#[require(Required<N>)]
struct Explicit<const N: u8>(u8);

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
struct Required<const N: u8>(u8);

fn setup() -> World {
    panic_on_error_events();
    let mut world = World::new();
    world.init_resource::<RevDespawnCleaner>();
    world.init_resource::<UndoRedoBuffer>();
    world.init_resource::<BundleIdOfOpCache>();
    world.insert_resource(RevMeta::running_new());
    world.register_disabling_component::<RevDespawned>();
    world
}

#[test]
fn rev_clear() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity_mut = world.spawn(Explicit::<1>(0));

    let entity = rev_try_clear_with_caller(&mut entity_mut, now, MaybeLocation::caller())
        .unwrap()
        .id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit::<1>>(), None);
    assert_eq!(entity_ref.get::<Required::<1>>(), None);

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get(), Some(&Explicit::<1>(0)));
    assert_eq!(entity_ref.get(), Some(&Required::<1>(0)));

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit::<1>>(), None);
    assert_eq!(entity_ref.get::<Required::<1>>(), None);
}

#[test]
fn rev_despawn_single() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let entity_mut = world.spawn_empty();
    let entity = entity_mut.id();

    rev_try_despawn_single_with_caller(entity_mut, now, MaybeLocation::caller()).unwrap();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert!(entity_ref.is_rev_despawned());

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert!(!entity_ref.is_rev_despawned());

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert!(entity_ref.is_rev_despawned());
}

#[test]
fn rev_insert() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity_mut = world.spawn((Explicit::<1>(1), Required::<1>(1)));

    let entity = entity_mut
        .rev_insert(now, (Explicit::<1>(0), Explicit::<2>(0)))
        .id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(1)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), None);
    assert_eq!(entity_ref.get::<Required<2>>(), None);

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
}

#[test]
fn rev_insert_if_new() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity_mut = world.spawn((Explicit::<1>(1), Required::<1>(1), Required::<2>(1)));

    let entity = entity_mut
        .rev_insert_if_new(now, (Explicit::<1>(0), Explicit::<2>(0), Explicit::<3>(0)))
        .id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(1)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<3>>(), Some(&Required(0)));

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(1)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), None);
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(1)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(1)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<3>>(), Some(&Required(0)));
}

#[test]
fn rev_remove() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity_mut = world.spawn((Explicit::<1>(0), Explicit::<2>(0)));

    let entity = entity_mut
        .rev_remove::<(Explicit<2>, Explicit<3>)>(now)
        .id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), None);
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), None);
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);
}

#[test]
fn rev_remove_with_requires() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity_mut = world.spawn((Explicit::<1>(0), Explicit::<2>(0)));

    let entity = entity_mut
        .rev_remove_with_requires::<(Explicit<2>, Explicit<3>)>(now)
        .id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), None);
    assert_eq!(entity_ref.get::<Required<2>>(), None);
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), None);
    assert_eq!(entity_ref.get::<Required<2>>(), None);
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), None);
}

#[test]
fn rev_retain() {
    let mut world = setup();
    let now = world.resource::<RevMeta>().non_log_now().unwrap();
    let mut entity_mut = world.spawn((Explicit::<1>(0), Explicit::<2>(0), Explicit::<3>(0)));

    let entity = entity_mut
        .rev_retain::<(Explicit<2>, Required<3>, Explicit<4>)>(now)
        .id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), None);
    assert_eq!(entity_ref.get::<Required<1>>(), None);
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<4>>(), None);
    assert_eq!(entity_ref.get::<Required<4>>(), None);

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<1>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<3>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<4>>(), None);
    assert_eq!(entity_ref.get::<Required<4>>(), None);

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.get::<Explicit<1>>(), None);
    assert_eq!(entity_ref.get::<Required<1>>(), None);
    assert_eq!(entity_ref.get::<Explicit<2>>(), Some(&Explicit(0)));
    assert_eq!(entity_ref.get::<Required<2>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<3>>(), None);
    assert_eq!(entity_ref.get::<Required<3>>(), Some(&Required(0)));
    assert_eq!(entity_ref.get::<Explicit<4>>(), None);
    assert_eq!(entity_ref.get::<Required<4>>(), None);
}

// todo: test noop situations
