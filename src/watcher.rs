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
