//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------
use notify::{Watcher, RecursiveMode, RecommendedWatcher};
use std::path::Path;
use std::sync::mpsc::Sender;
use crate::AppEvent;

pub fn spawn_watcher(path: &Path, tx: Sender<AppEvent>) -> RecommendedWatcher {
    let tx_clone = tx.clone();
    
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(_) = res {
            // 文件动了，拍拍主线程喵！
            let _ = tx_clone.send(AppEvent::ThemeChanged);
        }
    }).expect("error:watcher error");

    watcher.watch(path, RecursiveMode::NonRecursive).expect("盯不住那个文件喵");
    
    watcher
}
