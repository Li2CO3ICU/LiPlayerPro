//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------

use std::{collections::BTreeMap, sync::{Arc, atomic::Ordering}, path::Path, fs};
use souvlaki::{MediaControls, MediaMetadata, MediaPlayback, PlatformConfig, MediaControlEvent};
use crossbeam_channel::Receiver;
use ratatui::widgets::ListState;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use crossterm::event::{KeyCode, KeyEvent, MediaKeyCode};

use crate::{
    audio_engine::AudioEngine, library::MusicLibrary, config::{UiConfig, StyleEngine},
    types::{View, Categories, Settings, Modal, DisplayItem, TrackInfo, SongInfo}
};

pub struct App {
    pub engine: Arc<AudioEngine>,
    pub library: Arc<MusicLibrary>,
    pub ui_config: UiConfig,
    pub is_scanning: bool,
    pub spinner_index: usize,
    pub scan_progress: (usize, usize),
    pub style: StyleEngine,
    pub view_stack: Vec<View>,
    pub sidebar_state: ListState,
    pub modal_state: ListState,
    pub items: Vec<DisplayItem>,
    pub modal_items: Vec<String>,
    pub modal: Modal,
    pub search_buf: String,
    pub current_song: Option<SongInfo>,
    pub lyrics: BTreeMap<u32, String>,
    pub current_playlist: Vec<TrackInfo>,
    pub current_playlist_idx: usize,
    pub controls: MediaControls,
    pub mpris_rx: Receiver<MediaControlEvent>,
}

impl App {
    pub fn new(
        engine: Arc<AudioEngine>,
        library: Arc<MusicLibrary>,
        ui_config: UiConfig,
        tx: crossbeam_channel::Sender<MediaControlEvent>,
        rx: Receiver<MediaControlEvent>
    ) -> Self {
        let config = PlatformConfig {
            dbus_name: "liplayerpro",
            display_name: "LiPlayer Terminal Station",
            hwnd: None,
        };
        let mut controls = MediaControls::new(config).expect("无法初始化 MPRIS");
        
        controls.attach(move |event| {
            let _ = tx.send(event);
        }).expect("无法绑定 MPRIS 事件");

        let mut app = Self {
            engine,
            library,
            ui_config,
            style: StyleEngine::new(),
            view_stack: vec![View::Home],
            sidebar_state: ListState::default().with_selected(Some(0)),
            modal_state: ListState::default(),
            items: vec![],
            modal_items: vec![],
            modal: Modal::None,
            search_buf: String::new(),
            current_song: None,
            lyrics: BTreeMap::new(),
            is_scanning: true,
            spinner_index: 0,
            scan_progress: (0, 0),
            current_playlist: Vec::new(),
            current_playlist_idx: 0,
            controls,
            mpris_rx: rx,
        };
        app.update_list();
        app
    }

    pub fn reload_theme(&mut self) {
        if let Ok(new_style) = StyleEngine::reload() {
            self.style = new_style;
        }
    }

    pub fn load_lrc(&mut self, music_path: &str) {
        self.lyrics.clear();
        let lrc_path = Path::new(music_path).with_extension("lrc");
        if let Ok(content) = fs::read_to_string(lrc_path) {
            for line in content.lines() {
                if line.starts_with('[') && line.len() > 10 {
                    let mins = line[1..3].parse::<u32>().unwrap_or(0);
                    let secs = line[4..6].parse::<u32>().unwrap_or(0);
                    let txt = line[10..].trim().to_string();
                    if !txt.is_empty() {
                        self.lyrics.insert(mins * 60 + secs, txt);
                    }
                }
            }
        }
        if self.lyrics.is_empty() {
            self.lyrics.insert(0, "暂无歌词".into());
            self.lyrics.insert(5, "...".into());
        }
    }
    
    pub fn play_track_at(&mut self, idx: usize) {
        if self.current_playlist.is_empty() { return; }
        
        let safe_idx = idx % self.current_playlist.len();
        self.current_playlist_idx = safe_idx;
        
        let track = self.current_playlist[safe_idx].clone();
        
        self.engine.play(&track.path);
        
        self.current_song = Some(SongInfo {
            title: track.title.clone(),
            artist: track.artist.clone(),
            sample_rate: track.sample_rate,
            bit_rate: track.bit_rate,
            bit_depth: track.bit_depth,
            elapsed: 0,
            duration: track.duration,
        });
        
        self.load_lrc(&track.path);
        
        let _ = self.controls.set_metadata(MediaMetadata {
            title: Some(&track.title),
            artist: Some(&track.artist),
            album: Some(&track.title), // 这里可以换成 track.album 
            duration: Some(std::time::Duration::from_secs(track.duration as u64)),
            ..Default::default()
        });
        
        let _ = self.controls.set_playback(MediaPlayback::Playing { progress: None });
        
        if let Some(view) = self.view_stack.last()
        && matches!(view, View::AllTracks | View::CategoryTracks(_, _) | View::Playlist)
        && self.items.len() == self.current_playlist.len() 
        {
        self.sidebar_state.select(Some(safe_idx));
        }
    }

