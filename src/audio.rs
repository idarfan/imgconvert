//! 影片預覽的聲音：ffmpeg 把音軌解成 f32 PCM，經由緩衝區交給喇叭（Windows 用 cpal／WASAPI）。
//! 聲音同時當作播放時鐘：畫面依「喇叭實際播到第幾秒」送出影格，聲音和畫面才會對得上。
//! Linux 開發版不出聲（沒有連結音效函式庫），播放器會改用系統時間當時鐘。
// Linux 版沒有聲音輸出，這裡大部分只有 Windows 版會用到（測試仍在 Linux 上跑）
#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 播放時鐘：回傳從開始播放算起經過幾秒；還沒開始出聲時回傳 None（畫面先等）
pub type Clock = Arc<dyn Fn() -> Option<f64> + Send + Sync>;

/// 緩衝區最多先解好幾秒的聲音
const BUFFER_SECS: f64 = 0.5;

/// 解碼執行緒與喇叭回呼之間共用的狀態
pub struct Shared {
    buf: Mutex<VecDeque<f32>>,
    rate: u32,
    channels: usize,
    /// 喇叭已經拿走的樣本框數（一框 = 每個聲道各一個樣本）
    played: AtomicU64,
    /// 音量 0.0–1.0（f32 的位元）；滑桿調整時即時生效
    volume: Arc<AtomicU32>,
    /// ffmpeg 已經沒有更多聲音
    feeder_done: AtomicBool,
    /// 使用者暫停或換檔
    stop: AtomicBool,
    /// 聲音放完的時間點；之後改用系統時間繼續往前走，畫面才不會停住
    exhausted: OnceLock<Instant>,
}

impl Shared {
    pub fn new(rate: u32, channels: usize, volume: Arc<AtomicU32>) -> Self {
        Self {
            buf: Mutex::new(VecDeque::new()),
            rate: rate.max(1),
            channels: channels.max(1),
            played: AtomicU64::new(0),
            volume,
            feeder_done: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            exhausted: OnceLock::new(),
        }
    }

    fn capacity(&self) -> usize {
        (self.rate as f64 * BUFFER_SECS) as usize * self.channels
    }

    pub fn push(&self, samples: &[f32]) {
        self.buf.lock().unwrap().extend(samples);
    }

    /// 喇叭要聲音時呼叫：從緩衝區拿樣本乘上音量；不夠就補靜音（不算進已播放）
    pub fn fill(&self, out: &mut [f32]) {
        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let mut buf = self.buf.lock().unwrap();
        // 只拿完整的框，聲道才不會錯位
        let take = (buf.len().min(out.len()) / self.channels) * self.channels;
        for (o, s) in out.iter_mut().zip(buf.drain(..take)) {
            *o = s * vol;
        }
        out[take..].fill(0.0);
        self.played.fetch_add((take / self.channels) as u64, Ordering::Relaxed);
    }

    /// 播放時鐘（見 `Clock`）
    pub fn elapsed(&self) -> Option<f64> {
        if self.stop.load(Ordering::Relaxed) {
            return Some(f64::INFINITY); // 讓等待中的畫面執行緒立刻往下走、發現沒人要而結束
        }
        let secs = self.played.load(Ordering::Relaxed) as f64 / self.rate as f64;
        let drained = self.feeder_done.load(Ordering::Relaxed) && self.buf.lock().unwrap().is_empty();
        if drained {
            return Some(secs + self.exhausted.get_or_init(Instant::now).elapsed().as_secs_f64());
        }
        (secs > 0.0).then_some(secs)
    }
}

/// 正在播放的聲音；丟掉（drop）就停止
pub struct AudioOut {
    shared: Arc<Shared>,
    #[cfg(windows)]
    _stream: cpal::Stream,
}

impl AudioOut {
    pub fn clock(&self) -> Clock {
        let s = self.shared.clone();
        Arc::new(move || s.elapsed())
    }
}

impl Drop for AudioOut {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// 從 `start` 秒開始播放 `src` 的聲音
pub fn start(src: &Path, start: f64, volume: Arc<AtomicU32>) -> Result<AudioOut, String> {
    platform::start(src, start, volume)
}

/// 背景執行緒：讀 ffmpeg 的 f32le 輸出放進緩衝區；緩衝區滿了就等，停止時結束 ffmpeg
fn feed(mut child: std::process::Child, shared: Arc<Shared>) {
    let Some(mut out) = child.stdout.take() else { return };
    let frame_bytes = 4 * shared.channels;
    let mut bytes = vec![0u8; frame_bytes * 1024];
    let mut samples = Vec::with_capacity(bytes.len() / 4);
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.buf.lock().unwrap().len() >= shared.capacity() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let n = read_frames(&mut out, &mut bytes, frame_bytes);
        if n == 0 {
            break;
        }
        samples.clear();
        samples.extend(bytes[..n].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)));
        shared.push(&samples);
    }
    shared.feeder_done.store(true, Ordering::Relaxed);
    let _ = child.kill();
    let _ = child.wait();
}

