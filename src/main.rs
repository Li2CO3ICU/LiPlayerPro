mod audio_engine;
mod library;
mod scanner;
mod watcher;

use serde::Deserialize;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, PlatformConfig, MediaPlayback};
use crossbeam_channel::Receiver;
use crate::audio_engine::AudioEngine;
use crate::library::MusicLibrary;
use crossterm::{
    event::{MediaKeyCode, self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}, // 🌟 补上这些函数
};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::collections::{BTreeMap, HashMap};
use std::{fs, io};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use std::sync::atomic::Ordering;

#[derive(Deserialize, Clone)]
pub struct Config {

    pub library: LibraryConfig,
    pub audio: AudioConfig,
    pub ui: UiConfig,
}

#[derive(Deserialize, Clone)]
pub struct LibraryConfig {
    pub music_dir: String,
    pub index_path: String,
}


#[derive(Deserialize, Clone)]
pub struct AudioConfig {
    pub device: String,
}

#[derive(Deserialize, Clone)]
pub struct UiConfig {
    pub fps: u32,
}

impl Config {
    pub fn load() -> Self {
        let home = std::env::var("HOME").expect("home is boom!");
        let config_path = PathBuf::from(home)
            .join(".config/LiPlayerPro/config.toml");
        
        let context = fs::read_to_string(config_path)
            .expect("No found config.toml");

        let mut config: Self = toml::from_str(&context)
            .expect("config format error! 配置文件格式不对喵");
        let expanded_index = shellexpand::tilde(&config.library.index_path).to_string();
        config.library.index_path = expanded_index;

        // 处理音乐目录路径喵 (预防万一用户在这里也写了 ~)
        let expanded_music = shellexpand::tilde(&config.library.music_dir).to_string();
        config.library.music_dir = expanded_music;

        config
    }
}



// ==========================================
// 0. 数据绑定结构 (打通底层数据的桥梁)
// ==========================================
#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub path: String,
    pub duration: u32,
    pub sample_rate: u32,
    pub bit_rate: u32,
    pub bit_depth: u32,
}

#[allow(dead_code)]
enum AppEvent {
    ScanProgress { current: usize, total: usize },
    ScanFinished,
    Key(KeyEvent),
    // ScanFinished,
    ThemeChanged,
}

#[derive(Clone)]
struct DisplayItem {
    label: String,
    track: Option<TrackInfo>,
}

// ==========================================
// 1. 主题引擎
// ==========================================
#[derive(serde::Deserialize, Clone, Default)]
struct Theme {
    colors: HashMap<String, String>,
}

struct StyleEngine {
    theme: Theme,
}

impl StyleEngine {
    fn new() -> Self {
        Self::reload().unwrap_or_else(|_| {
            Self { 
                theme: Theme::default() 
            }
        })
    }
    
    fn reload() -> Result<Self, Box<dyn std::error::Error>> {
        let home = std::env::var("HOME")?;
        let theme_path = std::path::PathBuf::from(home).join(".config/LiPlayerPro/theme.toml");
        
        if !theme_path.exists() {
            return Ok(Self { theme: Theme::default() });
        }

        let theme_str = fs::read_to_string(theme_path)?;
        let theme: Theme = toml::from_str(&theme_str)?;
        Ok(Self { theme })
    }

    fn get(&self, key: &str) -> Style {
        match self.theme.colors.get(key) {
            Some(hex) if hex.len() == 7 && hex.starts_with('#') => {
                let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(255);
                let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(255);
                let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(255);
                Style::default().fg(Color::Rgb(r, g, b))
            }
            _ => Style::default(),
        }
    }
}

// ==========================================
// 2. 状态定义
// ==========================================
#[derive(Clone, PartialEq, Debug)]
enum View {
    AllTracks,
}

#[allow(dead_code)]
#[derive(PartialEq, Clone, Copy)]
enum Modal {
    None,
    Search,
    DeviceSelect,
    ExclusiveToggle,
}

