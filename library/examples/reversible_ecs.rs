use std::{io::stdout, num::NonZeroUsize, time::Duration};

use bevy::{
    app::App,
    ecs::{component::HookContext, world::DeferredWorld},
    prelude::*,
};

use crossterm::{cursor::*, terminal::*, ExecutableCommand};

use library::{
    log::{DenseTransitionLog, FrameTransitionLog, SparseTransitionLog},
    prelude::*,
};

const MAX_LOG_LEN: usize = 71;
const FIXED_TIMESTEP: Duration = Duration::from_millis(100);

// todo: mention how the last column cannot be undone

// todo: entirely work with entity disabling instead of removing/readding components

fn main() {
    let _crossterm = GlobalSettings::new();
    let meta = RevMeta::new(NonZeroUsize::new(MAX_LOG_LEN), 0, false);
    App::new()
        .add_plugins((
            MinimalPlugins,
            // todo: explain plugin
            RevSystemsPlugin::add_meta_and_runner(meta, FixedUpdate),
            // todo: explain general task of a row
            (row1, row2, row3, row4, row5, row6),
        ))
        .rev_configure_sets(
            RevUpdate,
            (
                Row(1).rev_ambiguous_with_all(),
                Row(2).rev_after(Row(1)),
                (
                    Row(3).rev_after_ignore_deferred(Row(2)),
                    Row(4),
                    Row(5).rev_before(Row(6)),
                )
                    .rev_chain(),
            ),
        )
        .add_systems(PreUpdate, map_input)
        .add_systems(
            FixedUpdate,
            (
                control_rev_meta.before(RevMeta::run_rev_update),
                render.after(RevMeta::run_rev_update),
            ),
        )
        .add_systems(RevUpdate, clear_input.after(RevSystemsSet))
        .init_resource::<KeysPressed>()
        .init_resource::<LostWaste>()
        .insert_resource(Time::<Fixed>::from_duration(FIXED_TIMESTEP))
        .run();
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct Row(u8);

#[derive(Component, Clone, Copy)]
#[component(on_remove = on_remove)]
struct Waste {
    tossed_at: u64,
    row: usize,
}

#[derive(Resource, Default)]
struct LostWaste(usize);

fn on_remove(mut world: DeferredWorld, _: HookContext) {
    if world.resource::<RevMeta>().get_direction() == Some(RevDirection::NOT_LOG) {
        world.resource_mut::<LostWaste>().0 += 1;
    }
}

fn row1(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(1)));

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if meta.direction() != RevDirection::NOT_LOG || !pressed.num1 {
            return;
        }
        commands./*rev_*/spawn(Waste {
            tossed_at: meta.now(),
            row: 1,
        });
    }
}

fn row2(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(2)));

    fn system(
        meta: Res<RevMeta>,
        pressed: Res<KeysPressed>,
        mut log: Local<SparseTransitionLog<Entity>>,
        mut commands: Commands,
    ) {
        let waste = Waste {
            tossed_at: meta.now(),
            row: 2,
        };
        match meta.direction() {
            RevDirection::NOT_LOG => {
                for entity in log.drain_future() {
                    commands.entity(entity).despawn();
                }
                // todo: explain why this is needed, point out how waste is visible but not undoable at the right edge
                let past_len = meta.past_len() + 1;
                let entity = pressed.num2.then(|| commands.spawn(waste).id());
                if let Some(entity) = log.push_and_pop_past(past_len as usize, entity) {
                    commands.entity(entity).despawn();
                }
            }
            RevDirection::FORWARD_LOG => {
                if let Some(entity) = log.forward_log().unwrap() {
                    commands.entity(*entity).insert(waste);
                }
            }
            RevDirection::BackwardLog => {
                if let Some(entity) = log.backward_log().unwrap() {
                    commands.entity(*entity).remove::<Waste>();
                }
            }
        }
    }
}

