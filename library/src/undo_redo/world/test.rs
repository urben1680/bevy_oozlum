use super::*;
use crate::{
    meta::RevMeta,
    panic_on_error_events,
    undo_redo::{IsRevDespawned, RevDespawned, UndoRedoBuffer},
};
use bevy_ecs::component::Component;

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
#[require(Required)]
struct Explicit(u8);

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
struct Required(u8);

#[derive(Resource, PartialEq, Debug, Default, Copy, Clone)]
struct TestRes(u8);

fn setup() -> World {
    panic_on_error_events();
    let mut world = World::new();
    world.init_resource::<UndoRedoBuffer>();
    world.insert_resource(RevMeta::running_new());
    world.register_disabling_component::<RevDespawned>();
    world
}
/*
#[test]
fn rev_init_resource_on_unexisting_inits_resource() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();

    world.rev_init_resource::<TestRes>(past_len);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert_eq!(world.get_resource(), Some(&TestRes(0)));
    buffer.undo(&mut world);
    assert_eq!(world.get_resource::<TestRes>(), None);
    buffer.redo(&mut world);
    assert_eq!(world.get_resource(), Some(&TestRes(0)));
}

#[test]
fn rev_init_resource_on_existing_noop() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    world.init_resource::<TestRes>();
    world.rev_init_resource::<TestRes>(past_len);
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}

#[test]
fn rev_insert_resource_on_unexisting_inserts() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();

    world.rev_insert_resource(past_len, TestRes(10));
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert_eq!(world.get_resource(), Some(&TestRes(10)));
    buffer.undo(&mut world);
    assert_eq!(world.get_resource::<TestRes>(), None);
    buffer.redo(&mut world);
    assert_eq!(world.get_resource(), Some(&TestRes(10)));
}

#[test]
fn rev_insert_resource_on_existing_overwrites() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();

    world.insert_resource(TestRes(10));
    world.rev_insert_resource(past_len, TestRes(20));
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert_eq!(world.get_resource(), Some(&TestRes(20)));
    buffer.undo(&mut world);
    assert_eq!(world.get_resource(), Some(&TestRes(10)));
    buffer.redo(&mut world);
    assert_eq!(world.get_resource(), Some(&TestRes(20)));
}

#[test]
fn rev_remove_resource_on_existing_removes() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();

    world.insert_resource(TestRes(10));
    let out = world.rev_remove_resource::<TestRes, _>(past_len, |r| *r);
    assert_eq!(out, Some(TestRes(10)));
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert_eq!(world.get_resource::<TestRes>(), None);
    buffer.undo(&mut world);
    assert_eq!(world.get_resource(), Some(&TestRes(10)));
    buffer.redo(&mut world);
    assert_eq!(world.get_resource::<TestRes>(), None);
}

#[test]
fn rev_remove_resource_on_unexisting_noop() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let out = world.rev_remove_resource::<TestRes, _>(past_len, |r| *r);
    assert_eq!(out, None);
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}

#[test]
fn rev_spawn_spawns() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let entity = world.rev_spawn(past_len, Required(0)).id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.is_rev_despawned(), false);
    assert_eq!(entity_ref.get(), Some(&Required(0)));

    buffer.undo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.is_rev_despawned(), true);
    assert_eq!(entity_ref.get::<Required>(), None);

    buffer.redo(&mut world);
    let entity_ref = world.entity(entity);
    assert_eq!(entity_ref.is_rev_despawned(), false);
    assert_eq!(entity_ref.get(), Some(&Required(0)));
}

#[test]
fn rev_spawn_batch_spawns() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let entities = world.rev_spawn_batch(past_len, [Explicit(10), Explicit(20)]);
    assert_eq!(entities.len(), 2);
    let entity1 = entities[0];
    let entity2 = entities[1];
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    let [ref1, ref2] = world.entity([entity1, entity2]);
    assert_eq!(ref1.get(), Some(&Explicit(10)));
    assert_eq!(ref1.get(), Some(&Required(0)));
    assert_eq!(ref1.is_rev_despawned(), false);
    assert_eq!(ref2.get(), Some(&Explicit(20)));
    assert_eq!(ref2.get(), Some(&Required(0)));
    assert_eq!(ref2.is_rev_despawned(), false);

    buffer.undo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);
    assert_eq!(ref1.get::<Explicit>(), None);
    assert_eq!(ref1.get::<Required>(), None);
    assert_eq!(ref1.is_rev_despawned(), true);
    assert_eq!(ref2.get::<Explicit>(), None);
    assert_eq!(ref2.get::<Required>(), None);
    assert_eq!(ref2.is_rev_despawned(), true);

    buffer.redo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);
    assert_eq!(ref1.get(), Some(&Explicit(10)));
    assert_eq!(ref1.get(), Some(&Required(0)));
    assert_eq!(ref1.is_rev_despawned(), false);
    assert_eq!(ref2.get(), Some(&Explicit(20)));
    assert_eq!(ref2.get(), Some(&Required(0)));
    assert_eq!(ref2.is_rev_despawned(), false);
}

#[test]
fn rev_spawn_empty_spawns() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let entity = world.rev_spawn_empty(past_len).id();
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert_eq!(world.entity(entity).is_rev_despawned(), false);

    buffer.undo(&mut world);
    assert_eq!(world.entity(entity).is_rev_despawned(), true);

    buffer.redo(&mut world);
    assert_eq!(world.entity(entity).is_rev_despawned(), false);
}

#[test]
fn rev_try_despawn_single_despawns() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let entity = world.spawn_empty().id();
    let result = world.rev_try_despawn_single(past_len, entity);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert!(matches!(result, Ok(())), "{result:?}");
    assert_eq!(world.entity(entity).is_rev_despawned(), true);

    buffer.undo(&mut world);
    assert_eq!(world.entity(entity).is_rev_despawned(), false);

    buffer.redo(&mut world);
    assert_eq!(world.entity(entity).is_rev_despawned(), true);
}

#[test]
fn rev_try_despawn_single_fails_at_invalid() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let result = world.rev_try_despawn_single(past_len, Entity::PLACEHOLDER);

    assert!(
        matches!(result, Err(RevEntityError::EntityNotSpawnedError(_))),
        "{result:?}"
    );
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}

#[test]
fn rev_try_despawn_single_fails_at_rev_despawned() {
    let mut world = setup();
    let past_len = world.resource::<RevMeta>().meta_past_len();
    let entity = world.spawn(RevDespawned).id();
    let result = world.rev_try_despawn_single(past_len, entity);

    assert!(
        matches!(result, Err(RevEntityError::EntityRevDespawnedError(_))),
        "{result:?}"
    );
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}
    */
