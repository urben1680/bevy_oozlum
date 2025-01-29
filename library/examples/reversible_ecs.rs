use std::{num::NonZeroU64, time::Duration};

use bevy::{
    app::App,
    ecs::{component::ComponentId, world::DeferredWorld},
    prelude::*,
};

use library::{
    log::{DenseTransitionLog, FrameTransitionLog, SparseTransitionLog},
    prelude::*,
};

const MAX_LOG_LEN: u64 = 71;
const FIXED_DURATION_MS: u64 = 100;

// todo: modules per system (if more than the system)

fn main() {
    let meta = RevMeta::new(NonZeroU64::new(MAX_LOG_LEN), None, false);
    App::new()
        .add_plugins((
            RevSystemsPlugin::add_meta_and_runner(meta, FixedUpdate),
            MinimalPlugins,
        ))
        .add_systems(PreUpdate, map_input)
        .rev_add_systems(
            RevUpdate,
            (
                (system_1, system_2).rev_chain(),
                system_3.rev_run_if(pressed_3),
                system_4.rev_after(system_3),
                system_5, /*.backward_noop()*/
                system_6, /*.backward_noop()*/
            ),
        )
        .add_systems(RevUpdate, clear_input.after(RevSystemsSet))
        .add_systems(
            FixedUpdate,
            (
                control_rev_meta.before(RevMeta::run_rev_update),
                render_game.after(RevMeta::run_rev_update),
            ),
        )
        .init_resource::<KeysPressed>()
        .init_resource::<LostTrash>()
        .insert_resource(Time::<Fixed>::from_duration(Duration::from_millis(
            FIXED_DURATION_MS,
        )))
        .add_observer(observer_6)
        .run();
}

#[derive(Component, Clone, Copy)]
#[component(on_add = on_add_5)]
#[component(on_remove = on_remove)]
struct Trash {
    tossed_at: u64,
    row: usize,
}

#[derive(Resource, Default)]
struct LostTrash(usize);

fn on_remove(mut world: DeferredWorld, _: Entity, _: ComponentId) {
    if world.resource::<RevMeta>().get_direction() == Some(RevDirection::NOT_LOG) {
        world.resource_mut::<LostTrash>().0 += 1;
    }
}

fn system_1(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
    if meta.direction() != RevDirection::NOT_LOG || !pressed.num1 {
        return;
    }
    // blocked on https://github.com/bevyengine/bevy/pull/13120
    commands./*rev_*/spawn(Trash {
        tossed_at: meta.present_world_state(),
        row: 1,
    });
}

fn system_2(
    meta: Res<RevMeta>,
    pressed: Res<KeysPressed>,
    mut log: Local<SparseTransitionLog<Entity>>,
    mut commands: Commands,
) {
    let trash = Trash {
        tossed_at: meta.present_world_state(),
        row: 2,
    };
    match meta.direction() {
        RevDirection::NOT_LOG => {
            let entity = pressed.num2.then(|| commands.spawn(trash).id());
            for entity in log.drain_future() {
                commands.entity(entity).despawn();
            }
            if let Some(entity) = log.push_and_pop_past(meta.past_world_states() as usize, entity) {
                commands.entity(entity).despawn();
            }
        }
        RevDirection::FORWARD_LOG => {
            if let Some(entity) = log.forward_log().unwrap() {
                commands.entity(*entity).insert(trash);
            }
        }
        RevDirection::BackwardLog => {
            if let Some(entity) = log.backward_log().unwrap() {
                commands.entity(*entity).remove::<Trash>();
            }
        }
    }
}

// todo: system that despawns stones
fn system_3(
    meta: Res<RevMeta>,
    mut entity_log: Local<DenseTransitionLog<Entity>>,
    mut frame_log: Local<FrameTransitionLog>,
    mut commands: Commands,
) {
    let trash = Trash {
        tossed_at: meta.present_world_state(),
        row: 3,
    };
    match meta.direction() {
        RevDirection::NOT_LOG => {
            for entity in entity_log.drain_future() {
                commands.entity(entity).despawn();
            }
            let past_len = frame_log.push_and_get_past_len(&meta);
            let entity = commands.spawn(trash).id();
            for entity in entity_log.push_and_drain_past(past_len, entity) {
                commands.entity(entity).despawn();
            }
        }
        RevDirection::FORWARD_LOG => {
            let entity = *entity_log.forward_log().unwrap();
            commands.entity(entity).insert(trash);
        }
        RevDirection::BackwardLog => {
            let entity = *entity_log.backward_log().unwrap();
            commands.entity(entity).remove::<Trash>();
        }
    }
}