fn row3(app: &mut App) {
    app.rev_add_systems(
        RevUpdate,
        spawn_and_log_system
            .rev_run_if(spawn_condition)
            .rev_in_set(Row(3)),
    )
    .add_systems(
        RevUpdate,
        despawn_system.after(forward_set(spawn_and_log_system)),
    );

    fn spawn_and_log_system(
        meta: Res<RevMeta>,
        mut entity_log: Local<DenseTransitionLog<Entity>>,
        mut frame_log: Local<FrameTransitionLog>,
        mut commands: Commands,
    ) {
        let waste = Waste {
            tossed_at: meta.now(),
            row: 3,
        };
        match meta.direction() {
            RevDirection::NOT_LOG => {
                for entity in entity_log.drain_future() {
                    commands.entity(entity).despawn();
                }
                // todo: explain why this is needed, point out how waste is visible but not undoable at the right edge
                let past_len = frame_log.push_and_get_past_len(&meta);
                let entity = commands.spawn(waste).id();
                // do not despawn drained entities here, could be too late because this system does not run every frame
                entity_log.push_and_drain_past(past_len, entity);
            }
            RevDirection::FORWARD_LOG => {
                let expects_forward_log_run = frame_log.forward_log(&meta);
                assert!(expects_forward_log_run);
                let entity = *entity_log.forward_log().unwrap();
                commands.entity(entity).insert(waste);
            }
            RevDirection::BackwardLog => {
                let expects_backward_log_run = frame_log.backward_log(&meta);
                assert!(expects_backward_log_run);
                let entity = *entity_log.backward_log().unwrap();
                commands.entity(entity).remove::<Waste>();
            }
        }
    }

    fn spawn_condition(pressed: Res<KeysPressed>) -> bool {
        pressed.num3
    }

    fn despawn_system(meta: Res<RevMeta>, query: Query<(Entity, &Waste)>, mut commands: Commands) {
        query
            .into_iter()
            .filter(|(_, waste)| waste.row == 3 && waste.tossed_at < meta.past_end())
            .for_each(|(entity, _)| commands.entity(entity).despawn());
    }
}

fn row4(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(4)));

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if meta.direction().is_log() || !pressed.num4 {
            return;
        }
        let waste = Waste {
            tossed_at: meta.now(),
            row: 4,
        };
        let entity = commands.spawn(waste).id();
        commands.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Waste>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(waste);
                }
            };
        });
        commands.buffer_finalize(move |world: &mut World, _: FinalizeDirection| {
            world.entity_mut(entity).despawn();
        });
    }
}

fn row5(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(5)))
        .world_mut()
        .register_component_hooks::<Waste>()
        .on_add(on_add);

    // buffered UndoRedo from on_add ends up in this system's state and thus needs to be added as a reversible system
    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if meta.direction().is_log() || !pressed.num5 {
            return;
        }
        commands.spawn(Waste {
            tossed_at: meta.now(),
            row: 5,
        });
    }

    fn on_add(mut world: DeferredWorld, context: HookContext) {
        if world.resource::<RevMeta>().direction().is_log() {
            return;
        }
        let entity = context.entity;
        let waste = *world.entity(entity).get::<Waste>().unwrap();
        if waste.row != 5 {
            return;
        }
        world.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Waste>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(waste);
                }
            };
        });
        world.buffer_finalize(move |world: &mut World, _: FinalizeDirection| {
            world.entity_mut(entity).despawn();
        });
    }
}

fn row6(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(6)))
        .add_observer(observer);

    #[derive(Event)]
    struct WasteObserverEvent(Waste);

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if meta.direction().is_log() || !pressed.num6 {
            return;
        }
        let waste = Waste {
            tossed_at: meta.now(),
            row: 6,
        };
        let entity = commands.spawn(waste).id();
        commands.trigger_targets(WasteObserverEvent(waste), entity);
    }

    fn observer(trigger: Trigger<WasteObserverEvent>, mut world: DeferredWorld) {
        let waste = trigger.0;
        let entity = trigger.target();
        world.buffer_undo_redo(move |world: &mut World, variant: UndoRedoDirection| {
            let mut entity = world.entity_mut(entity);
            match variant {
                UndoRedoDirection::Undo => {
                    entity.remove::<Waste>();
                }
                UndoRedoDirection::Redo => {
                    entity.insert(waste);
                }
            }
        });
        world.buffer_finalize(move |world: &mut World, _: FinalizeDirection| {
            world.entity_mut(entity).despawn();
        });
    }
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

