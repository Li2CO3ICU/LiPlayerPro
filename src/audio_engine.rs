#![allow(dead_code, unused_variables)]
use alsa::{Direction, ValueOr, device_name};
use crossbeam_channel::{unbounded, Receiver, Sender};
use ringbuf::HeapRb;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering}};
use std::thread;
use std::time::Duration;
use alsa::pcm::{PCM, Format, Access, HwParams};

pub enum AudioCmd {
    Play(String),
    Stop,
    Quit,
}

pub struct AudioEngine {
    cmd_sender: Sender<AudioCmd>,
    _engine_thread: Option<thread::JoinHandle<()>>,
    pub total_duration: Arc<AtomicU64>,
    pub current_pos: Arc<AtomicU64>,
    pub current_sample_rate: Arc<AtomicU32>,
    pub is_paused: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn new(device_name: &str) -> Self {
        let (tx, rx) = unbounded();
        let current_pos = Arc::new(AtomicU64::new(0));
        let current_sample_rate = Arc::new(AtomicU32::new(44100));
        let total_duration = Arc::new(AtomicU64::new(0));
        let is_paused = Arc::new(AtomicBool::new(false));

        let pos_clone = current_pos.clone();
        let rate_clone = current_sample_rate.clone();
        let pause_clone = is_paused.clone();

        let device_name_for_thread = device_name.to_string();
        let device_name_clone = device_name.to_string();

        let engine_thread = thread::Builder::new()
            .name("Engine_Orchestrator".into())
            .spawn(move || {
                Self::engine_loop(rx, pos_clone, rate_clone, pause_clone, device_name_clone)
            })
            .expect("无法启动音频调度引擎");

        Self {
            cmd_sender: tx,
            _engine_thread: Some(engine_thread),
            total_duration,
            current_pos,
            current_sample_rate,
            is_paused,
        }
    }

    pub fn play(&self, path: &str) {
        let _ = self.cmd_sender.send(AudioCmd::Play(path.to_string()));
    }
    pub fn stop(&self) {
        let _ = self.cmd_sender.send(AudioCmd::Stop);
    }
    pub fn quit(&self) {
        let _ = self.cmd_sender.send(AudioCmd::Quit);
    }

    pub fn get_elapsed_duration(&self) -> Duration {
        let frames = self.current_pos.load(Ordering::Acquire);
        let rate = self.current_sample_rate.load(Ordering::Acquire);
        
        if rate == 0 {
            return Duration::ZERO;
        }
        
        // 🌟 提高精度：先乘后除，防止因为整数除法丢失不足 1 秒的部分
        Duration::from_millis((frames * 1000) / rate as u64)
    }