fn pressed_3(pressed: Res<KeysPressed>) -> bool {
    pressed.num3
}

// is reversible because uses commands.buffer_undo_redo
fn system_4(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
    if meta.direction().is_log() || !pressed.num4 {
        return;
    }
    let trash = Trash {
        tossed_at: meta.present_world_state(),
        row: 4,
    };
    let entity = commands.spawn(trash).id();
    commands.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
        let mut entity = world.entity_mut(entity);
        match variant {
            UndoRedoDirection::Undo => {
                entity.remove::<Trash>();
            }
            UndoRedoDirection::Redo => {
                entity.insert(trash);
            }
            UndoRedoDirection::FinalizeUndone | UndoRedoDirection::FinalizeRedone => {
                entity.despawn();
            }
        };
    });
}

// is not reversible
fn system_5(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
    if meta.direction().is_log() || !pressed.num5 {
        return;
    }
    commands.spawn(Trash {
        tossed_at: meta.present_world_state(),
        row: 5,
    });
}

fn on_add_5(mut world: DeferredWorld, entity: Entity, _: ComponentId) {
    if world.resource::<RevMeta>().direction().is_log() {
        return;
    }
    let trash = *world.entity(entity).get::<Trash>().unwrap();
    if trash.row != 5 {
        return;
    }
    world.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
        let mut entity = world.entity_mut(entity);
        match variant {
            UndoRedoDirection::Undo => {
                entity.remove::<Trash>();
            }
            UndoRedoDirection::Redo => {
                entity.insert(trash);
            }
            UndoRedoDirection::FinalizeUndone | UndoRedoDirection::FinalizeRedone => {
                entity.despawn();
            }
        };
    });
}

// is not reversible
fn system_6(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
    if meta.direction().is_log() || !pressed.num6 {
        return;
    }
    let trash = Trash {
        tossed_at: meta.present_world_state(),
        row: 6,
    };
    let entity = commands.spawn(trash).id();
    commands.trigger_targets(Trash6Event(trash), entity);
}

#[derive(Event)]
struct Trash6Event(Trash);

fn observer_6(trigger: Trigger<Trash6Event>, mut world: DeferredWorld) {
    let trash = trigger.0;
    let entity = trigger.entity();
    world.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
        let mut entity = world.entity_mut(entity);
        match variant {
            UndoRedoDirection::Undo => {
                entity.remove::<Trash>();
            }
            UndoRedoDirection::Redo => {
                entity.insert(trash);
            }
            UndoRedoDirection::FinalizeUndone | UndoRedoDirection::FinalizeRedone => {
                entity.despawn();
            }
        };
    });
}

#[derive(Resource, Default)]
struct KeysPressed {
    direction: Option<Direction>,
    num1: bool,
    num2: bool,
    num3: bool,
    num4: bool,
    num5: bool,
    num6: bool,
}

enum Direction {
    Forward,
    Pause,
    FutureEnd,
    PastEnd,
}

