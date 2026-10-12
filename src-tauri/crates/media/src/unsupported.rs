use crate::{Action, NowPlaying, Notify};

pub struct Backend;

impl Backend {
    pub fn new(_notify: Notify) -> Result<Self, String> {
        Err("no media backend for this OS".into())
    }

    pub fn read(&mut self) -> Result<Option<NowPlaying>, String> {
        Ok(None)
    }

    pub fn command(&mut self, _action: Action) -> Result<(), String> {
        Ok(())
    }
}
