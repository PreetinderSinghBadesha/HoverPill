//! GSMTC: `Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager`.
//! Every app that integrates with the Windows media flyout (Spotify, browsers, VLC, Groove, ...)
//! shows up here as a session.

use windows::core::Interface;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows_future::IAsyncOperation;

use crate::{data_url, Action, ArtCache, NowPlaying, Notify, MAX_ARTWORK_BYTES};

/// The session we're showing, with the tokens needed to unsubscribe from it.
struct Watched {
    session: Session,
    props_token: i64,
    playback_token: i64,
}

pub struct Backend {
    manager: Manager,
    notify: Notify,
    watched: Option<Watched>,
    art: ArtCache,
}

fn err(e: windows::core::Error) -> String {
    format!("{} ({:#x})", e.message(), e.code().0)
}

impl Backend {
    pub fn new(notify: Notify) -> Result<Self, String> {
        // Already-initialised (S_FALSE) or a different apartment are both fine for these calls.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let manager = Manager::RequestAsync().and_then(|op| op.join()).map_err(err)?;
        let (n1, n2) = (notify.clone(), notify.clone());
        manager.SessionsChanged(&TypedEventHandler::new(move |_, _| Ok(n1()))).map_err(err)?;
        manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| Ok(n2()))).map_err(err)?;
        Ok(Self { manager, notify, watched: None, art: ArtCache::default() })
    }

    /// A session that is actually playing wins; otherwise whatever Windows considers current.
    fn pick(&self) -> Option<Session> {
        let sessions = self.manager.GetSessions().ok()?;
        let playing = (0..sessions.Size().ok()?)
            .filter_map(|i| sessions.GetAt(i).ok())
            .find(|s| status(s) == Some(Status::Playing));
        playing.or_else(|| self.manager.GetCurrentSession().ok())
    }

    /// Subscribes to change events on `session`, dropping the previous subscription.
    fn watch(&mut self, session: Option<&Session>) {
        if self.watched.as_ref().map(|w| &w.session) == session {
            return;
        }
        if let Some(old) = self.watched.take() {
            let _ = old.session.RemoveMediaPropertiesChanged(old.props_token);
            let _ = old.session.RemovePlaybackInfoChanged(old.playback_token);
        }
        let Some(session) = session else { return };
        let (n1, n2) = (self.notify.clone(), self.notify.clone());
        let tokens = session
            .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| Ok(n1())))
            .and_then(|p| Ok((p, session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| Ok(n2())))?)));
        if let Ok((props_token, playback_token)) = tokens {
            self.watched = Some(Watched { session: session.clone(), props_token, playback_token });
        }
    }

    pub fn read(&mut self) -> Result<Option<NowPlaying>, String> {
        let session = self.pick();
        self.watch(session.as_ref());
        let Some(session) = session else { return Ok(None) };

        let playing = match status(&session) {
            Some(Status::Playing) => true,
            Some(Status::Paused | Status::Changing) => false,
            _ => return Ok(None), // stopped / closed / opened-but-idle
        };
        let props = session.TryGetMediaPropertiesAsync().and_then(|op| op.join()).map_err(err)?;
        let title = props.Title().map_err(err)?.to_string_lossy();
        if title.is_empty() {
            return Ok(None); // e.g. a browser tab that registered a session without metadata
        }
        let artist = props.Artist().map_err(err)?.to_string_lossy();
        let artwork = self.art.get_or(&title, &artist, || thumbnail(&props));
        Ok(Some(NowPlaying { title, artist, playing, artwork }))
    }

    pub fn command(&mut self, action: Action) -> Result<(), String> {
        // Act on the session the pill is showing, not whatever `pick` would choose right now.
        let Some(session) = self.watched.as_ref().map(|w| w.session.clone()).or_else(|| self.pick()) else {
            return Ok(());
        };
        let op = match action {
            Action::PlayPause => session.TryTogglePlayPauseAsync(),
            Action::Next => session.TrySkipNextAsync(),
            Action::Previous => session.TrySkipPreviousAsync(),
        };
        op.and_then(|op| op.join()).map(drop).map_err(err)
    }
}

fn status(session: &Session) -> Option<Status> {
    session.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).ok()
}

fn thumbnail(props: &Properties) -> Option<String> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = usize::try_from(stream.Size().ok()?).ok().filter(|&s| s > 0 && s <= MAX_ARTWORK_BYTES)?;
    let mime = stream
        .ContentType()
        .map(|m| m.to_string_lossy())
        .ok()
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| "image/png".into());
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let load: IAsyncOperation<u32> = reader.LoadAsync(size as u32).ok()?.cast().ok()?;
    let loaded = load.join().ok()? as usize;
    let mut bytes = vec![0u8; loaded];
    reader.ReadBytes(&mut bytes).ok()?;
    data_url(&mime, &bytes)
}
