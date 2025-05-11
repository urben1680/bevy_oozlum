/*
use std::sync::atomic::{AtomicBool, Ordering};

use crate::panic_on_error_events;

use super::*;

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
#[require(Required1)]
struct Explicit1(u8);

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
#[require(Required2)]
struct Explicit2(u8);

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
struct Required1(u8);

#[derive(Component, PartialEq, Debug, Default, Copy, Clone)]
struct Required2(u8);

#[derive(Resource, PartialEq, Debug, Default, Copy, Clone)]
struct TestRes(u8);

fn setup() -> World {
    panic_on_error_events();
    let mut world = World::new();
    world.init_resource::<UndoRedoBuffer>();
    world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
    world
}

fn rev_despawned_entity_clear_buffer(world: &mut World) -> Entity {
    let entity_mut = world.spawn_empty();
    let entity = entity_mut.id();
    entity_mut.rev_despawn();
    world.insert_resource(UndoRedoBuffer::default());
    entity
}

mod buffer_at_now {
    use super::*;

    fn inner(
        c: impl FnOnce(&mut World, Entity, ComponentId) -> Result<Option<Entity>, RevMetaOrEntityError>,
    ) {
        let mut world = setup();
        let explicit_id = world.register_component::<Explicit1>();
        let entity = world.spawn((Explicit1(1), Required1(1))).id();
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        let buffer_entity = c(&mut world, entity, explicit_id)
            .expect("should be Ok")
            .expect("should be Some");
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Explicit1>(entity), None);
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        buffer.undo(&mut world);
        assert_eq!(world.get::<Explicit1>(buffer_entity), None);
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        buffer.redo(&mut world);
        assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Explicit1>(entity), None);
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));
    }

    #[test]
    fn buffer_components_buffers() {
        inner(|world, entity, component_id| {
            world.buffer_components(entity, BufferAt::Now, &[component_id])
        });
    }

    #[test]
    fn buffer_components_cached_buffers() {
        inner(|world, entity, component_id| {
            let components = |_: &mut World| {
                static ASSERT_CACHED: AtomicBool = AtomicBool::new(true);
                assert!(ASSERT_CACHED.fetch_not(Ordering::Relaxed));
                (BufferAt::Now, [component_id])
            };

            let another_entity = world.spawn_empty().id();
            world
                .buffer_components_cached(another_entity, (), components)
                .expect("should be Ok")
                .expect("should be Some");

            world.buffer_components_cached(entity, (), components)
        });
    }

    #[test]
    fn buffer_bundle_buffers() {
        inner(|world, entity, component_id| {
            let bundle = world.register_dynamic_bundle(&[component_id]).id();
            world.buffer_bundle(entity, BufferAt::Now, bundle)
        })
    }
}

mod buffer_at_undo {
    use super::*;

    fn inner(
        c: impl FnOnce(&mut World, Entity, ComponentId) -> Result<Option<Entity>, RevMetaOrEntityError>,
    ) {
        let mut world = setup();
        let explicit_id = world.register_component::<Explicit1>();
        let entity = world.spawn((Explicit1(1), Required1(1))).id();
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        let result = c(&mut world, entity, explicit_id);
        assert_eq!(result, Ok(None));
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        buffer.undo(&mut world);
        assert_eq!(world.get::<Explicit1>(entity), None);
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        buffer.redo(&mut world);
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));
    }

    #[test]
    fn buffer_components_buffers() {
        inner(|world, entity, component_id| {
            world.buffer_components(entity, BufferAt::Undo, &[component_id])
        });
    }

    #[test]
    fn buffer_components_cached_buffers() {
        inner(|world, entity, component_id| {
            let components = |_: &mut World| {
                static ASSERT_CACHED: AtomicBool = AtomicBool::new(true);
                assert!(ASSERT_CACHED.fetch_not(Ordering::Relaxed));
                (BufferAt::Undo, [component_id])
            };

            let another_entity = world.spawn_empty().id();
            let result = world.buffer_components_cached(another_entity, (), components);
            assert_eq!(result, Ok(None));

            world.buffer_components_cached(entity, (), components)
        });
    }

    #[test]
    fn buffer_bundle_buffers() {
        inner(|world, entity, component_id| {
            let bundle = world.register_dynamic_bundle(&[component_id]).id();
            world.buffer_bundle(entity, BufferAt::Undo, bundle)
        })
    }
}

mod buffer_at_now_and_undo {
    use super::*;

    fn inner(
        c: impl FnOnce(&mut World, Entity, ComponentId) -> Result<Option<Entity>, RevMetaOrEntityError>,
    ) {
        let mut world = setup();
        let explicit_id = world.register_component::<Explicit1>();
        let entity = world.spawn((Explicit1(1), Required1(1))).id();
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        let buffer_entity = c(&mut world, entity, explicit_id)
            .expect("should be Ok")
            .expect("should be Some");
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();
        world.entity_mut(entity).insert(Explicit1(2));

        assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(2)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        buffer.undo(&mut world);
        assert_eq!(world.get::<Explicit1>(buffer_entity), None);
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));

        buffer.redo(&mut world);
        assert_eq!(world.get::<Explicit1>(buffer_entity), Some(&Explicit1(1)));
        assert_eq!(world.get::<Explicit1>(entity), Some(&Explicit1(2)));
        assert_eq!(world.get::<Required1>(entity), Some(&Required1(1)));
    }

    #[test]
    fn buffer_components_buffers() {
        inner(|world, entity, component_id| {
            world.buffer_components(entity, BufferAt::NowAndUndo, &[component_id])
        });
    }

    #[test]
    fn buffer_components_cached_buffers() {
        inner(|world, entity, component_id| {
            let components = |_: &mut World| {
                static ASSERT_CACHED: AtomicBool = AtomicBool::new(true);
                assert!(ASSERT_CACHED.fetch_not(Ordering::Relaxed));
                (BufferAt::NowAndUndo, [component_id])
            };

            let another_entity = world.spawn_empty().id();
            world
                .buffer_components_cached(another_entity, (), components)
                .expect("should be Ok")
                .expect("should be Some");

            world.buffer_components_cached(entity, (), components)
        });
    }

    #[test]
    fn buffer_bundle_buffers() {
        inner(|world, entity, component_id| {
            let bundle = world.register_dynamic_bundle(&[component_id]).id();
            world.buffer_bundle(entity, BufferAt::NowAndUndo, bundle)
        })
    }
}

#[test]
fn buffer_fails_on_invalid() {
    let mut world = setup();
    let explicit_id = world.register_component::<Explicit1>();
    let bundle = world.register_dynamic_bundle(&[explicit_id]).id();

    for at in [BufferAt::Now, BufferAt::Undo, BufferAt::NowAndUndo] {
        let assertion = |result| {
            if !matches!(result, Err(RevMetaOrEntityError::EntityDoesNotExistError(_))) {
                panic!("at: {at:?}, result: {result:?}");
            }
        };

        assertion(world.buffer_components(Entity::PLACEHOLDER, at, &[explicit_id]));
        assertion(world.buffer_components_cached(Entity::PLACEHOLDER, at, |_| (at, [explicit_id])));
        assertion(world.buffer_bundle(Entity::PLACEHOLDER, at, bundle));
    }
}

#[test]
fn buffer_fails_on_rev_despawned() {
    let mut world = setup();
    let explicit_id = world.register_component::<Explicit1>();
    let bundle = world.register_dynamic_bundle(&[explicit_id]).id();
    let entity = rev_despawned_entity_clear_buffer(&mut world);

    for at in [BufferAt::Now, BufferAt::Undo, BufferAt::NowAndUndo] {
        let assertion = |result| {
            if !matches!(result, Err(RevMetaOrEntityError::EntityRevDespawnedError(_))) {
                panic!("at: {at:?}, result: {result:?}");
            }
        };

        assertion(world.buffer_components(entity, at, &[explicit_id]));
        assertion(world.buffer_components_cached(entity, at, |_| (at, [explicit_id])));
        assertion(world.buffer_bundle(entity, at, bundle));
    }
}

#[test]
fn rev_try_despawn_despawns() {
    let mut world = setup();
    let entity = world.spawn_empty().id();
    let result = world.rev_try_despawn(entity);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert!(matches!(result, Ok(())), "{result:?}");
    assert_eq!(world.entity(entity).rev_is_despawned(), true);

    buffer.undo(&mut world);
    assert_eq!(world.entity(entity).rev_is_despawned(), false);

    buffer.redo(&mut world);
    assert_eq!(world.entity(entity).rev_is_despawned(), true);
}

#[test]
fn rev_try_despawn_fails_at_invalid() {
    let mut world = setup();
    let result = world.rev_try_despawn(Entity::PLACEHOLDER);

    assert!(
        matches!(result, Err(RevMetaOrEntityError::EntityDoesNotExistError(_))),
        "{result:?}"
    );
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}

#[test]
fn rev_try_despawn_fails_at_rev_despawned() {
    let mut world = setup();
    let entity = rev_despawned_entity_clear_buffer(&mut world);
    let result = world.rev_try_despawn(entity);

    assert!(
        matches!(result, Err(RevMetaOrEntityError::EntityRevDespawnedError(_))),
        "{result:?}"
    );
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}

#[test]
fn rev_try_insert_batch_inserts() {
    let mut world = setup();
    let entity1 = world.spawn((Explicit1(10), Required1(10))).id();
    let entity2 = world.spawn((Explicit2(20), Required2(20))).id();

    let result = world.rev_try_insert_batch([
        (entity1, (Explicit1(30), Explicit2(30))),
        (entity2, (Explicit1(40), Explicit2(40))),
    ]);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert!(matches!(result, Ok(())), "{result:?}");
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(30)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get(), Some(&Explicit1(40)));
    assert_eq!(ref2.get(), Some(&Required1(0)));
    assert_eq!(ref2.get(), Some(&Explicit2(40)));
    assert_eq!(ref2.get(), Some(&Required2(20)));

    buffer.undo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get::<Explicit2>(), None);
    assert_eq!(ref1.get::<Required2>(), None);

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get(), Some(&Explicit2(20)));
    assert_eq!(ref2.get(), Some(&Required2(20)));

    buffer.redo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(30)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get(), Some(&Explicit1(40)));
    assert_eq!(ref2.get(), Some(&Required1(0)));
    assert_eq!(ref2.get(), Some(&Explicit2(40)));
    assert_eq!(ref2.get(), Some(&Required2(20)));
}

#[test]
fn rev_try_insert_batch_fails_partially_with_invalid_and_rev_despawned_entity() {
    let mut world = setup();
    let entity1 = world.spawn((Explicit1(10), Required1(10))).id();
    let entity2 = rev_despawned_entity_clear_buffer(&mut world);

    let result = world.rev_try_insert_batch([
        (entity1, (Explicit1(30), Explicit2(30))),
        (entity2, (Explicit1(40), Explicit2(40))),
        (Entity::PLACEHOLDER, (Explicit1(50), Explicit2(50))),
    ]);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    let Err(RevMetaOrEntitiesError::RevEntitiesError {
        invalid,
        rev_despawned,
    }) = result
    else {
        panic!("{result:?}");
    };
    let invalid: Vec<Entity> = invalid.into_iter().map(|err| err.entity).collect();
    let rev_despawned: Vec<Entity> = rev_despawned.into_iter().map(|err| err.entity).collect();
    assert_eq!(invalid, [Entity::PLACEHOLDER]);
    assert_eq!(rev_despawned, [entity2]);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(30)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get::<Explicit2>(), None);
    assert_eq!(ref2.get::<Required2>(), None);

    buffer.undo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get::<Explicit2>(), None);
    assert_eq!(ref1.get::<Required2>(), None);

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get::<Explicit2>(), None);
    assert_eq!(ref2.get::<Required2>(), None);

    buffer.redo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(30)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get::<Explicit2>(), None);
    assert_eq!(ref2.get::<Required2>(), None);
}

#[test]
fn rev_try_insert_batch_if_new_inserts() {
    let mut world = setup();
    let entity1 = world.spawn((Explicit1(10), Required1(10))).id();
    let entity2 = world.spawn((Explicit2(20), Required2(20))).id();

    let result = world.rev_try_insert_batch_if_new([
        (entity1, (Explicit1(30), Explicit2(30))),
        (entity2, (Explicit1(40), Explicit2(40))),
    ]);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    assert!(matches!(result, Ok(())), "{result:?}");
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get(), Some(&Explicit1(40)));
    assert_eq!(ref2.get(), Some(&Required1(0)));
    assert_eq!(ref2.get(), Some(&Explicit2(20)));
    assert_eq!(ref2.get(), Some(&Required2(20)));

    buffer.undo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get::<Explicit2>(), None);
    assert_eq!(ref1.get::<Required2>(), None);

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get(), Some(&Explicit2(20)));
    assert_eq!(ref2.get(), Some(&Required2(20)));

    buffer.redo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get(), Some(&Explicit1(40)));
    assert_eq!(ref2.get(), Some(&Required1(0)));
    assert_eq!(ref2.get(), Some(&Explicit2(20)));
    assert_eq!(ref2.get(), Some(&Required2(20)));
}

#[test]
fn rev_try_insert_batch_if_new_fails_partially_with_invalid_and_rev_despawned_entity() {
    let mut world = setup();
    let entity1 = world.spawn((Explicit1(10), Required1(10))).id();
    let entity2 = rev_despawned_entity_clear_buffer(&mut world);

    let result = world.rev_try_insert_batch_if_new([
        (entity1, (Explicit1(30), Explicit2(30))),
        (entity2, (Explicit1(40), Explicit2(40))),
        (Entity::PLACEHOLDER, (Explicit1(50), Explicit2(50))),
    ]);
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    let Err(RevMetaOrEntitiesError::RevEntitiesError {
        invalid,
        rev_despawned,
    }) = result
    else {
        panic!("{result:?}");
    };
    let invalid: Vec<Entity> = invalid.into_iter().map(|err| err.entity).collect();
    let rev_despawned: Vec<Entity> = rev_despawned.into_iter().map(|err| err.entity).collect();
    assert_eq!(invalid, [Entity::PLACEHOLDER]);
    assert_eq!(rev_despawned, [entity2]);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get::<Explicit2>(), None);
    assert_eq!(ref2.get::<Required2>(), None);

    buffer.undo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get::<Explicit2>(), None);
    assert_eq!(ref1.get::<Required2>(), None);

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get::<Explicit2>(), None);
    assert_eq!(ref2.get::<Required2>(), None);

    buffer.redo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);

    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(10)));
    assert_eq!(ref1.get(), Some(&Explicit2(30)));
    assert_eq!(ref1.get(), Some(&Required2(0)));

    assert_eq!(ref2.get::<Explicit1>(), None);
    assert_eq!(ref2.get::<Required1>(), None);
    assert_eq!(ref2.get::<Explicit2>(), None);
    assert_eq!(ref2.get::<Required2>(), None);
}

#[test]
fn rev_spawn_batch_spawns() {
    let mut world = setup();
    let entities = world.rev_spawn_batch([Explicit1(10), Explicit1(20)]);
    assert_eq!(entities.len(), 2);
    let entity1 = entities[0];
    let entity2 = entities[1];
    let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

    let [ref1, ref2] = world.entity([entity1, entity2]);
    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(0)));
    assert_eq!(ref1.rev_is_despawned(), false);
    assert_eq!(ref2.get(), Some(&Explicit1(20)));
    assert_eq!(ref2.get(), Some(&Required1(0)));
    assert_eq!(ref2.rev_is_despawned(), false);

    buffer.undo(&mut world);
    assert_eq!(world.entity(entity1).rev_is_despawned(), true);
    assert_eq!(world.entity(entity2).rev_is_despawned(), true);

    buffer.redo(&mut world);
    let [ref1, ref2] = world.entity([entity1, entity2]);
    assert_eq!(ref1.get(), Some(&Explicit1(10)));
    assert_eq!(ref1.get(), Some(&Required1(0)));
    assert_eq!(ref1.rev_is_despawned(), false);
    assert_eq!(ref2.get(), Some(&Explicit1(20)));
    assert_eq!(ref2.get(), Some(&Required1(0)));
    assert_eq!(ref2.rev_is_despawned(), false);
}

#[test]
fn rev_init_resource_on_unexisting_inits_resource() {
    let mut world = setup();

    world.rev_init_resource::<TestRes>();
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
    world.init_resource::<TestRes>();
    world.rev_init_resource::<TestRes>();
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}

#[test]
fn rev_insert_resource_on_unexisting_inserts() {
    let mut world = setup();

    world.rev_insert_resource(TestRes(10));
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

    world.insert_resource(TestRes(10));
    world.rev_insert_resource(TestRes(20));
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

    world.insert_resource(TestRes(10));
    let out = world.rev_remove_resource::<TestRes, _>(|r| *r);
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
    let out = world.rev_remove_resource::<TestRes, _>(|r| *r);
    assert_eq!(out, None);
    assert!(world.resource::<UndoRedoBuffer>().is_empty());
}
*/