    pub fn force_play(&mut self) {
        if self.current_song.is_none() {
            if !self.current_playlist.is_empty() {
                self.play_track_at(self.current_playlist_idx);
            }
            return;
        }
        self.engine.is_paused.store(false, Ordering::Release);
        let _ = self.controls.set_playback(MediaPlayback::Playing { progress: None });
    }

    pub fn force_pause(&mut self) {
        if self.current_song.is_none() { return; }
        self.engine.is_paused.store(true, Ordering::Release);
        let _ = self.controls.set_playback(MediaPlayback::Paused { progress: None });
    }

    pub fn toggle_pause(&mut self) {
        let current = self.engine.is_paused.load(Ordering::Acquire);
        if current { self.force_play(); } else { self.force_pause(); }
    }

    pub fn stop_playback(&mut self) {
        self.engine.stop();
        self.current_song = None; 
        let _ = self.controls.set_playback(MediaPlayback::Stopped);
    }

    pub fn update_list(&mut self) {
        let view = self.view_stack.last().unwrap().clone();
        let matcher = SkimMatcherV2::default();
    
        let raw_items: Vec<DisplayItem> = match view {
            View::Home => {
                vec![
                    DisplayItem { label: "󰎆 所有歌曲 (AllTracks)".into(), track: None },
                    DisplayItem { label: "󰓠 歌曲分类 (Categories)".into(), track: None },
                    DisplayItem { label: "󰲹 播放列表 (Playlist)".into(), track: None },
                    DisplayItem { label: "⚙ 系统设置 (Settings)".into(), track: None },
                ]
            }
            View::CategoriesMenu => {
                vec![
                    DisplayItem { label: "󰠃 专辑艺术家".into(), track: None },
                    DisplayItem { label: "󰀥 专辑".into(), track: None },
                    DisplayItem { label: "󰔊 流派".into(), track: None },
                    DisplayItem { label: "󰓡 采样率".into(), track: None },
                    DisplayItem { label: "󰈈 比特率".into(), track: None },
                    DisplayItem { label: "󰾆 位深".into(), track: None },
                ]
            }
            View::CategoryList(ref cat) => {
                let string_list = match cat {
                    Categories::AlbumArtist => self.library.get_distinct_artists(),
                    Categories::Album => self.library.get_distinct_albums(),
                    Categories::Genre => self.library.get_distinct_genres(),
                    Categories::SampleRate => self.library.get_distinct_sample_rates(),
                    Categories::BitRate => self.library.get_distinct_bit_rates(),
                    Categories::BitDepth => self.library.get_distinct_bit_depths(),
                };
                string_list.into_iter().map(|s| DisplayItem { label: s, track: None }).collect()
            }
            View::CategoryTracks(ref cat, ref val) => {
                self.library.get_tracks_by_category(cat, val)
                    .into_iter()
                    .map(|t| DisplayItem { label: format!("{} - {}", t.title, t.artist), track: Some(t) })
                    .collect()
            }
            View::SettingsMenu => {
                vec![
                    DisplayItem { label: "󰔊 设备选择 (Output Device)".into(), track: None },
                    DisplayItem { label: " 帮助 (Helps)".into(), track: None },
                ]
            }
            View::AllTracks => {
                self.library.get_all_tracks()
                    .into_iter()
                    .map(|t| DisplayItem { label: format!("{} - {}", t.title, t.artist), track: Some(t) })
                    .collect()
            }
            _ => vec![],
        };
    
        self.items = if self.modal == Modal::Search && !self.search_buf.is_empty() {
            let mut filtered: Vec<(i64, DisplayItem)> = raw_items
                .into_iter()
                .filter_map(|i| {
                    matcher
                        .fuzzy_match(&i.label, &self.search_buf)
                        .map(|score| (score, i))
                })
                .collect();
            filtered.sort_by(|a, b| b.0.cmp(&a.0));
            filtered.into_iter().map(|(_, i)| i).collect()
        } else {
            raw_items
        };
    
        if !self.items.is_empty() {
            let curr = self.sidebar_state.selected().unwrap_or(0);
            self.sidebar_state.select(Some(curr.min(self.items.len().saturating_sub(1))));
        } else {
            self.sidebar_state.select(None);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.modal != Modal::None {
            match key.code {
                KeyCode::Esc => self.modal = Modal::None,
                KeyCode::Char(c) if self.modal == Modal::Search => {
                    self.search_buf.push(c);
                    self.update_list();
                }
                KeyCode::Backspace if self.modal == Modal::Search => {
                    self.search_buf.pop();
                    self.update_list();
                }
                KeyCode::Up => {
                    if !self.modal_items.is_empty() {
                        let i = self.modal_state.selected().map_or(0, |i| {
                            if i == 0 { self.modal_items.len() - 1 } else { i - 1 }
                        });
                        self.modal_state.select(Some(i));
                    }
                }
                KeyCode::Down => {
                    if !self.modal_items.is_empty() {
                        // 加上非空检查，彻底告别 % 0 崩溃！
                        let i = self.modal_state.selected().map_or(0, |i| (i + 1) % self.modal_items.len());
                        self.modal_state.select(Some(i));
                    }
                }
                KeyCode::Enter => {
                    if self.modal == Modal::Search {
                        self.modal = Modal::None;
                    } else {
                        self.update_list();
                    }
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('f') => {
                self.modal = Modal::Search;
                self.search_buf.clear();
            }
            KeyCode::Esc |KeyCode::Left| KeyCode::Char('b') => {
                if self.view_stack.len() > 1 {
                    self.view_stack.pop();
                    self.update_list();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let i = self.sidebar_state.selected().map_or(0, |i| {
                    if i >= self.items.len().saturating_sub(1) { 0 } else { i + 1 }
                });
                self.sidebar_state.select(Some(i));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = self.sidebar_state.selected().map_or(0, |i| {
                    if i == 0 { self.items.len().saturating_sub(1) } else { i - 1 }
                });
                self.sidebar_state.select(Some(i));
            }
            KeyCode::Char('n') | KeyCode::Media(MediaKeyCode::TrackNext) => {
                if !self.current_playlist.is_empty() {
                    let next = self.current_playlist_idx + 1;
                    self.play_track_at(next);
                }
            }
            KeyCode::Char('p') | KeyCode::Media(MediaKeyCode::TrackPrevious) => {
                if !self.current_playlist.is_empty() {
                    let prev = if self.current_playlist_idx == 0 {
                        self.current_playlist.len().saturating_sub(1)
                    } else {
                        self.current_playlist_idx - 1
                    };
                    self.play_track_at(prev);
                }
            }
            KeyCode::Char(' ') | KeyCode::Media(MediaKeyCode::PlayPause) => self.toggle_pause(),
            KeyCode::Media(MediaKeyCode::Play) => self.force_play(),
            KeyCode::Media(MediaKeyCode::Pause) => self.force_pause(),
            KeyCode::Media(MediaKeyCode::Stop) => self.stop_playback(),
            KeyCode::Enter | KeyCode::Right => self.handle_enter(),
            _ => {}
        }
    }

    fn handle_enter(&mut self) {
        let old_len = self.view_stack.len();
        let idx = self.sidebar_state.selected().unwrap_or(0);
        if self.items.is_empty() { return; }
        let cur_view = self.view_stack.last().unwrap().clone();

        match cur_view {
            View::Home => {
                match idx {
                    0 => self.view_stack.push(View::AllTracks),
                    1 => self.view_stack.push(View::CategoriesMenu), 
                    2 => self.view_stack.push(View::Playlist),
                    3 => self.view_stack.push(View::SettingsMenu),
                    _ => {}
                }
                self.update_list();
            }
            View::CategoriesMenu => {
                let cat = match idx {
                    0 => Categories::AlbumArtist,
                    1 => Categories::Album,
                    2 => Categories::Genre,
                    3 => Categories::SampleRate,
                    4 => Categories::BitRate,
                    5 => Categories::BitDepth,
                    _ => Categories::AlbumArtist,
                };
                self.view_stack.push(View::CategoryList(cat));
                self.update_list();
            }
            View::CategoryList(cat) => {
                let selected_val = self.items[idx].label.clone();
                if selected_val != "(暂未实现此分类)" {
                    self.view_stack.push(View::CategoryTracks(cat, selected_val));
                    self.update_list();
                }
            }
            View::SettingsMenu => {
                match idx {
                    0 => self.view_stack.push(View::SettingsDetail(Settings::SelectOutputDevice)),
                    1 => self.view_stack.push(View::SettingsDetail(Settings::Helps)),
                    _ => {}
                }
                self.update_list();
            }
            View::AllTracks | View::CategoryTracks(_, _) | View::Playlist => {
                self.current_playlist = self.items.iter().filter_map(|i| i.track.clone()).collect();
                self.play_track_at(idx);
            }
            _ => {}
        }
        if self.view_stack.len() != old_len {
            self.sidebar_state.select(Some(0));
        }
    }
}