fn map_input(mut keys: ResMut<KeysPressed>, lost: Res<LostWaste>, mut exit: EventWriter<AppExit>) {
    use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyEventKind};

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
        Some(Direction::Forward) => meta.queue_not_log_forward(),
        Some(Direction::Pause) => meta.queue_pause(),
        Some(Direction::FutureEnd) => {
            let to = meta.future_end();
            let _ok = meta.queue_log(to);
        }
        Some(Direction::PastEnd) => {
            let to = meta.past_end();
            let _ok = meta.queue_log(to);
        }
        None => {}
    }
}

struct GlobalSettings;

impl GlobalSettings {
    fn new() -> Self {
        use bevy::log::{
            tracing::{dispatcher::get_default, Event, Subscriber},
            tracing_subscriber::{
                layer::{Context, SubscriberExt},
                registry,
                util::SubscriberInitExt,
                Layer,
            },
            Level,
        };

        struct PanicOnError;
        impl<S: Subscriber> Layer<S> for PanicOnError {
            fn on_event(&self, event: &Event, _ctx: Context<S>) {
                if *event.metadata().level() == Level::ERROR {
                    panic!("{event:#?}")
                }
            }
        }
        if registry().with(PanicOnError).try_init().is_err() {
            get_default(|subscriber| {
                assert!(subscriber.downcast_ref::<PanicOnError>().is_some());
            })
        }
        let _ = stdout().execute(SetSize(MAX_LOG_LEN as u16 + 2, 13));
        let _ = stdout().execute(Hide);
        Self
    }
}

impl Drop for GlobalSettings {
    fn drop(&mut self) {
        let _ = stdout().execute(MoveToRow(14));
        let _ = stdout().execute(Show);
    }
}

fn render(
    meta: Res<RevMeta>,
    waste: Query<&Waste>,
    lost: Res<LostWaste>,
    mut last_future_end: Local<Option<u64>>,
) {
    let _ = stdout().execute(BeginSynchronizedUpdate);
    let _ = stdout().execute(Clear(ClearType::All));

    println!();
    println!("Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!");
    println!("No worry, it's okay as long you undo it. Just don't wait for too long...");
    println!();

    let wave = |phase: u64| "`-._,~'".chars().cycle().skip(7 - phase as usize % 7);

    let row_future: String = wave(meta.future_end())
        .take(meta.future_len() as usize)
        .collect();

    let row_past: String = wave(meta.future_end())
        .skip(meta.future_len() as usize % 7)
        .take(meta.past_len() as usize + 1)
        .collect();
    let mut past_rows: [String; 6] = std::array::from_fn(|_| row_past.clone());

    let padding_cols = match *last_future_end {
        Some(frame) => {
            let mut padding = frame.wrapping_sub(meta.future_end());
            if padding > MAX_LOG_LEN as u64 {
                padding = 0;
                *last_future_end = Some(meta.future_end());
            }
            padding as usize
        }
        None => {
            *last_future_end = Some(meta.future_end());
            0
        }
    };
    let padding = " ".repeat(padding_cols);

    for Waste { row, tossed_at } in waste.iter().cloned() {
        let index = (meta.now() - tossed_at) as usize;
        // replace_range would panic if a waste is tossed into the water at a frame that is not the present or that is not within the past log
        // this is ensured by reversible logic and by wastes being despawned when they go out of log
        past_rows
            .get_mut(row - 1)
            .unwrap()
            .replace_range(index..=index, "#");
    }

    for (i, past_row) in past_rows.into_iter().enumerate() {
        println!("{padding}{row_future}{}{past_row}", i + 1);
    }

    let lost = lost.0.min(10);
    let lost_bar: String = "#"
        .repeat(lost)
        .chars()
        .chain(wave(meta.now()).take(10 - lost))
        .collect();

    println!(
        "{}^",
        " ".repeat(padding_cols + meta.future_len() as usize + 1)
    );
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
        println!("You left too much waste behind that you can no longer recover. GAME OVER");
    }
    let _ = stdout().execute(MoveTo(0, 0));
    let _ = stdout().execute(EndSynchronizedUpdate);
}
