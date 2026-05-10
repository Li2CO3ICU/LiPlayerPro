use std::sync::{Arc, mpsc};

use crate::{
    audio_engine::AudioEngine,
    config::LibraryConfig,
    library::MusicLibrary,
    types::{AppEvent, TrackInfo},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    StateChanged(PlayerState),
    ScanProgress { current: usize, total: usize },
    ScanFinished,
}

pub struct PlayerCore {
    engine: Arc<AudioEngine>,
    library: Arc<MusicLibrary>,
    state: PlayerState,
    current_track_idx: Option<usize>,
    tx: mpsc::Sender<PlayerEvent>,
    rx: mpsc::Receiver<PlayerEvent>,
}

impl PlayerCore {
    pub fn new(device_name: &str, music_dir: &str, index_path: &str) -> Self {
        let engine = Arc::new(AudioEngine::new(device_name));
        let library = Arc::new(MusicLibrary::new(&LibraryConfig {
            music_dir: music_dir.to_string(),
            index_path: index_path.to_string(),
        }));
        let (tx, rx) = mpsc::channel();
        Self {
            engine,
            library,
            state: PlayerState::Idle,
            current_track_idx: None,
            tx,
            rx,
        }
    }

    pub fn scan_local_library(&self, music_dir: &str) {
        let (scan_tx, scan_rx) = mpsc::channel::<AppEvent>();
        let lib = Arc::clone(&self.library);
        let event_tx = self.tx.clone();
        let music_dir = music_dir.to_string();

        std::thread::spawn(move || {
            lib.build_index(&music_dir, scan_tx);
        });

        std::thread::spawn(move || {
            while let Ok(evt) = scan_rx.recv() {
                match evt {
                    AppEvent::ScanProgress { current, total } => {
                        let _ = event_tx.send(PlayerEvent::ScanProgress { current, total });
                    }
                    AppEvent::ScanFinished => {
                        let _ = event_tx.send(PlayerEvent::ScanFinished);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }

    pub fn list_tracks(&self) -> Vec<TrackInfo> {
        self.library.get_all_tracks()
    }

    pub fn play_track_at(&mut self, idx: usize) -> Result<(), &'static str> {
        let tracks = self.list_tracks();
        if tracks.is_empty() {
            return Err("local music library is empty");
        }
        let safe_idx = idx % tracks.len();
        let track = &tracks[safe_idx];
        self.engine.play(&track.path);
        self.current_track_idx = Some(safe_idx);
        self.state = PlayerState::Playing;
        let _ = self.tx.send(PlayerEvent::StateChanged(self.state));
        Ok(())
    }

    pub fn stop(&mut self) {
        self.engine.stop();
        self.state = PlayerState::Idle;
        self.current_track_idx = None;
        let _ = self.tx.send(PlayerEvent::StateChanged(self.state));
    }

    pub fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            self.engine.pause();
            self.state = PlayerState::Paused;
            let _ = self.tx.send(PlayerEvent::StateChanged(self.state));
        }
    }

    pub fn resume(&mut self) {
        if self.state == PlayerState::Paused {
            self.engine.resume();
            self.state = PlayerState::Playing;
            let _ = self.tx.send(PlayerEvent::StateChanged(self.state));
        }
    }

    pub fn state(&self) -> PlayerState {
        self.state
    }

    pub fn elapsed_millis(&self) -> u64 {
        self.engine.get_elapsed_millis()
    }

    pub fn current_track_index(&self) -> Option<usize> {
        self.current_track_idx
    }

    pub fn try_recv_event(&self) -> Option<PlayerEvent> {
        self.rx.try_recv().ok()
    }
}
