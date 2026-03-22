use bevy_ecs::{
    hierarchy::{ChildOf, Children},
    spawn::SpawnRelated,
};

use crate::undo_redo::{IsRevDespawned, test::assert_undo_redo};

use super::*;

#[derive(Component, Debug, PartialEq)]
struct PlainComponent(u8);

#[derive(Resource, Debug, PartialEq)]
struct PlainResource(u8);

#[derive(Component, Debug, PartialEq)]
#[require(RequiredComponent1(1), RequiredComponent2(1))]
struct RequiringComponent(u8);

#[derive(Component, Debug, PartialEq)]
struct RequiredComponent1(u8);

#[derive(Component, Debug, PartialEq)]
struct RequiredComponent2(u8);

#[test]
fn toggle_component() {
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let mut buffer = InnerComponentBuffer {
        entity,
        buffer: Some(PlainComponent(1)),
        caller: MaybeLocation::caller(),
    };

    assert_eq!(
        buffer.toggle_component(&mut world).unwrap(),
        ToggleResult::Inserted
    );
    assert_eq!(
        world.get::<PlainComponent>(entity),
        Some(&PlainComponent(1))
    );
    assert_eq!(buffer.buffer, None);

    buffer.buffer = Some(PlainComponent(2));
    assert_eq!(
        buffer.toggle_component(&mut world).unwrap(),
        ToggleResult::Swapped
    );
    assert_eq!(
        world.get::<PlainComponent>(entity),
        Some(&PlainComponent(2))
    );
    assert_eq!(buffer.buffer, Some(PlainComponent(1)));

    buffer.buffer = None;
    assert_eq!(
        buffer.toggle_component(&mut world).unwrap(),
        ToggleResult::Taken
    );
    assert_eq!(world.get::<PlainComponent>(entity), None);
    assert_eq!(buffer.buffer, Some(PlainComponent(2)));

    buffer.buffer = None;
    assert_eq!(
        buffer.toggle_component(&mut world).unwrap(),
        ToggleResult::Noop
    );
    assert_eq!(world.get::<PlainComponent>(entity), None);
    assert_eq!(buffer.buffer, None);
}

#[test]
fn toggle_resource() {
    let mut world = World::new();
    let mut buffer = InnerResourceBuffer {
        buffer: Some(PlainResource(1)),
        caller: MaybeLocation::caller(),
    };

    assert_eq!(buffer.toggle_resource(&mut world), ToggleResult::Inserted);
    assert_eq!(
        world.get_resource::<PlainResource>(),
        Some(&PlainResource(1))
    );
    assert_eq!(buffer.buffer, None);

    buffer.buffer = Some(PlainResource(2));
    assert_eq!(buffer.toggle_resource(&mut world), ToggleResult::Swapped);
    assert_eq!(
        world.get_resource::<PlainResource>(),
        Some(&PlainResource(2))
    );
    assert_eq!(buffer.buffer, Some(PlainResource(1)));

    buffer.buffer = None;
    assert_eq!(buffer.toggle_resource(&mut world), ToggleResult::Taken);
    assert_eq!(world.get_resource::<PlainResource>(), None);
    assert_eq!(buffer.buffer, Some(PlainResource(2)));

    buffer.buffer = None;
    assert_eq!(buffer.toggle_resource(&mut world), ToggleResult::Noop);
    assert_eq!(world.get_resource::<PlainResource>(), None);
    assert_eq!(buffer.buffer, None);
}

#[test]
fn undo_new_required() {
    let mut world = World::new();
    let entity = world.spawn(PlainComponent(1)).id();
    let mut required = RevNewRequired::<PlainComponent> {
        entity,
        new_required: vec![world.register_component::<PlainComponent>()],
        caller: MaybeLocation::caller(),
        _p: PhantomData,
    };

    required.undo(&mut world);
    assert_eq!(world.get::<PlainComponent>(entity), None);
}

