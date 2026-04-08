//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------

mod audio_engine;
mod library;
mod scanner;
mod watcher;
mod types;
mod config;
mod app;
mod ui;

use std::{io, sync::{mpsc, Arc}, time::Duration};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture,
    Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode,
    EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use souvlaki::MediaControlEvent;

use crate::{
    app::App, audio_engine::AudioEngine, config::Config,
    library::MusicLibrary, types::AppEvent
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let library = Arc::new(MusicLibrary::new(&config.library));
    let engine = Arc::new(AudioEngine::new(&config.audio.device));
    let (tx, rx) = mpsc::channel();
    let (mpris_tx, mpris_rx) = crossbeam_channel::unbounded();

    let home = std::env::var("HOME").expect("home is boom!");
    let theme_path = std::path::PathBuf::from(home)
        .join(".config/LiPlayerPro/theme.toml");
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

    let mut app = App::new(engine, library, config.ui, mpris_tx, mpris_rx);

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
                    app.reload_theme();
                }
                _ => {}
            }
        }
        
        let mut auto_advance = false;
        if let Some(song) = &mut app.current_song {
            song.elapsed = app.engine.get_elapsed_duration().as_secs() as u32;
            if song.elapsed >= song.duration.saturating_sub(1) && song.duration > 0 {
                auto_advance = true;
            }
        }

        if auto_advance {
            if !app.current_playlist.is_empty() {
                let next = app.current_playlist_idx + 1;
                app.play_track_at(next);
            } else {
                app.stop_playback();
            }
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

        terminal.draw(|f| ui::render(f, &mut app))?;

        let fps = app.ui_config.fps.max(1);

        if event::poll(Duration::from_millis(u64::from(1000 / fps)))? 
            && let Event::Key(key) = event::read()? 
        {
            if key.code == KeyCode::Char('q') {
                app.engine.quit();
                break;
            }
            app.handle_key(key);
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
