use std::{collections::VecDeque, sync::mpsc::Sender};

use bevy::{ecs::event::Event, prelude::EventWriter};

struct ReversibleEvents<T: Event>{
    log: VecDeque<Vec<T>>,
    log_index: usize
}

/*
Todo: The event container is a resource and the event writing is done using commands
Finish this file when reversible systems are finished
*/