fn insert(mode: InsertMode) {
    let maybe_overwritten = match mode {
        InsertMode::Replace => PlainComponent(1),
        InsertMode::Keep => PlainComponent(2),
    };
    let mut world = World::new();
    let entity = world.spawn((PlainComponent(2), RequiredComponent2(2))).id();

    assert_undo_redo(
        &mut world,
        |world, not_log| {
            let mut entity = world.entity_mut(entity);
            (PlainComponent(1), RequiringComponent(1)).rev_insert(
                not_log,
                &mut entity,
                mode,
                MaybeLocation::caller(),
            );
            assert_eq!(entity.get::<PlainComponent>(), Some(&maybe_overwritten));
            assert_eq!(
                entity.get::<RequiringComponent>(),
                Some(&RequiringComponent(1))
            );
            assert_eq!(
                entity.get::<RequiredComponent1>(),
                Some(&RequiredComponent1(1))
            );
            assert_eq!(
                entity.get::<RequiredComponent2>(),
                Some(&RequiredComponent2(2))
            );
        },
        |world, _| {
            let entity = world.entity(entity);
            assert_eq!(entity.get::<PlainComponent>(), Some(&PlainComponent(2)));
            assert_eq!(entity.get::<RequiringComponent>(), None);
            assert_eq!(entity.get::<RequiredComponent1>(), None);
            assert_eq!(
                entity.get::<RequiredComponent2>(),
                Some(&RequiredComponent2(2))
            );
        },
        |world, _| {
            let entity = world.entity(entity);
            assert_eq!(entity.get::<PlainComponent>(), Some(&maybe_overwritten));
            assert_eq!(
                entity.get::<RequiringComponent>(),
                Some(&RequiringComponent(1))
            );
            assert_eq!(
                entity.get::<RequiredComponent1>(),
                Some(&RequiredComponent1(1))
            );
            assert_eq!(
                entity.get::<RequiredComponent2>(),
                Some(&RequiredComponent2(2))
            );
        },
    );
}

#[test]
fn insert_replace() {
    insert(InsertMode::Replace);
}

#[test]
fn insert_keep() {
    insert(InsertMode::Keep);
}

#[test]
fn remove() {
    let mut world = World::new();
    let entity = world.spawn(PlainComponent(1)).id();

    assert_undo_redo(
        &mut world,
        |world, not_log| {
            let mut entity = world.entity_mut(entity);
            <(PlainComponent, RequiringComponent)>::rev_remove(
                not_log,
                &mut entity,
                MaybeLocation::caller(),
            );
            assert_eq!(entity.get::<PlainComponent>(), None);
        },
        |world, _| {
            assert_eq!(
                world.get::<PlainComponent>(entity),
                Some(&PlainComponent(1))
            );
        },
        |world, _| {
            assert_eq!(world.get::<PlainComponent>(entity), None);
        },
    );
}

fn insert_related(one: bool) {
    let mut world = World::new();
    let parent = world.spawn_empty().id();

    assert_undo_redo(
        &mut world,
        |world, not_log| {
            let mut parent_mut = world.entity_mut(parent);
            if one {
                Children::spawn_one(()).rev_insert(
                    not_log,
                    &mut parent_mut,
                    InsertMode::Replace,
                    MaybeLocation::caller(),
                );
            } else {
                Children::spawn(vec![()]).rev_insert(
                    not_log,
                    &mut parent_mut,
                    InsertMode::Replace,
                    MaybeLocation::caller(),
                );
            }
            let child_id = *parent_mut.get::<Children>().unwrap().iter().next().unwrap();
            let child_ref = world.get_entity(child_id).unwrap();
            assert_eq!(child_ref.get::<ChildOf>(), Some(&ChildOf(parent)));
            assert!(!child_ref.is_rev_despawned());
            child_id
        },
        |world, child| {
            assert_eq!(world.get::<Children>(parent), None);
            let child_ref = world.get_entity(*child).unwrap();
            assert_eq!(child_ref.get::<ChildOf>(), None);
            assert!(child_ref.is_rev_despawned());
        },
        |world, child| {
            let child_id = *world
                .get::<Children>(parent)
                .unwrap()
                .iter()
                .next()
                .unwrap();
            assert_eq!(child_id, *child);
            let child_ref = world.get_entity(child_id).unwrap();
            assert_eq!(child_ref.get::<ChildOf>(), Some(&ChildOf(parent)));
            assert!(!child_ref.is_rev_despawned());
        },
    );
}

#[test]
fn insert_one_related() {
    insert_related(true);
}

#[test]
fn insert_many_related() {
    insert_related(false);
}