    fn engine_loop(
        rx: Receiver<AudioCmd>,
        pos_tracker: Arc<AtomicU64>,
        rate_tracker: Arc<AtomicU32>,
        is_paused: Arc<AtomicBool>,
        device_name: String,
    ) {
        let mut play_flag = Arc::new(AtomicBool::new(false));
        let mut pipeline_threads: Vec<thread::JoinHandle<()>> = vec![];

        loop {
            match rx.recv() {
                Ok(AudioCmd::Play(path)) => {
                    play_flag.store(false, Ordering::Release);
                    for t in pipeline_threads.drain(..) {
                        let _ = t.join();
                    }

                    play_flag = Arc::new(AtomicBool::new(true));
                    let eof_flag = Arc::new(AtomicBool::new(false));

                    let source_rate = Self::probe_sample_rate(&path);
                    let actual_rate = Self::negotiate_alsa_rate(&device_name, source_rate);

                    let alsa_device = device_name.clone();

                    pos_tracker.store(0, Ordering::Release);
                    rate_tracker.store(actual_rate, Ordering::Release);
                    is_paused.store(false, Ordering::Release);

                    let pos_tracker_alsa = pos_tracker.clone();
                    let pause_alsa = is_paused.clone();
                    
                    let rb = HeapRb::<i32>::new(2_097_152);
                    let (mut prod, mut cons) = rb.split();

                    let flag_dec = play_flag.clone();
                    let flag_alsa = play_flag.clone();
                    let eof_dec = eof_flag.clone();
                    let eof_alsa = eof_flag.clone();
                    let path_clone = path.clone();

                    let dec_thread = thread::Builder::new()
                        .name("Decoder_Pipeline".into())
                        .spawn(move || {
                            let mut child = Command::new("ffmpeg")
                                .args([
                                    "-v",
                                    "error",
                                    "-i",
                                    &path_clone,
                                    "-threads",
                                    "0",
                                    "-acodec",
                                    "pcm_s32le",
                                    "-f",
                                    "s32le",
                                    "-ac",
                                    "2",
                                    "-ar",
                                    &actual_rate.to_string(),
                                    "-",
                                ])
                                .stdout(Stdio::piped())
                                .stderr(Stdio::null())
                                .spawn()
                                .expect("FFmpeg Error");

                            let mut stdout = child.stdout.take().unwrap();
                            let mut byte_buf = [0u8; 131072];

                            while flag_dec.load(Ordering::Acquire) {
                                match stdout.read(&mut byte_buf) {
                                    Ok(0) | Err(_) => {
                                        eof_dec.store(true, Ordering::Release);
                                        break;
                                    }
                                    Ok(n) => {
                                        let mut samples = Vec::with_capacity(n / 4);
                                        for chunk in byte_buf[..n].chunks_exact(4) {
                                            samples.push(i32::from_le_bytes([
                                                chunk[0], chunk[1], chunk[2], chunk[3],
                                            ]));
                                        }
                                        let mut written = 0;
                                        while written < samples.len()
                                            && flag_dec.load(Ordering::Acquire)
                                        {
                                            let pushed = prod.push_slice(&samples[written..]);
                                            written += pushed;
                                            if pushed == 0 {
                                                thread::sleep(Duration::from_millis(1));
                                            }
                                        }
                                    }
                                }
                            }
                            let _ = child.kill();
                        })
                        .unwrap();

                    let alsa_thread = thread::Builder::new()
                        .name("ALSA_Driver".into())
                        .spawn(move || {
                            while cons.len() < 80_000
                                && flag_alsa.load(Ordering::Acquire)
                                && !eof_alsa.load(Ordering::Acquire)
                            {
                                thread::sleep(Duration::from_millis(5));
                            }
                            if !flag_alsa.load(Ordering::Acquire) {
                                return;
                            }

                            if let Ok(pcm) = PCM::new(&alsa_device, Direction::Playback, false) {
                                use alsa::ValueOr;
                                let hwp = HwParams::any(&pcm).unwrap();
                                hwp.set_channels(2).unwrap();
                                hwp.set_rate(actual_rate, ValueOr::Nearest).unwrap();
                                hwp.set_format(Format::S32LE).unwrap();
                                hwp.set_access(Access::RWInterleaved).unwrap();
                                hwp.set_buffer_time_near(500000, ValueOr::Nearest).unwrap();
                                pcm.hw_params(&hwp).unwrap();

                                let io = pcm.io_i32().unwrap();
                                let mut local_buf = vec![0i32; 16384];

                                while flag_alsa.load(Ordering::Acquire) {

                                    // 🌟 核心休眠逻辑：优雅释放声卡并挂起
                                    if pause_alsa.load(Ordering::Acquire) {
                                        let _ = pcm.drop(); // 安全丢弃缓存防止爆音
                                        while pause_alsa.load(Ordering::Acquire) && flag_alsa.load(Ordering::Acquire) {
                                            thread::sleep(Duration::from_millis(50));
                                        }
                                        let _ = pcm.prepare(); // 取消暂停时重新激活声卡
                                        continue;
                                    }
                                    
                                    let avail = cons.len();
                                    if avail >= 2 {
                                        let read_len = std::cmp::min(avail, local_buf.len());
                                        let read_len = read_len - (read_len % 2);
                                        let popped = cons.pop_slice(&mut local_buf[..read_len]);
                                        if popped > 0 {
                                            if let Err(_) = io.writei(&local_buf[..popped]) {
                                                let _ = pcm.prepare();
                                            } else {
                                                pos_tracker_alsa.fetch_add(
                                                    (popped / 2) as u64,
                                                    Ordering::Relaxed,
                                                );
                                            }
                                        }
                                    } else if eof_alsa.load(Ordering::Acquire) {
                                        break;
                                    } else {
                                        thread::sleep(Duration::from_micros(200));
                                    }
                                }
                                let _ = pcm.drop();
                            }
                        })
                        .unwrap();

                    pipeline_threads.push(dec_thread);
                    pipeline_threads.push(alsa_thread);
                }
                Ok(AudioCmd::Stop) => {
                    play_flag.store(false, Ordering::Release);
                    for t in pipeline_threads.drain(..) {
                        let _ = t.join();
                    }
                }
                Ok(AudioCmd::Quit) => {
                    play_flag.store(false, Ordering::Release);
                    for t in pipeline_threads.drain(..) {
                        let _ = t.join();
                    }
                    break;
                }
                Err(_) => break,
            }
        }
    }

    fn probe_sample_rate(path: &str) -> u32 {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=sample_rate",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                path,
            ])
            .output();
        if let Ok(out) = output {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .unwrap_or(44100)
        } else {
            44100
        }
    }

    fn negotiate_alsa_rate(device_name: &str, target: u32) -> u32 {
        if let Ok(pcm) = PCM::new(device_name, Direction::Playback, false) {
            if let Ok(hwp) = HwParams::any(&pcm) {
                if let Ok(actual) = hwp.set_rate_near(target, ValueOr::Nearest) {
                    return actual;
                }
            }
        }
        44100
    }
}
