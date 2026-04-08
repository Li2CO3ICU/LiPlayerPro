//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------

use crossterm::style::Color;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct ThemeConfig {
    header: String,
    status_play: String,
    status_stop: String,
    warning: String,
}

#[allow(dead_code)]
pub struct Theme {
    pub header: Color,
    pub status_play: Color,
    pub status_stop: Color,
    pub warning: Color,
}

impl Theme {
    /// 从指定路径加载外部配色文件
    pub fn load(path: &str) -> Self {
        let content = fs::read_to_string(path).expect("致命错误: 无法读取外部配色文件");
        let config: ThemeConfig = toml::from_str(&content).expect("致命错误: 配色文件格式解析失败");
        
        Self {
            header: Self::hex_to_rgb(&config.header),
            status_play: Self::hex_to_rgb(&config.status_play),
            status_stop: Self::hex_to_rgb(&config.status_stop),
            warning: Self::hex_to_rgb(&config.warning),
        }
    }

    /// 纯粹的字节解析：将 "#RRGGBB" 转为 crossterm 的 RGB 结构
    fn hex_to_rgb(hex: &str) -> Color {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        
        Color::Rgb { r, g, b }
    }
}
