//                   Copyright (C) 2026 Li2CO3ICU
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// -----------------------------------------------------------------------

#![allow(unused_imports, dead_code)]
use crate::types::TrackInfo;
use crate::config::LibraryConfig;
use crate::scanner::scan_track;
use crate::AppEvent;
use rayon::prelude::*;
use tantivy::time::format_description::well_known::iso8601::Config;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::path::Path;
use tantivy::directory::{self, MmapDirectory};
use tantivy::schema::*;
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use tantivy::schema::{
    IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument};
use walkdir::WalkDir;

pub struct MusicLibrary {
    index: Index,
    reader: IndexReader,
    f_title: tantivy::schema::Field,
    f_artist: tantivy::schema::Field,
    f_album: Field,
    f_genre: Field,
    f_path: tantivy::schema::Field,
    f_duration: tantivy::schema::Field,    
    f_sample_rate: tantivy::schema::Field,
    f_bit_rate: tantivy::schema::Field,
    f_bit_depth:tantivy::schema::Field,
    pub tracks: Arc<RwLock<Vec<TrackInfo>>>,
}

impl MusicLibrary {
    pub fn new(config: &LibraryConfig) -> Self {

        let index_path = Path::new(&config.index_path);

        std::fs::create_dir_all(&index_path).expect("创建目录失败喵...");

        let mut schema_builder = Schema::builder();
        
        let cjk_text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("cjk_ngram") // 关联分析器名称
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        let f_title = schema_builder.add_text_field("title", cjk_text_options.clone());
        let f_artist = schema_builder.add_text_field("artist", cjk_text_options.clone());
        let f_album = schema_builder.add_text_field("album", cjk_text_options.clone());
        let f_genre = schema_builder.add_text_field("genre", cjk_text_options.clone());
        let f_path = schema_builder.add_text_field("path", STRING | STORED);
        let f_duration = schema_builder.add_u64_field("duration", STORED);
        let f_sample_rate = schema_builder.add_u64_field("sample_rate", STORED);
        let f_bit_rate = schema_builder.add_u64_field("bit_rate", STORED);
        let f_bit_depth = schema_builder.add_u64_field("bit_depth", STORED);
        let schema = schema_builder.build();
        
        let directory = MmapDirectory::open(&index_path).expect("打不开索引目录喵");
        let index = Index::open_or_create(directory, schema).expect("索引初始化失败喵");

        let ngram_analyzer = TextAnalyzer::builder(NgramTokenizer::new(1, 2, false).unwrap())
            .filter(LowerCaser)
            .build();
        index.tokenizers().register("cjk_ngram", ngram_analyzer);

        let reader = index
            .reader_builder()
            .try_into()
            .expect("无法创建 IndexReader");

        Self {
            index,
            reader,
            f_title,
            f_artist,
            f_album,
            f_genre,
            f_path,
            f_duration,
            f_sample_rate,
            f_bit_rate,
            f_bit_depth,
            tracks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn build_index(&self, music_dir: &str, tx: std::sync::mpsc::Sender<AppEvent>) {
        let paths: Vec<String> = WalkDir::new(music_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let p = e.path();
                let ext = p.extension()?.to_str()?.to_lowercase();
                if ["flac", "wav", "dsf", "dff", "m4a", "mp3"].contains(&ext.as_str()) {
                    Some(p.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();

        let total = paths.len();
        if total == 0 {
            let _ = tx.send(AppEvent::ScanFinished);
            return;
        }

        let mut index_writer = self.index.writer(200_000_000).expect("Writer failed");
        
        let counter = AtomicUsize::new(0);
        let tx_ref = &tx;
        let counter_ref = &counter;

        let _ : Vec<()> = paths
            .into_par_iter()
            .filter_map(|p| {
                let meta = crate::scanner::scan_track(&p)?;
                
                let term = Term::from_field_text(self.f_path, &meta.path);
                index_writer.delete_term(term); 

                let mut doc = TantivyDocument::default();
                doc.add_text(self.f_title, &meta.title);
                doc.add_text(self.f_artist, &meta.artist);
                doc.add_text(self.f_album, &meta.album);
                doc.add_text(self.f_genre, &meta.genre);
                doc.add_text(self.f_path, &meta.path);
                doc.add_u64(self.f_duration, meta.duration as u64);
                doc.add_u64(self.f_sample_rate, meta.sample_rate as u64);
                doc.add_u64(self.f_bit_rate, meta.bit_rate as u64);
                doc.add_u64(self.f_bit_depth, meta.bit_depth as u64);
                
                index_writer.add_document(doc).ok()?;

                let current = counter_ref.fetch_add(1, Ordering::SeqCst) + 1;
                if current % 5 == 0 || current == total {
                    let _ = tx_ref.send(AppEvent::ScanProgress { current, total });
                }
                Some(())
            })
            .collect();

        index_writer.commit().expect("Commit failed");
        
        self.reader.reload().expect("Reload failed");
    
        let _ = tx.send(AppEvent::ScanFinished);
    }
    pub fn get_all_tracks(&self) -> Vec<TrackInfo> {
        let searcher = self.reader.searcher();
        let top_docs = searcher
            .search(&AllQuery, &TopDocs::with_limit(10000))
            .unwrap();

        let mut results = Vec::new();
        for (_, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address).unwrap();
            results.push(TrackInfo {
                title: doc
                    .get_first(self.f_title)
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                artist: doc
                    .get_first(self.f_artist)
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string(),
                path: doc
                    .get_first(self.f_path)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                duration: doc
                    .get_first(self.f_duration)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                album: doc
                    .get_first(self.f_album) 
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Album")
                    .to_string(),
                genre: doc
                    .get_first(self.f_genre)
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Genre")
                    .to_string(),
                sample_rate: doc
                    .get_first(self.f_sample_rate)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(44100) as u32,
                bit_rate: doc
                    .get_first(self.f_bit_rate)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                bit_depth: doc
                    .get_first(self.f_bit_depth)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
            });
        }
        results.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        results
    }

    pub fn get_track_count(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }

    pub fn get_distinct_artists(&self) -> Vec<String> {
        let tracks = self.get_all_tracks(); 
        let mut artists: Vec<String> = tracks.into_iter().map(|t| t.artist).collect();
        artists.sort();
        artists.dedup(); 
        artists
    }

    pub fn get_distinct_sample_rates(&self) -> Vec<String> {
        let tracks = self.get_all_tracks();
        let mut rates: Vec<u32> = tracks.into_iter().map(|t| t.sample_rate).collect();
        rates.sort();
        rates.dedup();
        rates.into_iter().map(|r| format!("{} Hz", r)).collect()
    }

    pub fn get_distinct_bit_depths(&self) -> Vec<String> {
        let tracks = self.get_all_tracks();
        let mut depths: Vec<u32> = tracks.into_iter().map(|t| t.bit_depth).collect();
        depths.sort();
        depths.dedup();
        depths.into_iter().map(|d| format!("{} Bit", d)).collect()
    }

    pub fn get_distinct_albums(&self) -> Vec<String> {
        let tracks = self.get_all_tracks();
        let mut albums: Vec<String> = tracks.into_iter()
            .map(|t| if t.album.is_empty() { "Unknown Album".into() } else { t.album })
            .collect();
        albums.sort();
        albums.dedup();
        albums
    }

    pub fn get_distinct_genres(&self) -> Vec<String> {
        let tracks = self.get_all_tracks();
        let mut genres: Vec<String> = tracks.into_iter()
            .map(|t| if t.genre.is_empty() { "Unknown Genre".into() } else { t.genre })
            .collect();
        genres.sort();
        genres.dedup();
        genres
    }

    pub fn get_distinct_bit_rates(&self) -> Vec<String> {
        let tracks = self.get_all_tracks();
        let mut rates: Vec<u32> = tracks.into_iter().map(|t| t.bit_rate).collect();
        rates.sort();
        rates.dedup();
        // 通常显示为 XXX kbps
        rates.into_iter().map(|r| format!("{} kbps", r / 1000)).collect()
    }

    pub fn get_tracks_by_category(&self, cat: &crate::types::Categories, val: &str) -> Vec<crate::types::TrackInfo> {
        let tracks = self.get_all_tracks();
        tracks.into_iter().filter(|t| {
            match cat {
                crate::types::Categories::AlbumArtist => t.artist == val,
                crate::types::Categories::Album => {
                    let album_val = if t.album.is_empty() { "Unknown Album" } else { &t.album };
                    album_val == val
                },
                crate::types::Categories::Genre => {
                    let genre_val = if t.genre.is_empty() { "Unknown Genre" } else { &t.genre };
                    genre_val == val
                },
                crate::types::Categories::SampleRate => format!("{} Hz", t.sample_rate) == val,
                crate::types::Categories::BitRate => format!("{} kbps", t.bit_rate / 1000) == val,
                crate::types::Categories::BitDepth => format!("{} Bit", t.bit_depth) == val,
            }
        }).collect()
    }
}
