//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------

use crossterm::event::KeyEvent;

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub path: String,
    pub duration: u32,
    pub album: String,
    pub genre: String,
    pub sample_rate: u32,
    pub bit_rate: u32,
    pub bit_depth: u32,
}

#[allow(dead_code)]
pub enum AppEvent {
    ScanProgress { current: usize, total: usize },
    ScanFinished,
    Key(KeyEvent),
    ThemeChanged,
}

#[derive(Clone)]
pub struct DisplayItem {
    pub label: String,
    pub track: Option<TrackInfo>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum View {
    Home,
    AllTracks,
    CategoriesMenu,                     // 🌟 1级：分类大菜单 (显示6个分类项)
    CategoryList(Categories),           // 🌟 2级：分类数据层 (例如：具体的歌手名字列表)
    CategoryTracks(Categories, String),
    SettingsMenu,
    SettingsDetail(Settings),
    Playlist,
}

#[derive(Clone, PartialEq, Debug)]
#[allow(dead_code)]
pub enum Categories {
    AlbumArtist,
    Album,
    Genre,
    SampleRate,
    BitRate,
    BitDepth,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Settings {
    SelectOutputDevice,
    Helps,
}

#[allow(dead_code)]
#[derive(PartialEq, Clone, Copy)]
pub enum Modal {
    None,
    Search,
    DeviceSelect,
    ExclusiveToggle,
}

pub struct SongInfo {
    pub title: String,
    pub artist: String,
    pub sample_rate: u32,
    pub elapsed: u32,
    pub duration: u32,
    pub bit_rate: u32,
    pub bit_depth: u32,
}

impl View {
    pub fn display_name(&self) -> String {
        match self {
            View::Home => "主菜单".to_string(),
            View::AllTracks => "所有曲目".to_string(),
            View::Playlist => "播放列表".to_string(),
            View::CategoriesMenu => "媒体分类".to_string(),
            View::CategoryList(c) => match c {
                Categories::AlbumArtist => "歌曲分类".to_string(),
                Categories::Album => "专辑".to_string(),
                Categories::Genre => "流派".to_string(),
                Categories::SampleRate => "采样率".to_string(),
                Categories::BitRate => "比特率".to_string(),
                Categories::BitDepth => "位深".to_string(),
            },
            View::CategoryTracks(_, name) => format!("正在查看: {}", name),
            View::SettingsMenu => "系统设置".to_string(),
            View::SettingsDetail(s) => match s {
                Settings::SelectOutputDevice => "选择输出设备".to_string(),
                Settings::Helps => "帮助".to_string(),
            },
        }
    }
}
