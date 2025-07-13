use std::any::TypeId;

use bevy::ecs::{
    bundle::{Bundle, BundleFromComponents, InsertMode, NoBundleEffect},
    component::Component,
    entity::Entity,
    world::{EntityWorldMut, World},
};

use variadics_please::all_tuples;

use crate::{
    meta::NonLogNow,
    prelude::{UndoRedo, UndoRedoSwap},
    undo_redo::{BuffersUndoRedo, RevEntityWorldMut},
};

// todo: move to entity_world
// todo: test unchecked methods

pub(crate) fn rev_remove<T: PartialOp<Marker>, Marker>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
) {
    T::apply_op::<(), (), (), _, _, _>(entity, now, RevOp::Remove);
}

pub(crate) fn rev_remove_unchecked<T: BundleFromComponents + Bundle>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
) {
    let id = entity.id();
    entity.redo_and_buffer(
        now,
        RevRemove::<T> {
            state: None,
            entity: id,
        },
    );
}

pub(crate) fn rev_insert<T: PartialOp<Marker>, Marker>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    value: T,
    insert_mode: InsertMode,
) {
    T::apply_op::<(), (), _, _, _, _>(entity, now, RevOp::Insert(value, insert_mode));
}

pub(crate) fn rev_insert_unchecked<
    Insert: BundleFromComponents + Bundle,
    NewRequired: BundleFromComponents + Bundle,
    Overwrite: BundleFromComponents + Bundle,
>(
    entity: &mut EntityWorldMut,
    now: NonLogNow,
    value: Insert,
) {
    let id = entity.id();
    if TypeId::of::<Overwrite>() == TypeId::of::<()>() {
        entity.insert(value).buffer_undo_redo(
            now,
            UndoRedoSwap(RevRemove::<(Insert, NewRequired)> {
                state: None,
                entity: id,
            }),
        );
    } else {
        let overwrite = entity.take::<Overwrite>().expect("todo");
        entity.insert(value).buffer_undo_redo(
            now,
            RevInsert::<(Insert, NewRequired), _> {
                state: Swap::Overwrite(overwrite),
                entity: id,
            },
        );
    }
}

pub(crate) struct RevRemove<T> {
    pub(crate) state: Option<T>,
    pub(crate) entity: Entity,
}

impl<T: Bundle + BundleFromComponents> UndoRedo for RevRemove<T> {
    fn undo(&mut self, world: &mut World) {
        let Some(value) = self.state.take() else {
            unreachable!()
        };
        world
            .get_entity_mut(self.entity)
            .expect("todo")
            .insert(value);
    }
    fn redo(&mut self, world: &mut World) {
        self.state = world
            .get_entity_mut(self.entity)
            .expect("todo")
            .take::<T>()
            .map(Some)
            .expect("todo");
    }
}

pub(crate) struct RevInsert<Insert, Overwrite> {
    pub(crate) state: Swap<Insert, Overwrite>,
    pub(crate) entity: Entity,
}

enum Swap<Insert, Overwrite> {
    Insert(Insert),
    Overwrite(Overwrite),
}

impl<Insert: Bundle + BundleFromComponents, Overwrite: Bundle + BundleFromComponents> UndoRedo
    for RevInsert<Insert, Overwrite>
{
    fn undo(&mut self, world: &mut World) {
        let mut entity_mut = world.get_entity_mut(self.entity).expect("todo");
        let insert = entity_mut
            .take::<Insert>()
            .map(Swap::Insert)
            .expect("todo");
        let Swap::Overwrite(overwrite) = core::mem::replace(&mut self.state, insert) else {
            unreachable!()
        };
        entity_mut.insert(overwrite);
    }
    fn redo(&mut self, world: &mut World) {
        let mut entity_mut = world.get_entity_mut(self.entity).expect("todo");
        let overwrite = entity_mut
            .take::<Overwrite>()
            .map(Swap::Overwrite)
            .expect("todo");
        let Swap::Insert(insert) = core::mem::replace(&mut self.state, overwrite) else {
            unreachable!()
        };
        entity_mut.insert(insert);
    }
}

#[doc(hidden)]
pub enum RevOp<T> {
    Remove,
    Insert(T, InsertMode),
}

pub trait PartialOp<Marker>: BundleFromComponents + Bundle<Effect: NoBundleEffect> {
    /// If `Self` consists only of `()`, this is `true`. Otherwise, this is `false`.
    const EMPTY: bool;

    #[doc(hidden)]
    fn apply_op<
        Unpacked: PartialOp<UnpackedMarker>,
        Queue: PartialOp<QueueMarker>,
        Op: PartialOp<OpMarker>,
        UnpackedMarker,
        QueueMarker,
        OpMarker,
    >(
        entity: &mut EntityWorldMut,
        now: NonLogNow,
        op: RevOp<Op>,
    );
}
    