/// 盡量讀滿 `buf`，回傳讀到的位元組數（去掉最後不完整的框）；0 = 結束
fn read_frames(r: &mut impl Read, buf: &mut [u8], frame_bytes: usize) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    n - n % frame_bytes
}

#[cfg(windows)]
mod platform {
    use super::{AudioOut, Shared, feed};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    pub fn start(src: &Path, start: f64, volume: Arc<AtomicU32>) -> Result<AudioOut, String> {
        let device = cpal::default_host().default_output_device().ok_or("找不到播放聲音的裝置")?;
        let supported = device.default_output_config().map_err(|e| format!("無法取得音效設定：{e}"))?;
        let config: StreamConfig = supported.config();
        let shared = Arc::new(Shared::new(config.sample_rate, config.channels as usize, volume));
        let args = crate::video::audio_args(src, start, config.sample_rate, config.channels as u32);
        let child = crate::ffmpeg::spawn_piped(&args)?;
        let feeder = shared.clone();
        std::thread::spawn(move || feed(child, feeder));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, &shared),
            SampleFormat::I16 => build::<i16>(&device, config, &shared),
            SampleFormat::I32 => build::<i32>(&device, config, &shared),
            SampleFormat::U16 => build::<u16>(&device, config, &shared),
            SampleFormat::F64 => build::<f64>(&device, config, &shared),
            other => Err(format!("不支援的音效格式 {other}")),
        };
        let stream = stream.inspect_err(|_| shared.stop.store(true, std::sync::atomic::Ordering::Relaxed))?;
        stream.play().map_err(|e| format!("無法開始播放聲音：{e}"))?;
        Ok(AudioOut { shared, _stream: stream })
    }

    fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: StreamConfig, shared: &Arc<Shared>) -> Result<cpal::Stream, String> {
        let s = shared.clone();
        let mut scratch: Vec<f32> = Vec::new();
        let data = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            scratch.resize(out.len(), 0.0);
            s.fill(&mut scratch);
            for (o, v) in out.iter_mut().zip(&scratch) {
                *o = T::from_sample(*v);
            }
        };
        let err = |e: cpal::Error| eprintln!("聲音播放錯誤：{e}");
        device.build_output_stream(config, data, err, None).map_err(|e| format!("無法開啟聲音輸出：{e}"))
    }
}

#[cfg(not(windows))]
mod platform {
    use super::AudioOut;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    pub fn start(_src: &Path, _start: f64, _volume: Arc<AtomicU32>) -> Result<AudioOut, String> {
        Err("Linux 開發版不播放聲音".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(vol: f32) -> Shared {
        Shared::new(4, 2, Arc::new(AtomicU32::new(vol.to_bits())))
    }

    #[test]
    fn fill_applies_volume_and_pads_silence() {
        let s = shared(0.5);
        s.push(&[1.0, -1.0, 0.5, 0.5, 0.2]); // 最後一個不是完整的框
        let mut out = [9.0f32; 6];
        s.fill(&mut out);
        assert_eq!(out, [0.5, -0.5, 0.25, 0.25, 0.0, 0.0]);
        assert_eq!(s.played.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn clock_waits_then_follows_played_frames() {
        let s = shared(1.0);
        assert_eq!(s.elapsed(), None, "還沒出聲：畫面要等");
        s.push(&[0.0; 4 * 2]); // 4 框 = 取樣率 4 下的 1 秒
        let mut out = [0.0f32; 4];
        s.fill(&mut out); // 播了 2 框 = 0.5 秒
        assert_eq!(s.elapsed(), Some(0.5));
    }

    #[test]
    fn clock_keeps_running_after_audio_ends_and_releases_on_stop() {
        let s = shared(1.0);
        s.push(&[0.0; 2]);
        s.fill(&mut [0.0f32; 2]);
        s.feeder_done.store(true, Ordering::Relaxed);
        let t = s.elapsed().unwrap();
        assert!((0.25..0.3).contains(&t), "聲音放完後從 0.25 秒繼續以系統時間前進：{t}");
        s.stop.store(true, Ordering::Relaxed);
        assert_eq!(s.elapsed(), Some(f64::INFINITY));
    }

    #[test]
    fn read_frames_drops_partial_frame() {
        let data = [1u8; 19];
        let mut buf = [0u8; 64];
        assert_eq!(read_frames(&mut &data[..], &mut buf, 8), 16);
        assert_eq!(read_frames(&mut &[][..], &mut buf, 8), 0);
    }
}