struct SongInfo {
    title: String,
    artist: String,
    sample_rate: u32,
    elapsed: u32,
    duration: u32,
    bit_rate: u32,
    bit_depth: u32,
}

// ==========================================
// 3. 应用逻辑
// ==========================================
struct App {
    pub engine: Arc<AudioEngine>,
    pub library: Arc<MusicLibrary>,
    pub ui_config: crate::UiConfig,
    pub is_scanning: bool,
    pub spinner_index: usize,
    pub scan_progress: (usize, usize),
    style: StyleEngine,
    view_stack: Vec<View>,
    sidebar_state: ListState,
    modal_state: ListState,
    items: Vec<DisplayItem>,
    modal_items: Vec<String>,
    modal: Modal,
    search_buf: String,
    current_song: Option<SongInfo>,
    lyrics: BTreeMap<u32, String>,
    current_playlist: Vec<TrackInfo>,
    current_playlist_idx: usize,
    controls: MediaControls,
    mpris_rx: Receiver<MediaControlEvent>,
}

impl App {
    pub fn new(
        engine: Arc<AudioEngine>,
        library: Arc<MusicLibrary>,
        ui_config: crate::UiConfig 
    ) -> Self {
        let config = PlatformConfig {
            dbus_name: "liplayerpro",
            display_name: "LiPlayer Terminal Station",
            hwnd: None,
        };
        let mut controls = MediaControls::new(config).expect("无法初始化 MPRIS");
        let (tx, rx) = crossbeam_channel::unbounded();
         
        controls.attach(move |event| {
            let _ = tx.send(event);
        }).expect("无法绑定 MPRIS 事件");
        let mut app = Self {
            engine,
            library,
            ui_config,
            style: StyleEngine::new(),
            view_stack: vec![View::AllTracks],
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

    fn reload_theme(&mut self) {
        if let Ok(new_style) = StyleEngine::reload() {
            self.style = new_style;
        }
    }

    fn load_lrc(&mut self, music_path: &str) {
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
    
    fn play_track_at(&mut self, idx: usize) {
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
            album: Some(&track.title),
            duration: Some(std::time::Duration::from_secs(track.duration as u64)),
            ..Default::default()
        });
        
        let _ = self.controls.set_playback(MediaPlayback::Playing { progress: None });
        
        if let Some(View::AllTracks) = self.view_stack.last() {
            if self.items.len() == self.current_playlist.len() {
                self.sidebar_state.select(Some(safe_idx));
            }
        }
    }

    // 🌟 1. 强制播放（严格对应 playerctl play）
    fn force_play(&mut self) {
        if self.current_song.is_none() {
            // 如果是被 Stop 彻底掐断了引擎，再次按下播放时，重新拉起当前歌单的歌
            if !self.current_playlist.is_empty() {
                self.play_track_at(self.current_playlist_idx);
            }
            return;
        }
        self.engine.is_paused.store(false, Ordering::Release);
        let _ = self.controls.set_playback(MediaPlayback::Playing { progress: None });
    }

    // 🌟 2. 强制暂停（严格对应 playerctl pause）
    fn force_pause(&mut self) {
        if self.current_song.is_none() { return; }
        self.engine.is_paused.store(true, Ordering::Release);
        let _ = self.controls.set_playback(MediaPlayback::Paused { progress: None });
    }

    // 🌟 3. 智能反转（留给键盘空格键使用）
    fn toggle_pause(&mut self) {
        let current = self.engine.is_paused.load(Ordering::Acquire);
        if current { self.force_play(); } else { self.force_pause(); }
    }

    // 🌟 4. 彻底停止
    fn stop_playback(&mut self) {
        self.engine.stop();
        self.current_song = None; // 彻底清空状态栏
        let _ = self.controls.set_playback(MediaPlayback::Stopped);
    }

    fn update_list(&mut self) {
        let view = self.view_stack.last().unwrap().clone();
        let matcher = SkimMatcherV2::default();

        let raw_items: Vec<DisplayItem> = match view {
            View::AllTracks => {
                let tracks = self.library.get_all_tracks();
                tracks
                    .into_iter()
                    .map(|t| DisplayItem {
                        label: format!("{} - {}", t.title, t.artist),
                        track: Some(t),
                    })
                    .collect()
            }
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
            self.sidebar_state
                .select(Some(curr.min(self.items.len().saturating_sub(1))));
        }
    }
}

// ==========================================
// 4. 渲染引擎
// ==========================================
fn render(f: &mut Frame, app: &mut App) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);

    let title_text = if app.is_scanning {
        format!(" LiPlayer Pro | ♪ Indexing... ")
    } else {
        format!(
            " LiPlayer Pro | ♪ {} Tracks ",
            app.library.get_track_count()
        )
    };

    f.render_widget(
        Paragraph::new(title_text)
            .alignment(Alignment::Center)
            .style(app.style.get("primary").add_modifier(Modifier::BOLD)),
        chunks[0],
    );

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[1]);

    if app.is_scanning {
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Length(3),
                Constraint::Percentage(45),
            ])
            .split(chunks[1]);

        let area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .split(v_chunks[1])[1];

        let (cur, tot) = app.scan_progress;
        let ratio = if tot > 0 {
            (cur as f64 / tot as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let frame = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][app.spinner_index % 10];
        let label = format!(" {} Loading...: {} / {} ", frame, cur, tot);

        f.render_widget(
            Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(app.style.get("secondary")),
                )
                .gauge_style(app.style.get("primary"))
                .label(label)
                .ratio(ratio),
            area,
        );
    } else {
        // --- 只有不扫描时，才渲染原本的歌曲列表 ---
        let list_items: Vec<ListItem> = app
            .items
            .iter()
            .map(|i| ListItem::new(i.label.as_str()).style(app.style.get("primary")))
            .collect();

        let view_name = format!(" {:?} ", app.view_stack.last().unwrap()).to_uppercase();

        f.render_stateful_widget(
            List::new(list_items)
                .block(
                    Block::default()
                        .title(Span::styled(
                            format!(" {} ", view_name),
                            app.style.get("primary"),
                        ))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(app.style.get("secondary")),
                )
                .highlight_style(app.style.get("border_active").add_modifier(Modifier::BOLD))
                .highlight_symbol("> "),
            body[0],
            &mut app.sidebar_state,
        );

        if let Some(song) = &app.current_song {
            let mut lines = Vec::new();
            let lyric_keys: Vec<_> = app.lyrics.keys().cloned().collect();
            let active_idx = lyric_keys
                .iter()
                .position(|&t| t > song.elapsed)
                .unwrap_or(lyric_keys.len()) as i32
                - 1;

            let center_offset = (body[1].height / 2).saturating_sub(1) as usize;
            for _ in 0..center_offset {
                lines.push(Line::from(""));
            }

            for (idx, (_, txt)) in app.lyrics.iter().enumerate() {
                let style = if idx as i32 == active_idx {
                    app.style.get("lyric_present").add_modifier(Modifier::BOLD)
                } else if (idx as i32) < active_idx {
                    app.style.get("lyric_past").add_modifier(Modifier::DIM)
                } else {
                    app.style.get("lyric_future")
                };
                lines.push(
                    Line::from(Span::styled(txt.clone(), style)).alignment(Alignment::Center),
                );
            }

            f.render_widget(
                Paragraph::new(lines)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(app.style.get("secondary"))
                            .title(" LYRICS "),
                    )
                    .scroll((active_idx.max(0) as u16, 0)),
                body[1],
            );
        } else {
            f.render_widget(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(app.style.get("secondary"))
                    .title(" LYRICS "),
                body[1],
            );
        }

        if let Some(song) = &mut app.current_song {
            let duration = song.duration.max(1) as f64;
            song.elapsed = app.engine.get_elapsed_duration().as_secs() as u32;
            let ratio = (song.elapsed as f64 / duration).clamp(0.0, 1.0);
            let width = size.width.saturating_sub(1) as usize;
            let pos = (ratio * width as f64) as usize;
            let bar = format!(
                "{}{}{}",
                "━".repeat(pos),
                "●",
                "─".repeat(width.saturating_sub(pos + 1))
            );
            let state_str = if app.engine.is_paused.load(Ordering::Relaxed) { "⏸  PAUSED"} else { "▶ PLAYING"};
            let meta = format!(
                "{} |{:02}:{:02} |{} - {} |{}Bit/{}Hz/{}kbps| {:02}:{:02}",
                state_str,
                song.elapsed / 60, song.elapsed % 60,
                song.title, song.artist,
                song.bit_depth, song.sample_rate, song.bit_rate / 1000,
                song.duration / 60, song.duration % 60
            );

            f.render_widget(
                Paragraph::new(format!("{}\n{}", bar, meta))
                    .alignment(Alignment::Center)
                    .style(app.style.get("progress_bar")),
                chunks[2],
            );
        }
    
        if app.modal != Modal::None {
            let area = Rect::new(
                (size.width as f32 * 0.2) as u16,
                size.height / 2 - 1,
                (size.width as f32 * 0.6) as u16,
                3,
            );
            f.render_widget(Clear, area);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(app.style.get("border_active"));
            match app.modal {
                Modal::Search => f.render_widget(
                    Paragraph::new(format!(" 搜索: {}_", app.search_buf)).block(block),
                    area,
                ),
                Modal::DeviceSelect | Modal::ExclusiveToggle => {
                    let items: Vec<ListItem> = app
                        .modal_items
                        .iter()
                        .map(|i| ListItem::new(i.as_str()).style(app.style.get("primary")))
                        .collect();
                    f.render_stateful_widget(
                        List::new(items)
                            .block(block)
                            .highlight_style(app.style.get("border_active")),
                        area,
                        &mut app.modal_state,
                    );
                }
                _ => {}
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let library = Arc::new(MusicLibrary::new(&config.library));
    let engine = Arc::new(AudioEngine::new(&config.audio.device));
    let (tx, rx) = mpsc::channel();

    let home = std::env::var("HOME").expect("home is boom!");
    let theme_path = std::path::PathBuf::from(home).join(".config/LiPlayerPro/theme.toml");
    let _watcher = watcher::spawn_watcher(&theme_path, tx.clone());

    let track_count = library.get_track_count();

    if track_count == 0 {
        let tx_clone = tx.clone();
        let lib_clone = Arc::clone(&library);
        std::thread::spawn(move || {
            lib_clone.build_index("/home/Li2CO3/Music/", tx_clone);
        });
    } else {
        let _ = tx.send(AppEvent::ScanFinished);
    }

    let mut app = App::new(engine, library, config.ui);

    loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::ScanProgress { current, total } => {
                    app.scan_progress = (current, total);
                }
                AppEvent::ScanFinished => {
                    app.is_scanning = false;
                    app.update_list();
                }
                AppEvent::ThemeChanged => {
                    // 🌟 收到信号，重载主题！
                    app.reload_theme();
                }
                _ => {}
            }
        }

        if let Some(song) = &mut app.current_song {
            song.elapsed = app.engine.get_elapsed_duration().as_secs() as u32;
        }

        if app.is_scanning {
            app.spinner_index = app.spinner_index.wrapping_add(1);
        }

        while let Ok(mpris_evt) = app.mpris_rx.try_recv() {
            match mpris_evt {
                MediaControlEvent::Next => {
                    if !app.current_playlist.is_empty() {
                        let next = app.current_playlist_idx + 1;
                        app.play_track_at(next);
                    }
                }
                MediaControlEvent::Previous => {
                    if !app.current_playlist.is_empty() {
                        let prev = if app.current_playlist_idx == 0 {
                            app.current_playlist.len().saturating_sub(1)
                        } else {
                            app.current_playlist_idx - 1
                        };
                        app.play_track_at(prev);
                    }
                }
                MediaControlEvent::Play => app.force_play(),
                MediaControlEvent::Pause => app.force_pause(),
                MediaControlEvent::Toggle => app.toggle_pause(),
                MediaControlEvent::Stop => app.stop_playback(),
                _ => {}
            }
        }

        terminal.draw(|f| render(f, &mut app))?;

        if event::poll(Duration::from_millis(16))? {
            // 60FPS
            if let Event::Key(key) = event::read()? {
                if app.modal != Modal::None {
                    match key.code {
                        KeyCode::Esc => app.modal = Modal::None,
                        KeyCode::Char(c) if app.modal == Modal::Search => {
                            app.search_buf.push(c);
                            app.update_list();
                        }
                        KeyCode::Backspace if app.modal == Modal::Search => {
                            app.search_buf.pop();
                            app.update_list();
                        }
                        KeyCode::Up => {
                            let i = app.modal_state.selected().map_or(0, |i| {
                                if i == 0 {
                                    app.modal_items.len() - 1
                                } else {
                                    i - 1
                                }
                            });
                            app.modal_state.select(Some(i));
                        }
                        KeyCode::Down => {
                            let i = app
                                .modal_state
                                .selected()
                                .map_or(0, |i| (i + 1) % app.modal_items.len());
                            app.modal_state.select(Some(i));
                        }
                        KeyCode::Enter => {
                            if app.modal == Modal::Search {
                                app.modal = Modal::None;
                            } else {
                                app.update_list();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => {
                        app.engine.quit();
                        break;
                    }
                    KeyCode::Char('f') => {
                        app.modal = Modal::Search;
                        app.search_buf.clear();
                    }

                    KeyCode::Esc | KeyCode::Char('b') => {
                        if app.view_stack.len() > 1 {
                            app.view_stack.pop();
                            app.update_list();
                        }
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        let i = app.sidebar_state.selected().map_or(0, |i| {
                            if i >= app.items.len().saturating_sub(1) {
                                0
                            } else {
                                i + 1
                            }
                        });
                        app.sidebar_state.select(Some(i));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let i = app.sidebar_state.selected().map_or(0, |i| {
                            if i == 0 {
                                app.items.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        });
                        app.sidebar_state.select(Some(i));
                    }
                    
                    KeyCode::Char('n') | KeyCode::Media(MediaKeyCode::TrackNext) => {
                        if !app.current_playlist.is_empty() {
                            let next = app.current_playlist_idx + 1;
                            app.play_track_at(next);
                        }
                    }

                    KeyCode::Char('p') | KeyCode::Media(MediaKeyCode::TrackPrevious)=> {
                        if !app.current_playlist.is_empty() {
                            let prev = if app.current_playlist_idx == 0 {
                                app.current_playlist.len().saturating_sub(1)
                            } else {
                                app.current_playlist_idx - 1
                            };
                            app.play_track_at(prev);
                        }
                    }

                    KeyCode::Char(' ') | KeyCode::Media(MediaKeyCode::PlayPause) => app.toggle_pause(),
                    KeyCode::Media(MediaKeyCode::Play) => app.force_play(),
                    KeyCode::Media(MediaKeyCode::Pause) => app.force_pause(),
                    KeyCode::Media(MediaKeyCode::Stop) => app.stop_playback(),

                    KeyCode::Enter => {
                        let idx = app.sidebar_state.selected().unwrap_or(0);
                        if app.items.is_empty() {
                            continue;
                        }
                        let cur_view = app.view_stack.last().unwrap().clone();

                        match cur_view {
                            View::AllTracks => {
                                app.current_playlist = app.items.iter()
                                    .filter_map(|i| i.track.clone())
                                    .collect();
                                app.play_track_at(idx);
                                if let Some(track) = app.items[idx].track.clone() {
                                    app.engine.play(&track.path);

                                    app.current_song = Some(SongInfo {
                                        title: track.title,
                                        artist: track.artist,
                                        sample_rate: track.sample_rate,
                                        elapsed: 0,
                                        duration: track.duration,
                                        bit_rate: track.bit_rate,
                                        bit_depth: track.bit_depth,
                                    });
                                    app.load_lrc(&track.path);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}
