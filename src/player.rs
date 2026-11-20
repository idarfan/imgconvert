//! 影片播放器的狀態：讀影片資訊、跳到某一格、播放／暫停（含聲音）、A/B 範圍、音量波形。
//! 畫面與聲音都由 ffmpeg 在背景解碼（見 ffmpeg.rs、audio.rs），這裡只負責收資料、更新貼圖。

use crate::audio::{self, AudioOut};
use crate::ffmpeg::{self, Frame, Pace};
use crate::video::{Selection, VideoInfo};
use crate::waveform::Bin;
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// 波形的狀態
pub enum Wave {
    /// 還沒讀到影片資訊，或影片沒有聲音
    NoAudio,
    Loading(Receiver<Result<Vec<Bin>, String>>),
    Ready(Vec<Bin>),
    Failed(String),
}

pub struct Player {
    pub path: PathBuf,
    pub info: Option<VideoInfo>,
    pub err: Option<String>,
    /// 聲音無法播放的原因（畫面照常播放）
    pub audio_note: Option<String>,
    pub tex: Option<egui::TextureHandle>,
    /// 目前播放位置（秒）
    pub pos: f64,
    pub sel: Selection,
    pub playing: bool,
    pub wave: Wave,
    /// 播放到這個時間就停（「播放範圍」用）
    stop_at: Option<f64>,
    probe_rx: Option<Receiver<Result<VideoInfo, String>>>,
    frames: Option<Receiver<Result<Frame, String>>>,
    /// 拖曳時間軸時，上一格還在解碼就先記下最新位置，解完再跳
    pending_seek: Option<f64>,
    audio: Option<AudioOut>,
    /// 音量 0.0–1.0（f32 的位元），與聲音執行緒共用，調整後立刻生效
    volume: Arc<AtomicU32>,
}

impl Player {
    pub fn open(path: PathBuf, ctx: &egui::Context, volume: f32) -> Self {
        let (tx, rx) = channel();
        let (p, c) = (path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(ffmpeg::probe(&p));
            c.request_repaint();
        });
        Self {
            path,
            info: None,
            err: None,
            audio_note: None,
            tex: None,
            pos: 0.0,
            sel: Selection::full(0.0),
            playing: false,
            wave: Wave::NoAudio,
            stop_at: None,
            probe_rx: Some(rx),
            frames: None,
            pending_seek: None,
            audio: None,
            volume: Arc::new(AtomicU32::new(volume.to_bits())),
        }
    }

    pub fn duration(&self) -> f64 {
        self.info.map_or(0.0, |i| i.duration)
    }

    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// 每幀呼叫：收影片資訊、影格與波形
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(Ok(r)) = self.probe_rx.as_ref().map(Receiver::try_recv) {
            self.probe_rx = None;
            match r {
                Ok(info) => {
                    self.info = Some(info);
                    self.sel = Selection::full(info.duration);
                    if info.has_audio {
                        let c = ctx.clone();
                        self.wave = Wave::Loading(ffmpeg::waveform(&self.path, move || c.request_repaint()));
                    }
                    self.seek(0.0, ctx);
                }
                Err(e) => self.err = Some(e),
            }
        }
        if let Wave::Loading(rx) = &self.wave {
            match rx.try_recv() {
                Ok(Ok(bins)) => self.wave = Wave::Ready(bins),
                Ok(Err(e)) => self.wave = Wave::Failed(e),
                Err(TryRecvError::Disconnected) => self.wave = Wave::Failed("無法分析音訊波形".into()),
                Err(TryRecvError::Empty) => {}
            }
        }
        self.poll_frames(ctx);
    }

    fn poll_frames(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.frames else { return };
        let mut latest = None;
        let ended = loop {
            match rx.try_recv() {
                Ok(Ok(f)) => latest = Some(f),
                Ok(Err(e)) => {
                    self.err = Some(e);
                    break true;
                }
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        if let Some(f) = latest {
            self.pos = f.t.min(self.duration());
            let img = egui::ColorImage::from_rgba_unmultiplied(f.size, &f.rgba);
            match &mut self.tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => self.tex = Some(ctx.load_texture("video_frame", img, egui::TextureOptions::LINEAR)),
            }
        }
        if self.playing && self.stop_at.is_some_and(|s| self.pos >= s) {
            self.pause();
            return;
        }
        if ended {
            self.frames = None;
            self.playing = false;
            self.audio = None;
            if let Some(t) = self.pending_seek.take() {
                self.seek(t, ctx);
            }
        }
    }

    /// 跳到某個時間並顯示那一格（會暫停播放）
    pub fn seek(&mut self, t: f64, ctx: &egui::Context) {
        let Some(info) = self.info else { return };
        let t = t.clamp(0.0, info.duration);
        self.pos = t;
        if self.playing {
            self.pause();
        }
        if self.frames.is_some() {
            self.pending_seek = Some(t); // 上一格還在解，解完再跳到最新位置
            return;
        }
        self.err = None;
        let c = ctx.clone();
        // 太接近結尾時往前一點，避免 ffmpeg 一格都解不到
        let start = t.min((info.duration - 0.05).max(0.0));
        self.frames = Some(ffmpeg::decode_frames(&self.path, &info, start, Pace::Still, move || c.request_repaint()));
    }

    /// 從目前位置播放（有聲音就一起播）；到結尾（或 stop_at）自動停止
    pub fn play(&mut self, ctx: &egui::Context, stop_at: Option<f64>) {
        let Some(info) = self.info else { return };
        let start = if self.pos >= info.duration - 0.05 { 0.0 } else { self.pos };
        self.pending_seek = None;
        self.err = None;
        self.stop_at = stop_at;
        self.playing = true;
        self.audio = None;
        let clock = if info.has_audio {
            match audio::start(&self.path, start, self.volume.clone()) {
                Ok(a) => {
                    self.audio_note = None;
                    let clock = a.clock();
                    self.audio = Some(a);
                    Some(clock)
                }
                Err(e) => {
                    self.audio_note = Some(e);
                    None
                }
            }
        } else {
            None
        };
        let c = ctx.clone();
        self.frames = Some(ffmpeg::decode_frames(&self.path, &info, start, Pace::Play(clock), move || c.request_repaint()));
    }

    /// 播放 A→B 範圍
    pub fn play_selection(&mut self, ctx: &egui::Context) {
        self.pos = self.sel.a;
        self.play(ctx, Some(self.sel.b));
    }

    pub fn pause(&mut self) {
        self.playing = false;
        self.stop_at = None;
        self.frames = None; // 丟掉 Receiver，背景 ffmpeg 會自己結束
        self.audio = None; // 丟掉聲音輸出，喇叭立刻停
    }

    pub fn toggle_play(&mut self, ctx: &egui::Context) {
        if self.playing { self.pause() } else { self.play(ctx, None) }
    }
}
