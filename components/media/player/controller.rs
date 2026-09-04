/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A deterministic, product-neutral playback state machine.
//!
//! Source I/O and audio-device ownership stay behind caller-supplied traits.
//! The controller only orders commands, backend signals, and authoritative
//! sink snapshots.

use std::time::Duration;

use crate::{PlaybackSnapshot, PlaybackState, PlayerError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediaSource {
    Local { path: String },
    Http { url: String },
    HostBlob { id: String },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackRateRange {
    pub minimum: f64,
    pub maximum: f64,
}

impl PlaybackRateRange {
    fn contains(self, rate: f64) -> bool {
        rate.is_finite() && rate > 0.0 && rate >= self.minimum && rate <= self.maximum
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackCapabilities {
    pub seekable: bool,
    pub playback_rates: Option<PlaybackRateRange>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaInfo {
    pub duration: Option<Duration>,
    pub capabilities: PlaybackCapabilities,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackCommand {
    Load(MediaSource),
    Play,
    Pause,
    Stop,
    Seek(Duration),
    SetRate(f64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackSignal {
    Ready,
    Buffering,
    EndOfStream,
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackTerminal {
    EndOfStream,
    Error(String),
}

pub trait PlaybackSource {
    fn load(&mut self, source: &MediaSource) -> Result<MediaInfo, String>;
    fn seek(&mut self, position: Duration) -> Result<(), String>;
    fn set_rate(&mut self, rate: f64) -> Result<(), String>;
}

/// The host-owned sink supplies the presentation clock used for persisted
/// timed targets. Implementations should subtract queued output from accepted
/// frames before returning `position`.
pub trait PlaybackSink {
    fn set_playing(&mut self, playing: bool) -> Result<(), String>;
    fn reset(&mut self, position: Duration) -> Result<(), String>;
    fn position(&self) -> Result<Duration, String>;
}

pub struct PlaybackController<S, K> {
    source: S,
    sink: K,
    state: PlaybackState,
    info: Option<MediaInfo>,
    rate: f64,
    play_requested: bool,
    sequence: u64,
    terminal: Option<PlaybackTerminal>,
}

impl<S: PlaybackSource, K: PlaybackSink> PlaybackController<S, K> {
    pub fn new(source: S, sink: K) -> Self {
        Self {
            source,
            sink,
            state: PlaybackState::Stopped,
            info: None,
            rate: 1.0,
            play_requested: false,
            sequence: 0,
            terminal: None,
        }
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn terminal(&self) -> Option<&PlaybackTerminal> {
        self.terminal.as_ref()
    }

    pub fn source(&self) -> &S {
        &self.source
    }

    pub fn sink(&self) -> &K {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut K {
        &mut self.sink
    }

    pub fn command(&mut self, command: PlaybackCommand) -> Result<(), PlayerError> {
        match command {
            PlaybackCommand::Load(source) => self.load(source),
            PlaybackCommand::Play => self.play(),
            PlaybackCommand::Pause => self.pause(),
            PlaybackCommand::Stop => self.stop(),
            PlaybackCommand::Seek(position) => self.seek(position),
            PlaybackCommand::SetRate(rate) => self.set_rate(rate),
        }
    }

    pub fn signal(&mut self, signal: PlaybackSignal) -> Result<(), PlayerError> {
        self.require_source()?;
        match signal {
            PlaybackSignal::Ready => {
                self.sink
                    .set_playing(self.play_requested)
                    .map_err(PlayerError::Backend)?;
                self.state = if self.play_requested {
                    PlaybackState::Playing
                } else {
                    PlaybackState::Paused
                };
            },
            PlaybackSignal::Buffering => {
                self.sink.set_playing(false).map_err(PlayerError::Backend)?;
                self.state = PlaybackState::Buffering;
            },
            PlaybackSignal::EndOfStream => {
                self.sink.set_playing(false).map_err(PlayerError::Backend)?;
                self.play_requested = false;
                self.state = PlaybackState::Stopped;
                self.terminal = Some(PlaybackTerminal::EndOfStream);
            },
            PlaybackSignal::Error(error) => {
                self.sink.set_playing(false).map_err(PlayerError::Backend)?;
                self.play_requested = false;
                self.state = PlaybackState::Stopped;
                self.terminal = Some(PlaybackTerminal::Error(error));
            },
        }
        Ok(())
    }

    pub fn snapshot(&mut self) -> Result<PlaybackSnapshot, PlayerError> {
        let info = self.require_source()?;
        let position = self.sink.position().map_err(PlayerError::Backend)?;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(PlaybackSnapshot {
            state: self.state,
            position,
            duration: info.duration,
            rate: self.rate,
            sequence: self.sequence,
        })
    }

    fn load(&mut self, source: MediaSource) -> Result<(), PlayerError> {
        let info = self.source.load(&source).map_err(PlayerError::Backend)?;
        self.sink
            .reset(Duration::ZERO)
            .map_err(PlayerError::Backend)?;
        self.sink.set_playing(false).map_err(PlayerError::Backend)?;
        self.info = Some(info);
        self.rate = 1.0;
        self.play_requested = false;
        self.state = PlaybackState::Buffering;
        self.terminal = None;
        Ok(())
    }

    fn play(&mut self) -> Result<(), PlayerError> {
        self.require_source()?;
        self.play_requested = true;
        if self.state != PlaybackState::Buffering {
            self.sink.set_playing(true).map_err(PlayerError::Backend)?;
            self.state = PlaybackState::Playing;
        }
        self.terminal = None;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PlayerError> {
        self.require_source()?;
        self.play_requested = false;
        self.sink.set_playing(false).map_err(PlayerError::Backend)?;
        self.state = PlaybackState::Paused;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PlayerError> {
        self.require_source()?;
        self.sink.set_playing(false).map_err(PlayerError::Backend)?;
        self.sink
            .reset(Duration::ZERO)
            .map_err(PlayerError::Backend)?;
        self.play_requested = false;
        self.state = PlaybackState::Stopped;
        self.terminal = None;
        Ok(())
    }

    fn seek(&mut self, position: Duration) -> Result<(), PlayerError> {
        let info = self.require_source()?;
        if !info.capabilities.seekable {
            return Err(PlayerError::NonSeekableStream);
        }
        if info.duration.is_some_and(|duration| position > duration) {
            return Err(PlayerError::SeekOutOfRange);
        }
        self.source.seek(position).map_err(PlayerError::Backend)?;
        self.sink.reset(position).map_err(PlayerError::Backend)?;
        self.sink.set_playing(false).map_err(PlayerError::Backend)?;
        self.state = PlaybackState::Buffering;
        self.terminal = None;
        Ok(())
    }

    fn set_rate(&mut self, rate: f64) -> Result<(), PlayerError> {
        let info = self.require_source()?;
        let Some(range) = info.capabilities.playback_rates else {
            return Err(PlayerError::UnsupportedPlaybackRate);
        };
        if !range.contains(rate) {
            return Err(PlayerError::InvalidPlaybackRate);
        }
        self.source.set_rate(rate).map_err(PlayerError::Backend)?;
        self.rate = rate;
        Ok(())
    }

    fn require_source(&self) -> Result<MediaInfo, PlayerError> {
        self.info
            .ok_or_else(|| PlayerError::Backend("no media source is loaded".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeSource {
        loaded: Vec<MediaSource>,
        seeks: Vec<Duration>,
        rates: Vec<f64>,
        info: Option<MediaInfo>,
    }

    impl PlaybackSource for FakeSource {
        fn load(&mut self, source: &MediaSource) -> Result<MediaInfo, String> {
            self.loaded.push(source.clone());
            self.info.ok_or_else(|| "missing fake media info".into())
        }

        fn seek(&mut self, position: Duration) -> Result<(), String> {
            self.seeks.push(position);
            Ok(())
        }

        fn set_rate(&mut self, rate: f64) -> Result<(), String> {
            self.rates.push(rate);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSink {
        playing: bool,
        position: Duration,
        resets: Vec<Duration>,
    }

    impl PlaybackSink for FakeSink {
        fn set_playing(&mut self, playing: bool) -> Result<(), String> {
            self.playing = playing;
            Ok(())
        }

        fn reset(&mut self, position: Duration) -> Result<(), String> {
            self.position = position;
            self.resets.push(position);
            Ok(())
        }

        fn position(&self) -> Result<Duration, String> {
            Ok(self.position)
        }
    }

    fn media_info(seekable: bool, playback_rates: Option<PlaybackRateRange>) -> MediaInfo {
        MediaInfo {
            duration: Some(Duration::from_secs(60)),
            capabilities: PlaybackCapabilities {
                seekable,
                playback_rates,
            },
        }
    }

    fn controller(
        seekable: bool,
        playback_rates: Option<PlaybackRateRange>,
    ) -> PlaybackController<FakeSource, FakeSink> {
        PlaybackController::new(
            FakeSource {
                info: Some(media_info(seekable, playback_rates)),
                ..FakeSource::default()
            },
            FakeSink::default(),
        )
    }

    fn local_source() -> MediaSource {
        MediaSource::Local {
            path: "episode.mp3".into(),
        }
    }

    #[test]
    fn ready_play_pause_and_snapshot_use_the_sink_clock() {
        let mut player = controller(true, None);
        player
            .command(PlaybackCommand::Load(local_source()))
            .unwrap();
        player.command(PlaybackCommand::Play).unwrap();
        assert_eq!(player.state(), PlaybackState::Buffering);

        player.signal(PlaybackSignal::Ready).unwrap();
        assert_eq!(player.state(), PlaybackState::Playing);
        assert!(player.sink().playing);

        player.sink_mut().position = Duration::from_millis(12_345);
        let snapshot = player.snapshot().unwrap();
        assert_eq!(snapshot.position, Duration::from_millis(12_345));
        assert_eq!(snapshot.sequence, 1);

        player.command(PlaybackCommand::Pause).unwrap();
        assert_eq!(player.state(), PlaybackState::Paused);
        assert!(!player.sink().playing);
    }

    #[test]
    fn buffering_preserves_play_intent_across_seek() {
        let mut player = controller(true, None);
        player
            .command(PlaybackCommand::Load(local_source()))
            .unwrap();
        player.command(PlaybackCommand::Play).unwrap();
        player.signal(PlaybackSignal::Ready).unwrap();
        player
            .command(PlaybackCommand::Seek(Duration::from_secs(20)))
            .unwrap();

        assert_eq!(player.state(), PlaybackState::Buffering);
        assert_eq!(player.source().seeks, [Duration::from_secs(20)]);
        assert_eq!(player.snapshot().unwrap().position, Duration::from_secs(20));

        player.signal(PlaybackSignal::Ready).unwrap();
        assert_eq!(player.state(), PlaybackState::Playing);
        assert!(player.sink().playing);
    }

    #[test]
    fn end_of_stream_and_error_are_explicit_terminal_results() {
        let mut player = controller(true, None);
        player
            .command(PlaybackCommand::Load(local_source()))
            .unwrap();
        player.signal(PlaybackSignal::EndOfStream).unwrap();
        assert_eq!(player.terminal(), Some(&PlaybackTerminal::EndOfStream));
        assert_eq!(player.state(), PlaybackState::Stopped);

        player
            .command(PlaybackCommand::Load(local_source()))
            .unwrap();
        player
            .signal(PlaybackSignal::Error("decode failed".into()))
            .unwrap();
        assert_eq!(
            player.terminal(),
            Some(&PlaybackTerminal::Error("decode failed".into()))
        );
        assert_eq!(player.state(), PlaybackState::Stopped);
    }

    #[test]
    fn every_source_kind_uses_the_same_load_command() {
        let sources = [
            local_source(),
            MediaSource::Http {
                url: "https://example.test/episode.mp3".into(),
            },
            MediaSource::HostBlob {
                id: "blob-42".into(),
            },
        ];
        let mut player = controller(true, None);
        for source in sources.clone() {
            player.command(PlaybackCommand::Load(source)).unwrap();
        }
        assert_eq!(player.source().loaded, sources);
    }

    #[test]
    fn unsupported_seek_and_rate_are_typed() {
        let mut player = controller(false, None);
        player
            .command(PlaybackCommand::Load(local_source()))
            .unwrap();
        assert_eq!(
            player.command(PlaybackCommand::Seek(Duration::from_secs(1))),
            Err(PlayerError::NonSeekableStream)
        );
        assert_eq!(
            player.command(PlaybackCommand::SetRate(1.25)),
            Err(PlayerError::UnsupportedPlaybackRate)
        );

        let mut player = controller(
            true,
            Some(PlaybackRateRange {
                minimum: 0.5,
                maximum: 2.0,
            }),
        );
        player
            .command(PlaybackCommand::Load(local_source()))
            .unwrap();
        assert_eq!(
            player.command(PlaybackCommand::SetRate(2.5)),
            Err(PlayerError::InvalidPlaybackRate)
        );
        player.command(PlaybackCommand::SetRate(1.5)).unwrap();
        assert_eq!(player.source().rates, [1.5]);
        assert_eq!(player.snapshot().unwrap().rate, 1.5);
    }
}