impl<T: Bundle + BundleFromComponents> RevOp<T> {
    fn apply_op<Unpacked, Marker, UnpackedMarker>(self, entity: &mut EntityWorldMut, now: NonLogNow)
    where
        T: PartialOp<Marker>,
        Unpacked: PartialOp<UnpackedMarker>,
    {
        let id = entity.id();
        match self {
            Self::Remove => {
                if !Unpacked::EMPTY {
                    entity.redo_and_buffer(
                        now,
                        RevRemove::<Unpacked> {
                            state: None,
                            entity: id,
                        },
                    )
                }
            }
            Self::Insert(value, InsertMode::Replace) => {
                if !T::EMPTY {
                    if Unpacked::EMPTY {
                        entity.redo_and_buffer(
                            now,
                            UndoRedoSwap(RevRemove {
                                state: Some(value),
                                entity: id,
                            }),
                        )
                    } else {
                        entity.redo_and_buffer(
                            now,
                            RevInsert::<_, Unpacked> {
                                state: Swap::Insert(value),
                                entity: id,
                            },
                        )
                    }
                }
            }
            Self::Insert(value, InsertMode::Keep) => {
                if !Unpacked::EMPTY {
                    entity.insert_if_new(value).buffer_undo_redo(
                        now,
                        UndoRedoSwap(RevRemove::<Unpacked> {
                            state: None,
                            entity: id,
                        }),
                    );
                }
            }
        }
    }
}

#[doc(hidden)]
pub struct Empty;

impl PartialOp<Empty> for () {
    const EMPTY: bool = true;

    fn apply_op<
        Unpacked: PartialOp<UnpackedMarker>,
        Queue: PartialOp<QueueMarker>,
        Op: PartialOp<OpMarker>,
        UnpackedMarker,
        QueueMarker,
        OpMarker,
    >(
        entity: &mut EntityWorldMut,
        now: NonLogNow,
        op: RevOp<Op>,
    ) {
        if !Queue::EMPTY {
            Queue::apply_op::<Unpacked, (), Op, _, _, _>(entity, now, op);
        } else if !Unpacked::EMPTY {
            op.apply_op::<Unpacked, _, _>(entity, now);
        }
    }
}

#[doc(hidden)]
pub struct Single;

impl<T: Component> PartialOp<Single> for T {
    const EMPTY: bool = false;

    fn apply_op<
        Unpacked: PartialOp<UnpackedMarker>,
        Queue: PartialOp<QueueMarker>,
        Op: PartialOp<OpMarker>,
        UnpackedMarker,
        QueueMarker,
        OpMarker,
    >(
        entity: &mut EntityWorldMut,
        now: NonLogNow,
        op: RevOp<Op>,
    ) {
        let add_to_unpacked =
            matches!(op, RevOp::Insert(_, InsertMode::Keep)) != entity.contains::<T>();
        if Queue::EMPTY {
            if add_to_unpacked {
                op.apply_op::<(Unpacked, T), _, _>(entity, now);
            } else if !Unpacked::EMPTY {
                op.apply_op::<Unpacked, _, _>(entity, now);
            }
        } else if add_to_unpacked {
            if Unpacked::EMPTY {
                Queue::apply_op::<T, (), Op, _, _, _>(entity, now, op);
            } else {
                Queue::apply_op::<(Unpacked, T), (), Op, _, _, _>(entity, now, op);
            }
        } else {
            Queue::apply_op::<Unpacked, (), Op, _, _, _>(entity, now, op);
        }
    }
}

macro_rules! impl_remove {
    ($(($T: ident, $M: ident)),*) => {
        impl<T, Marker, $($T, $M),*> PartialOp<(Marker, $($M,)*)> for (T, $($T,)*)
        where
            T: PartialOp<Marker>,
            $($T: PartialOp<$M>,)*
        {
            const EMPTY: bool = T::EMPTY $(&& $T::EMPTY)*;

            fn apply_op<
                Unpacked: PartialOp<UnpackedMarker>,
                Queue: PartialOp<QueueMarker>,
                Op: PartialOp<OpMarker>,
                UnpackedMarker,
                QueueMarker,
                OpMarker
            >(
                entity: &mut EntityWorldMut,
                now: NonLogNow,
                op: RevOp<Op>
            ) {
                if Queue::EMPTY {
                    T::apply_op::<Unpacked, ($($T,)*), Op, _, _, _>(entity, now, op);
                } else {
                    T::apply_op::<Unpacked, (Queue, $($T,)*), Op, _, _, _>(entity, now, op);
                }
            }
        }
    };
}

all_tuples!(impl_remove, 0, 14, T, Marker);

#[cfg(test)]
mod test {
    use crate::{
        meta::{RevDirection, RevMeta},
        panic_on_error_events,
        prelude::UndoRedoBuffer,
    };

    use super::*;

    mod remove {
        use super::*;

        #[derive(Component, Default)]
        struct C1;

        #[derive(Component, Default)]
        struct C2;

