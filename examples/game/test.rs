use bevy::{
    ecs::error::{FallbackErrorHandler, Severity},
    input::{InputPlugin, keyboard::Key},
    prelude::*,
    state::app::StatesPlugin,
};
use bevy_oozlum::meta::{RevDirection, RevMeta};
use core::panic::Location;

use super::*;

const MAX_PAST_LEN: u64 = 5;

pub fn main() {
    // setup should align with main.rs main()
    let mut app = App::new();
    app.add_plugins((
        RevPlugin
            .set_max_past_len(MAX_PAST_LEN)
            .set_runner_in_schedule(Update), // not FixedUpdae
        rows::plugin,
        control::plugin,
        MinimalPlugins,
        StatesPlugin,
        InputPlugin,
    ))
    .add_systems(RevUpdate, despawn_lost_waste.after(RevSystems))
    .init_state::<GameState>()
    .init_resource::<Stats>();

    test(app);
}

fn test(mut app: App) {
    app.world_mut()
        .insert_resource(FallbackErrorHandler(|err, _| {
            if err.severity() >= Severity::Error {
                panic!("{err}");
            }
        }));

    app.update();
    assert_ran_not_log(&app);

    press_key(&mut app, KeyCode::ArrowRight);
    app.update();
    assert_ran(&app, RevDirection::BackwardLog);
    app.update();
    assert_pause(&app);

    press_key(&mut app, KeyCode::ArrowLeft);
    app.update();
    assert_ran(&app, RevDirection::ForwardLog);
    app.update();
    assert_pause(&app);

    press_key(&mut app, KeyCode::ArrowUp);
    app.update();
    assert_ran_not_log(&app);

    press_key(&mut app, KeyCode::ArrowDown);
    app.update();
    assert_pause(&app);

    press_key(&mut app, KeyCode::ArrowUp);
    app.update();
    assert_past_len(&app, 3);

    press_key(&mut app, KeyCode::Backspace);
    app.update();
    assert_past_len(&app, 1);

    press_all_nums(&mut app);
    app.update();
    assert_wastes(&mut app, ROWS);

    press_key(&mut app, KeyCode::ArrowRight);
    app.update();
    assert_wastes(&mut app, 0);

    press_key(&mut app, KeyCode::ArrowLeft);
    app.update();
    assert_wastes(&mut app, ROWS);

    press_key(&mut app, KeyCode::ArrowRight);
    app.update();
    press_key(&mut app, KeyCode::ArrowUp);
    app.update();
    assert_wastes(&mut app, 0);

    press_key(&mut app, KeyCode::ArrowRight);
    app.update();
    press_key(&mut app, KeyCode::ArrowLeft);
    app.update();

    press_key(&mut app, KeyCode::ArrowUp);
    press_all_nums(&mut app);
    for _ in 0..=MAX_PAST_LEN {
        app.update();
    }
    assert_wastes(&mut app, ROWS);

    app.update();
    assert_wastes(&mut app, 0);
}

#[track_caller]
fn assert_ran_not_log(app: &App) {
    let meta = app.world().resource::<RevMeta>();
    assert!(
        meta.get_ran_direction()
            .is_some_and(RevDirection::is_not_log),
        "{:?}",
        Location::caller()
    );
}

#[track_caller]
fn assert_ran(app: &App, ran: RevDirection) {
    let meta = app.world().resource::<RevMeta>();
    assert_eq!(
        meta.get_ran_direction(),
        Some(ran),
        "{:?}",
        Location::caller()
    );
}

#[track_caller]
fn assert_pause(app: &App) {
    let meta = app.world().resource::<RevMeta>();
    assert!(meta.paused(), "{:?}", Location::caller());
}

#[track_caller]
fn assert_past_len(app: &App, past_len: u64) {
    let meta = app.world().resource::<RevMeta>();
    assert_eq!(meta.past_len(), past_len, "{:?}", Location::caller());
}

#[track_caller]
fn assert_wastes(app: &mut App, amount: usize) {
    let mut query_state = app.world_mut().query_filtered::<(), With<Waste>>();
    assert_eq!(
        query_state.iter(app.world()).len(),
        amount,
        "{:?}",
        Location::caller()
    );
}

fn press_key(app: &mut App, key: KeyCode) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(key);
    keys.release(key);
}

fn press_all_nums(app: &mut App) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<Key>>();
    for num in 1..=ROWS {
        let code = Key::Character(num.to_string().into());
        keys.press(code.clone());
        keys.release(code);
    }
}
