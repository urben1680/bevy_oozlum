/*
todo: nicht in `library` setzen, neuen ordner im projekt anlegen!!
*/

struct BitVec{
    receiver: Vec<BitCommand>, //wird von commands befüllt und für transitions gelesen
    data: Vec<u8>
    //buffer
}

struct BitCommand{
    index: u32, //save space for states that store bit commands
    set_0: u8,
    set_1: u8,
}

struct TransitionElement{
    index: u32, //save space for log?
    xor: u8
}

//also impl for arrays/vec/iter of BitCommand
impl ReversibleCommand for BitCommand{ //functionality for delayed bit commands
    fn init(self: Box<Self>, world: &mut World) -> Option<Box<dyn ReversibleCommandInitialized>>{
        world.resource_mut::<BitVec>().receiver.push(*self);
        None //no further need of commands
    }
}

impl PerSystem for BitVec{
    type Params<'w, 's> = ResMut<'w, BitVec>;
    type Transition = Vec<TransitionElement>;
    //todo
}