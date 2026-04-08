#![allow(dead_code)]
use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct TrackMeta {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub sample_rate: u32,
    pub format: String,
    pub duration: u64,
    pub bit_rate: u32,
    pub bit_depth: u32,
}

pub fn scan_track(path: &str) -> Option<TrackMeta> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&json_str).ok()?;

    let audio_stream = json["streams"]
        .as_array()?
        .iter()
        .find(|s| s["codec_type"] == "audio")?;

    let sample_rate: u32 = audio_stream["sample_rate"]
        .as_u64()
        .map(|v| v as u32)
        .or_else(|| {
            audio_stream["sample_rate"].as_str().and_then(|s| s.parse().ok())
        })
        .unwrap_or(44100);

    let format = audio_stream["codec_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    let tags = json["format"]["tags"].as_object();
    let get_tag = |key: &str| -> Option<String> {
        tags.and_then(|t| {
            t.get(key)
                .or_else(|| t.get(&key.to_uppercase()))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
    };

    let file_stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;

    let bit_rate = json["format"]["bit_rate"]
        .as_str()
        .and_then(|s| s.parse::<u32>().ok())
        .or_else(|| {
            audio_stream["bit_rate"]
                .as_str()
                .and_then(|s| s.parse::<u32>().ok())
        })
        .unwrap_or(0);

    let parse_u32 = |v: &Value| -> Option<u32> {
        v.as_u64().map(|n| n as u32).or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    };

    // 先找 bits_per_raw_sample (防 FLAC/ALAC 陷阱)，再找 bits_per_sample，过滤掉为 0 的无效值
    let bit_depth = parse_u32(&audio_stream["bits_per_raw_sample"])
        .or_else(|| parse_u32(&audio_stream["bits_per_sample"]))
        .filter(|&v| v > 0)
        .unwrap_or(0);

    
    let mut final_sample_rate = sample_rate;
    let mut final_bit_depth = bit_depth;

    if format.starts_with("dsd_") {
        if bit_depth == 8 {
            final_sample_rate = sample_rate * 8; // 例如 352800 * 8 = 2822400
            final_bit_depth = 1;                 // 永远纯粹的 1-bit
        }
    }

    Some(TrackMeta {
        path: path.to_string(),
        title: get_tag("title").unwrap_or(file_stem),
        artist: get_tag("artist").unwrap_or_else(|| "Unknown".to_string()),
        album: get_tag("album").unwrap_or_else(|| "Unknown".to_string()),
        sample_rate: final_sample_rate,
        format,
        duration, 
        bit_rate,  
        bit_depth: final_bit_depth,
    })
}