fn map_input(mut keys: ResMut<KeysPressed>, lost: Res<LostTrash>, mut exit: EventWriter<AppExit>) {
    use crossterm::{
        event::{poll, read, Event, KeyCode, KeyEvent, KeyEventKind},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    let mut f = || -> std::io::Result<()> {
        // check if event exists to read, do not block thread to wait for one if not
        if !poll(Duration::from_secs(0))? {
            return Ok(());
        }
        // check if the event is a pressed key
        let Event::Key(KeyEvent {
            kind: KeyEventKind::Press,
            code,
            ..
        }) = read()?
        else {
            return Ok(());
        };
        match code {
            KeyCode::Esc => {
                exit.send(AppExit::Success);
            }
            // ignore any other inputs at game over
            _ if lost.0 >= 10 => {}
            KeyCode::Left => keys.direction = Some(Direction::FutureEnd),
            KeyCode::Right => keys.direction = Some(Direction::PastEnd),
            KeyCode::Up => keys.direction = Some(Direction::Forward),
            KeyCode::Down => keys.direction = Some(Direction::Pause),
            KeyCode::Char('1') => keys.num1 = true,
            KeyCode::Char('2') => keys.num2 = true,
            KeyCode::Char('3') => keys.num3 = true,
            KeyCode::Char('4') => keys.num4 = true,
            KeyCode::Char('5') => keys.num5 = true,
            KeyCode::Char('6') => keys.num6 = true,
            _ => {}
        }
        Ok(())
    };

    if enable_raw_mode().is_ok() {
        let _ = f();
        let _ = disable_raw_mode();
    }
}

fn clear_input(mut keys: ResMut<KeysPressed>) {
    *keys = default();
}

fn control_rev_meta(mut meta: ResMut<RevMeta>, keys: Res<KeysPressed>) {
    match keys.direction {
        Some(Direction::Forward) => meta.queue_forward(),
        Some(Direction::Pause) => meta.queue_pause(),
        Some(Direction::FutureEnd) => {
            let to = meta.future_end_world_state();
            let _ok = meta.queue_log(to);
        }
        Some(Direction::PastEnd) => {
            let to = meta.past_end_world_state();
            let _ok = meta.queue_log(to);
        }
        None => {}
    }
}

fn render_game(
    meta: Res<RevMeta>,
    trash: Query<&Trash>,
    lost: Res<LostTrash>,
    mut last_future_end: Local<Option<u64>>,
) {
    println!("\x1B[2J"); // this clears the last frame
    println!();
    println!("Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!");
    println!("No worry, it's okay as long you undo it. Just don't wait for too long...");
    println!();

    let wave = |phase: u64| "`-._,~'".chars().cycle().skip(7 - phase as usize % 7);

    let row_future: String = wave(meta.future_end_world_state())
        .take(meta.future_world_states() as usize)
        .collect();

    let row_past: String = wave(meta.future_end_world_state())
        .skip(meta.future_world_states() as usize % 7)
        .take(meta.past_world_states() as usize + 1)
        .collect();
    let mut past_rows: [String; 6] = std::array::from_fn(|_| row_past.clone());

    let padding = match *last_future_end {
        Some(frame) => {
            let mut padding = frame.wrapping_sub(meta.future_end_world_state());
            if padding > MAX_LOG_LEN {
                padding = 0;
                *last_future_end = Some(meta.future_end_world_state());
            }
            padding as usize
        }
        None => {
            *last_future_end = Some(meta.future_end_world_state());
            0
        }
    };
    let padding = " ".repeat(padding);

    for Trash { row, tossed_at } in trash.iter().cloned() {
        if row == 3 && !meta.past_contains(tossed_at) {
            // todo: no longer includes present
            // The log in system_3 is only cleaned up when the system runs.
            // As the system does not run every frame, it might not despawn as early as possible but remains in the world.
            // These trashs would cause a panic further down, so we skip them.
            continue;
        }
        let index = (meta.present_world_state() - tossed_at) as usize;
        // replace_range would panic if a trash is tossed into the water at a frame that is not the present or that is not within the past log
        // this is ensured by reversible logic and by trashs being despawned when they go out of log
        past_rows
            .get_mut(row - 1)
            .unwrap()
            .replace_range(index..(index + 1), "#");
    }

    for (i, past_row) in past_rows.into_iter().enumerate() {
        println!("{padding}{row_future}{}{past_row}", i + 1);
    }

    let lost = lost.0.min(10);
    let lost_bar: String = "#"
        .repeat(lost)
        .chars()
        .chain(wave(meta.present_world_state()).take(10 - lost))
        .collect();

    println!();
    if meta.ran_direction() != Some(RevDirection::NOT_LOG) || lost == 10 {
        println!("           (waste lost: {lost_bar})    ESC: close");
    } else {
        println!(" 1-6: toss waste (lost: {lost_bar})    ESC: close");
    }
    if lost < 10 {
        println!("LEFT: forward log, pause at end        RIGHT: backward log, pause at end");
        println!("  UP: exit log and resume              DOWN: pause");
    } else {
        println!();
        println!("You left too much trash behind that you can no longer recover. GAME OVER");
    }
    println!();
}
