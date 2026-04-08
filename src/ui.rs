//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table},
    Frame,
};
use crate::app::App;
use crate::types::{Modal, Settings, View};

const HELP_KEYBINDS: &[(&str, &str)] = &[
    ("Q", "退出程序"),
    ("Enter", "播放选中/进入菜单"),
    ("J/Down", "向下移动"),
    ("K/Up", "向上移动"),
    ("P", "上一曲"),
    ("Space", "暂停/播放"),
    ("N", "下一曲"),
    ("F", "搜索"),
];

pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);

    let current_view = app.view_stack.last().unwrap_or(&View::AllTracks);
    let title_text = if app.is_scanning {
        format!(" LiPlayer Pro | ♪ 正在索引曲库... ")
    } else {
        format!(
            " LiPlayer Pro | {} | ♪ {} 首曲目 ",
            current_view.display_name(),
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
        let frame = [
            "⠋", "⠙", "⠹", "⠸", "⠼",
            "⠴", "⠦", "⠧", "⠇", "⠏"
        ][app.spinner_index % 10];
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
        if let View::SettingsDetail(Settings::Helps) = current_view {
            let full_area = body[0].union(body[1]);
            render_help_view(f, full_area, app);
        } else {
            let view_name = current_view.display_name().to_uppercase();
            let list_items: Vec<ListItem> = app.items.iter()
                .map(|i| ListItem::new(i.label.as_str()).style(app.style.get("primary")))
                .collect();

            f.render_stateful_widget(
                List::new(list_items)
                    .block(Block::default()
                        .title(Span::styled(format!(" {} ", view_name), app.style.get("primary")))
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(app.style.get("secondary")))
                    .highlight_style(app.style.get("border_active").add_modifier(Modifier::BOLD))
                    .highlight_symbol("> "),
                body[0],
                &mut app.sidebar_state,
            );

            if matches!(current_view, View::AllTracks | View::Playlist) {
                render_lyrics_view(f, body[1], app);
            } else {
                f.render_widget(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(app.style.get("secondary")).title(" INFO "), body[1]);
            }
        }
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
        let state_str = if app.engine.is_paused.load(std::sync::atomic::Ordering::Relaxed) { "⏸  PAUSED"} else { "▶ PLAYING"};
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
                let items: Vec<ListItem> = app.modal_items.iter()
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

fn render_lyrics_view(f: &mut Frame, area: Rect, app: &App) {
    if let Some(song) = &app.current_song {
        let mut lines = Vec::new();
        let lyric_keys: Vec<_> = app.lyrics.keys().cloned().collect();
        let active_idx = lyric_keys
            .iter()
            .position(|&t| t > song.elapsed)
            .unwrap_or(lyric_keys.len()) as i32 - 1;

        let center_offset = (area.height / 2).saturating_sub(1) as usize;
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
            lines.push(Line::from(Span::styled(txt.clone(), style)).alignment(Alignment::Center));
        }

        f.render_widget(
            Paragraph::new(lines)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(app.style.get("secondary"))
                    .title(" 歌词 "))
                .scroll((active_idx.max(0) as u16, 0)),
            area,
        );
    } else {
        f.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(app.style.get("secondary"))
                .title(" 歌词 "),
            area,
        );
    }
}

fn render_help_view(f: &mut Frame, area: Rect, app: &App) {
    let rows: Vec<Row> = HELP_KEYBINDS.iter()
        .map(|(k, v)| {
            Row::new(vec![
                Cell::from(Span::styled(*k, app.style.get("primary").add_modifier(Modifier::BOLD))),
                Cell::from(Span::raw(*v)),
            ])
        }).collect();

    let table = Table::new(rows, [Constraint::Percentage(30), Constraint::Percentage(70)])
        .block(Block::default()
            .title("快捷键帮助|Esc返回")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(app.style.get("secondary")));
    f.render_widget(table, area);
}
