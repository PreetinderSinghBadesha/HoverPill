use crate::Event;

pub fn run(_on_event: &dyn Fn(Event)) -> Result<(), String> {
    Err("notification mirroring isn't supported on this OS".into())
}
