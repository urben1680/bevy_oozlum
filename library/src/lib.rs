/*
TODO:

- general todo!() und //todo
- value logs
- log tests
- reversible hooks ?
- reversible observer ?
- reversible Event reader/writer
- standard rev commands
- App Methoden auch für RevSchedule struct
- tuple impl configs
- impl Reflection für logs
- cfg für ecs only (keine App), logging
- examples
- documentations, besonders mit informationen welche Methoden für deterministische Logik geeignet ist
*/

use std::hash::Hash;

use bevy::ecs::{intern::Interned, schedule::ScheduleLabel};

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;