        fn test<T: PartialOp<Marker> + Default, Marker>(remove_count: usize) {
            panic_on_error_events();

            let mut world = World::new();

            world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
            world.init_resource::<UndoRedoBuffer>();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();

            let mut entity_mut = world.spawn(T::default());
            let entity = entity_mut.id();

            rev_remove::<T, _>(&mut entity_mut, now);
            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

            if remove_count == 0 {
                assert!(buffer.is_empty());
                return;
            }

            assert_eq!(world.entity(entity).archetype().component_count(), 0);

            buffer.undo(&mut world);
            assert_eq!(
                world.entity(entity).archetype().component_count(),
                remove_count
            );

            buffer.redo(&mut world);
            assert_eq!(world.entity(entity).archetype().component_count(), 0);
        }

        #[test]
        fn remove() {
            test::<(), _>(0);
            test::<((),), _>(0);
            test::<((), ()), _>(0);
            test::<(((),), ((),)), _>(0);
            test::<(((), ()), ((), ())), _>(0);

            test::<C1, _>(1);

            test::<(C1,), _>(1);
            test::<(C1, ()), _>(1);
            test::<((), C1), _>(1);

            test::<((C1,),), _>(1);
            test::<((C1, ()),), _>(1);
            test::<(((), C1),), _>(1);

            test::<((C1,), ()), _>(1);
            test::<((C1, ()), ()), _>(1);
            test::<(((), C1), ()), _>(1);

            test::<(C1, C2), _>(2);

            test::<((C1,), C2), _>(2);
            test::<((C1, ()), C2), _>(2);
            test::<(((), C1), C2), _>(2);

            test::<(C1, (C2,)), _>(2);
            test::<(C1, (C2, ())), _>(2);
            test::<(C1, ((), C2)), _>(2);
        }
    }

    mod insert {
        use super::*;

        #[derive(Component, Default, PartialEq, Debug)]
        struct C1(bool);

        #[derive(Component, Default, PartialEq, Debug)]
        struct C2;

        /// Tests on entity with `C1` unequal to a potential insert and no C2.
        fn test<T: PartialOp<Marker> + Default, Marker>(
            insert_1: bool,
            insert_2: bool,
            insert_mode: InsertMode,
        ) {
            panic_on_error_events();

            let mut world = World::new();

            world.insert_resource(RevDirection::NOT_LOG.to_meta(0, 1, 1));
            world.init_resource::<UndoRedoBuffer>();
            let now = world.resource::<RevMeta>().non_log_now().unwrap();

            let mut entity_mut = world.spawn(C1(true));

            let entity = entity_mut.id();

            rev_insert::<_, _>(&mut entity_mut, now, T::default(), insert_mode);
            let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

            if !insert_1 && !insert_2 {
                assert!(buffer.is_empty());
                return;
            }

            let entity_ref = world.entity(entity);
            assert_eq!(entity_ref.get::<C1>(), Some(&C1(!insert_1)));
            assert_eq!(entity_ref.contains::<C2>(), insert_2);

            buffer.undo(&mut world);

            let entity_ref = world.entity(entity);
            assert_eq!(entity_ref.get::<C1>(), Some(&C1(true)));
            assert_eq!(entity_ref.contains::<C2>(), false);

            buffer.redo(&mut world);

            let entity_ref = world.entity(entity);
            assert_eq!(entity_ref.get::<C1>(), Some(&C1(!insert_1)));
            assert_eq!(entity_ref.contains::<C2>(), insert_2);
        }

        fn insert(insert_mode: InsertMode) {
            let insert_1 = insert_mode == InsertMode::Replace;

            test::<(), _>(false, false, insert_mode);
            test::<((),), _>(false, false, insert_mode);
            test::<((), ()), _>(false, false, insert_mode);
            test::<(((),), ((),)), _>(false, false, insert_mode);
            test::<(((), ()), ((), ())), _>(false, false, insert_mode);

            test::<C1, _>(insert_1, false, insert_mode);

            test::<(C1,), _>(insert_1, false, insert_mode);
            test::<(C1, ()), _>(insert_1, false, insert_mode);
            test::<((), C1), _>(insert_1, false, insert_mode);

            test::<((C1,),), _>(insert_1, false, insert_mode);
            test::<((C1, ()),), _>(insert_1, false, insert_mode);
            test::<(((), C1),), _>(insert_1, false, insert_mode);

            test::<((C1,), ()), _>(insert_1, false, insert_mode);
            test::<((C1, ()), ()), _>(insert_1, false, insert_mode);
            test::<(((), C1), ()), _>(insert_1, false, insert_mode);

            test::<(C1, C2), _>(insert_1, true, insert_mode);

            test::<((C1,), C2), _>(insert_1, true, insert_mode);
            test::<((C1, ()), C2), _>(insert_1, true, insert_mode);
            test::<(((), C1), C2), _>(insert_1, true, insert_mode);

            test::<(C1, (C2,)), _>(insert_1, true, insert_mode);
            test::<(C1, (C2, ())), _>(insert_1, true, insert_mode);
            test::<(C1, ((), C2)), _>(insert_1, true, insert_mode);
        }

        #[test]
        fn replace() {
            insert(InsertMode::Replace);
        }

        #[test]
        fn keep() {
            insert(InsertMode::Keep);
        }
    }
}
