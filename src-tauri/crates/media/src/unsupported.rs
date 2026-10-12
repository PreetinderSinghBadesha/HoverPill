use std::collections::HashSet;

use crate::{Action, NowPlaying, Notify, Source};

pub struct Backend;

impl Backend {
    pub fn new(_notify: Notify) -> Result<Self, String> {
        Err("no media backend for this OS".into())
    }

    pub fn read(&mut self, _ignored: &HashSet<String>) -> Result<(Option<NowPlaying>, Vec<Source>), String> {
        Ok((None, Vec::new()))
    }

    pub fn command(&mut self, _action: Action) -> Result<(), String> {
        Ok(())
    }
}
