#![allow(unused_imports, dead_code)]
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
    f_album: tantivy::schema::Field,
    f_path: tantivy::schema::Field,
    f_duration: tantivy::schema::Field,    // 新增时长
    f_sample_rate: tantivy::schema::Field, // 新增采样率
    f_bit_rate: tantivy::schema::Field,
    f_bit_depth:tantivy::schema::Field,
    pub tracks: Arc<RwLock<Vec<crate::TrackInfo>>>,
}

impl MusicLibrary {
    pub fn new(config: &crate::LibraryConfig) -> Self {

        //get home dir
        let index_path = Path::new(&config.index_path);

        std::fs::create_dir_all(&index_path).expect("创建目录失败喵...");

        let mut schema_builder = Schema::builder();
        
        // --- 1. 定义 Schema ---
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
        let f_path = schema_builder.add_text_field("path", STRING | STORED);
        let f_duration = schema_builder.add_u64_field("duration", STORED);
        let f_sample_rate = schema_builder.add_u64_field("sample_rate", STORED);
        let f_bit_rate = schema_builder.add_u64_field("bit_rate", STORED);
        let f_bit_depth = schema_builder.add_u64_field("bit_depth", STORED);
        let schema = schema_builder.build();
        
        let directory = MmapDirectory::open(&index_path).expect("打不开索引目录喵");
        // 如果目录里已经有索引，open_or_create 会自动加载它
        let index = Index::open_or_create(directory, schema).expect("索引初始化失败喵");

        // --- 3. 立即注册分析器 (顺序非常重要喵！) ---
        // 加上 LowerCaser 过滤器后，所有搜索和索引都会转为小写处理
        let ngram_analyzer = TextAnalyzer::builder(NgramTokenizer::new(1, 2, false).unwrap())
            .filter(LowerCaser)
            .build();
        index.tokenizers().register("cjk_ngram", ngram_analyzer);

        // --- 4. 最后创建 Reader ---
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
            f_path,
            f_duration,
            f_sample_rate,
            f_bit_rate,
            f_bit_depth,
            tracks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn build_index(&self, music_dir: &str, tx: std::sync::mpsc::Sender<AppEvent>) {
    // 1. 获取所有待扫描路径 (这一步很快，主要是磁盘遍历)
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

        // 2. 创建 IndexWriter (200MB 缓冲区足够了)
        let mut index_writer = self.index.writer(200_000_000).expect("Writer failed");
        
        let counter = AtomicUsize::new(0);
        let tx_ref = &tx;
        let counter_ref = &counter;

        // 3. 并行扫描并即时处理
        // 注意：我们将解析和入库逻辑结合，减少中间 Vec 的内存占用
        let _ : Vec<()> = paths
            .into_par_iter()
            .filter_map(|p| {
                // 解析元数据
                let meta = crate::scanner::scan_track(&p)?;
                
                // --- 核心改动：去重逻辑 ---
                // 每一个 path 作为一个唯一的 Term
                let term = Term::from_field_text(self.f_path, &meta.path);
                // 虽然是多线程，但 IndexWriter 内部处理了同步
                index_writer.delete_term(term); 

                let mut doc = TantivyDocument::default();
                doc.add_text(self.f_title, &meta.title);
                doc.add_text(self.f_artist, &meta.artist);
                doc.add_text(self.f_album, &meta.album);
                doc.add_text(self.f_path, &meta.path);
                doc.add_u64(self.f_duration, meta.duration as u64);
                doc.add_u64(self.f_sample_rate, meta.sample_rate as u64);
                doc.add_u64(self.f_bit_rate, meta.bit_rate as u64);
                doc.add_u64(self.f_bit_depth, meta.bit_depth as u64);
                
                index_writer.add_document(doc).ok()?;

                // 进度报告
                let current = counter_ref.fetch_add(1, Ordering::SeqCst) + 1;
                if current % 5 == 0 || current == total {
                    let _ = tx_ref.send(AppEvent::ScanProgress { current, total });
                }
                Some(())
            })
            .collect();

        // 4. 提交持久化
        // commit 会把内存中的数据刷到 ~/.local/share/LiPlayerPro 的磁盘文件里
        index_writer.commit().expect("Commit failed");
        
        // 重新加载 reader 以便能搜到新歌
        self.reader.reload().expect("Reload failed");
    
        let _ = tx.send(AppEvent::ScanFinished);
    }
    // 适配前端：获取所有歌曲
    pub fn get_all_tracks(&self) -> Vec<crate::TrackInfo> {
        let searcher = self.reader.searcher();
        let top_docs = searcher
            .search(&AllQuery, &TopDocs::with_limit(10000))
            .unwrap();

        let mut results = Vec::new();
        for (_, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address).unwrap();
            results.push(crate::TrackInfo {
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

    // 适配前端：获取歌曲总数
    pub fn get_track_count(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }
}
