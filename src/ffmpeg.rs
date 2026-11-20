//! 呼叫隨附的 ffmpeg／ffprobe（另開程序，不連結進 ImgConvert.exe）：
//! 讀影片資訊、背景解碼預覽影格、背景轉檔並回報進度。

use crate::audio::Clock;
use crate::video::{self, Mode, Selection, VideoInfo};
use crate::waveform;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 安裝目錄下放 ffmpeg 的子資料夾
const BUNDLE_DIR: &str = "ffmpeg";

/// 找 ffmpeg／ffprobe：優先用 exe 旁的 `ffmpeg\` 子資料夾，找不到才用系統 PATH
pub fn tool(name: &str) -> PathBuf {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(BUNDLE_DIR).join(&file)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(file))
}

fn command(name: &str, args: &[OsString]) -> Command {
    let mut cmd = Command::new(tool(name));
    cmd.args(args).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000; // 不要跳出黑色命令列視窗
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn spawn_error(name: &str, e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        format!("找不到 {name}，請重新安裝 ImgConvert（ffmpeg 應在安裝目錄的 {BUNDLE_DIR} 資料夾）")
    } else {
        format!("無法啟動 {name}：{e}")
    }
}

/// 讀影片資訊（寬高、長度、影格率）
pub fn probe(src: &Path) -> Result<VideoInfo, String> {
    let out = command("ffprobe", &video::probe_args(src)).output().map_err(|e| spawn_error("ffprobe", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("無法讀取影片：{}", err.lines().last().unwrap_or("未知錯誤").trim()));
    }
    let info = video::parse_probe(&String::from_utf8_lossy(&out.stdout))?;
    // 查音軌失敗不影響看畫面，當作沒有聲音
    let audio = command("ffprobe", &video::probe_audio_args(src)).output();
    let has_audio = audio.is_ok_and(|o| video::has_audio(&String::from_utf8_lossy(&o.stdout)));
    Ok(VideoInfo { has_audio, ..info })
}

/// 開 ffmpeg，stdout 接管線、stderr 丟掉（給聲音播放用）
#[cfg_attr(not(windows), allow(dead_code))]
pub fn spawn_piped(args: &[OsString]) -> Result<Child, String> {
    command("ffmpeg", args).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| spawn_error("ffmpeg", e))
}

/// 背景計算整條音軌的音量波形
pub fn waveform(src: &Path, repaint: impl Fn() + Send + 'static) -> Receiver<Result<Vec<waveform::Bin>, String>> {
    let (tx, rx) = sync_channel(1);
    let args = video::waveform_args(src);
    std::thread::spawn(move || {
        let _ = tx.send(read_waveform(&args));
        repaint();
    });
    rx
}

fn read_waveform(args: &[OsString]) -> Result<Vec<waveform::Bin>, String> {
    let mut child = command("ffmpeg", args).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| spawn_error("ffmpeg", e))?;
    let mut out = child.stdout.take().expect("stdout 已設為 piped");
    let mut b = waveform::Builder::default();
    let mut buf = vec![0u8; 64 * 1024];
    let mut carry: Option<u8> = None; // 讀到奇數個位元組時，留一個給下一次湊成樣本
    loop {
        let n = match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let mut chunk = &buf[..n];
        if let Some(lo) = carry.take() {
            b.push(i16::from_le_bytes([lo, chunk[0]]));
            chunk = &chunk[1..];
        }
        let even = chunk.len() & !1;
        b.push_bytes(&chunk[..even]);
        carry = chunk.get(even).copied();
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    let bins = b.finish();
    if !ok && bins.is_empty() {
        return Err("無法分析音訊波形".into());
    }
    Ok(bins)
}

/// 一張預覽影格
pub struct Frame {
    pub t: f64,
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// 預覽要「一格」還是「連續播放」
pub enum Pace {
    /// 只取 `start` 那一格
    Still,
    /// 連續播放，依時鐘送出影格；有聲音時用聲音當時鐘，沒有就用系統時間（None）
    Play(Option<Clock>),
}

/// 背景解碼預覽影格。
/// 呼叫端丟掉 Receiver 時，背景執行緒送不出去就會結束 ffmpeg，不需要另外取消。
pub fn decode_frames(src: &Path, info: &VideoInfo, start: f64, pace: Pace, repaint: impl Fn() + Send + 'static) -> Receiver<Result<Frame, String>> {
    let (tx, rx) = sync_channel(2);
    let size = video::preview_size(info.width, info.height);
    let still = matches!(pace, Pace::Still);
    let args = video::preview_args(src, start, size, still.then_some(1));
    std::thread::spawn(move || {
        let res = command("ffmpeg", &args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        match res {
            Ok(child) => stream_frames(child, size, start, pace, &tx, &repaint),
            Err(e) => _ = tx.send(Err(spawn_error("ffmpeg", e))),
        }
        repaint();
    });
    rx
}

/// 等到時鐘走到 `offset` 秒才回來
fn wait_until(offset: f64, clock: &Option<Clock>, began: Instant) {
    loop {
        let now = match clock {
            Some(c) => c(),
            None => Some(began.elapsed().as_secs_f64()),
        };
        match now {
            Some(t) if t >= offset => return,
            Some(t) if clock.is_none() => std::thread::sleep(Duration::from_secs_f64(offset - t)),
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

fn stream_frames(mut child: Child, size: (u32, u32), start: f64, pace: Pace, tx: &SyncSender<Result<Frame, String>>, repaint: &dyn Fn()) {
    let len = size.0 as usize * size.1 as usize * 4;
    let mut stdout = child.stdout.take().expect("stdout 已設為 piped");
    let began = Instant::now();
    let mut n = 0u32;
    loop {
        let mut buf = vec![0u8; len];
        if stdout.read_exact(&mut buf).is_err() {
            break;
        }
        let offset = n as f64 / video::PREVIEW_FPS;
        if let Pace::Play(clock) = &pace {
            wait_until(offset, clock, began); // 依時鐘送出，播放速度才正確、畫面才跟得上聲音
        }
        let frame = Frame { t: start + offset, size: [size.0 as usize, size.1 as usize], rgba: buf };
        if tx.send(Ok(frame)).is_err() {
            break; // 介面不要了（暫停、跳轉或換檔）
        }
        repaint();
        n += 1;
    }
    let _ = child.kill();
    let _ = child.wait();
    if n == 0 {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        let msg = err.lines().last().map(str::trim).filter(|l| !l.is_empty()).unwrap_or("沒有畫面");
        let _ = tx.send(Err(format!("無法解碼預覽：{msg}")));
    }
}

/// 背景轉檔工作的狀態
pub struct EncodeJob {
    /// 已編碼的秒數（f64 的位元，方便跨執行緒）
    done_secs: Arc<AtomicU64>,
    pub total_secs: f64,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<Encoded, String>>>>,
    pub started: Instant,
}

/// 轉檔成功的結果
pub struct Encoded {
    pub path: PathBuf,
    /// 輸出大小與預期不符時的提醒（可能是 AMD 編碼器自行補邊）
    pub warning: Option<String>,
}

/// 檢查輸出檔的實際畫面大小；ffprobe 讀的是 AV1 位元流本身，所以能抓到檔頭與畫面不一致
fn check_size(path: &Path, expected: (u32, u32)) -> Option<String> {
    let actual = probe(path).ok().map(|i| (i.width, i.height))?;
    (actual != expected).then(|| {
        format!("注意：實際畫面是 {}×{}，與預期 {}×{} 不同，部分看圖程式可能打不開", actual.0, actual.1, expected.0, expected.1)
    })
}

impl EncodeJob {
    pub fn progress(&self) -> f32 {
        let done = f64::from_bits(self.done_secs.load(Ordering::Relaxed));
        (done / self.total_secs.max(1e-6)).clamp(0.0, 1.0) as f32
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// 完成時回傳結果（只回傳一次）
    pub fn take_result(&self) -> Option<Result<Encoded, String>> {
        self.result.lock().unwrap().take()
    }
}

/// 開始轉檔：先寫到 `.avif.part`，成功才改名成 `dst`，取消或失敗會刪掉暫存檔
#[allow(clippy::too_many_arguments)]
pub fn encode(src: &Path, dst: PathBuf, sel: Selection, mode: Mode, fps: u32, info: &VideoInfo, repaint: impl Fn() + Send + 'static) -> EncodeJob {
    let job = EncodeJob {
        done_secs: Arc::new(AtomicU64::new(0f64.to_bits())),
        total_secs: sel.span(),
        child: Default::default(),
        cancelled: Default::default(),
        result: Default::default(),
        started: Instant::now(),
    };
    let tmp = dst.with_extension("avif.part");
    let args = video::encode_args(src, &tmp, sel, mode, fps, info);
    let expected = mode.output_size(info);
    let (done, child_slot, cancelled, result) = (job.done_secs.clone(), job.child.clone(), job.cancelled.clone(), job.result.clone());
    std::thread::spawn(move || {
        let r = run_encode(&args, &tmp, &dst, &done, &child_slot, &repaint);
        let r = if cancelled.load(Ordering::Relaxed) { Err("已取消".into()) } else { r };
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        let r = r.map(|path| Encoded { warning: check_size(&path, expected), path });
        *result.lock().unwrap() = Some(r);
        repaint();
    });
    job
}

fn run_encode(args: &[OsString], tmp: &Path, dst: &Path, done: &AtomicU64, slot: &Mutex<Option<Child>>, repaint: &dyn Fn()) -> Result<PathBuf, String> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("無法建立資料夾：{e}"))?;
    }
    let mut child = command("ffmpeg", args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| spawn_error("ffmpeg", e))?;
    let stdout = child.stdout.take().expect("stdout 已設為 piped");
    let mut stderr = child.stderr.take().expect("stderr 已設為 piped");
    *slot.lock().unwrap() = Some(child);
    // stderr 另開執行緒讀，避免管線塞滿卡住 ffmpeg
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(t) = video::parse_progress(&line) {
            done.store(t.to_bits(), Ordering::Relaxed);
            repaint();
        }
    }
    let child = slot.lock().unwrap().take(); // 先放開鎖再等待，取消按鈕才不會卡住
    let status = child.map(|mut c| c.wait());
    let err_text = err_reader.join().unwrap_or_default();
    match status {
        Some(Ok(s)) if s.success() => {
            if dst.exists() {
                let _ = std::fs::remove_file(dst);
            }
            std::fs::rename(tmp, dst).map_err(|e| format!("改名失敗：{e}"))?;
            Ok(dst.to_path_buf())
        }
        Some(Err(e)) => Err(format!("ffmpeg 執行失敗：{e}")),
        _ => Err(video::friendly_error(&err_text)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;

    /// 用 ffmpeg 產生 2 秒 320×240 測試影片；系統沒有 ffmpeg 時回傳 None（略過測試）
    fn sample_clip(dir: &Path) -> Option<PathBuf> {
        let p = dir.join("測試 影片.mp4");
        let src = "testsrc=size=320x240:rate=25:duration=2";
        let mut args: Vec<OsString> =
            ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", src, "-pix_fmt", "yuv420p"].map(OsString::from).to_vec();
        args.push(p.clone().into_os_string());
        command("ffmpeg", &args).status().ok()?.success().then_some(p)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("imgconvert-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn probe_and_decode_one_frame() {
        let dir = temp_dir("probe");
        let Some(clip) = sample_clip(&dir) else { return eprintln!("沒有 ffmpeg，略過") };
        let info = probe(&clip).unwrap();
        assert_eq!((info.width, info.height), (320, 240));
        assert!((info.duration - 2.0).abs() < 0.1 && (info.fps - 25.0).abs() < 0.01);
        assert!(!info.has_audio, "testsrc 沒有音軌");
        let rx = decode_frames(&clip, &info, 1.0, Pace::Still, || {});
        let f = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(f.size, [320, 240]);
        assert_eq!(f.rgba.len(), 320 * 240 * 4);
        assert!((f.t - 1.0).abs() < 1e-9);
        // 只要一格：之後通道關閉
        assert!(matches!(rx.recv_timeout(Duration::from_secs(10)), Err(RecvTimeoutError::Disconnected)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 2 秒的影片，前 1 秒有 440 Hz 聲音、後 1 秒靜音
    fn clip_with_audio(dir: &Path) -> Option<PathBuf> {
        let p = dir.join("有聲 影片.mp4");
        let args: Vec<OsString> = [
            "-hide_banner", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i", "testsrc=size=160x120:rate=25:duration=2",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1,apad=whole_dur=2",
            "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest",
        ]
        .map(OsString::from)
        .into_iter()
        .chain([p.clone().into_os_string()])
        .collect();
        command("ffmpeg", &args).status().ok()?.success().then_some(p)
    }

    #[test]
    fn audio_probe_and_waveform() {
        let dir = temp_dir("wave");
        let Some(clip) = clip_with_audio(&dir) else { return eprintln!("沒有 ffmpeg，略過") };
        let info = probe(&clip).unwrap();
        assert!(info.has_audio);
        let bins = waveform(&clip, || {}).recv_timeout(Duration::from_secs(20)).unwrap().unwrap();
        // 每秒 100 格；容許 AAC 編碼頭尾多出幾格
        assert!((195..=210).contains(&bins.len()), "格數 {}", bins.len());
        let loud = waveform::column(&bins, 0.2, 0.8);
        let quiet = waveform::column(&bins, 1.3, 1.9);
        // ffmpeg 的 sine 來源預設振幅是 1/8
        assert!((0.1..0.16).contains(&loud.peak) && quiet.peak < 0.01, "有聲 {loud:?}／靜音 {quiet:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn playback_follows_clock() {
        let dir = temp_dir("clock");
        let Some(clip) = sample_clip(&dir) else { return eprintln!("沒有 ffmpeg，略過") };
        let info = probe(&clip).unwrap();
        // 假時鐘：一開始沒聲音（None），之後直接跳到 0.5 秒
        let gate = Arc::new(AtomicBool::new(false));
        let g = gate.clone();
        let clock: Clock = Arc::new(move || g.load(Ordering::Relaxed).then_some(0.5));
        let rx = decode_frames(&clip, &info, 0.0, Pace::Play(Some(clock)), || {});
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "時鐘還沒開始，不能送畫面");
        gate.store(true, Ordering::Relaxed);
        // 0.5 秒以內的 16 格（0, 1/30, … 15/30）立刻送出，第 17 格要等時鐘
        let got: Vec<f64> = (0..16).map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap().t).collect();
        assert!((got[15] - 0.5).abs() < 1e-9);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "超過時鐘的影格要等");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn encode_succeeds_or_cleans_part_file() {
        let dir = temp_dir("enc");
        let Some(clip) = sample_clip(&dir) else { return eprintln!("沒有 ffmpeg，略過") };
        let info = probe(&clip).unwrap();
        let dst = dir.join("out.avif");
        let job = encode(&clip, dst.clone(), Selection { a: 0.5, b: 1.5 }, Mode::Original, 15, &info, || {});
        let start = Instant::now();
        let result = loop {
            if let Some(r) = job.take_result() {
                break r;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "轉檔逾時");
            std::thread::sleep(Duration::from_millis(50));
        };
        // 有 AMD 硬體編碼時會成功；沒有時要回報錯誤，而且不留暫存檔
        match result {
            Ok(e) => assert!(e.path.is_file() && e.path == dst && e.warning.is_none()),
            Err(e) => {
                eprintln!("轉檔失敗（預期在沒有 AMD 編碼器的環境）：{e}");
                assert!(!dst.exists() && !dst.with_extension("avif.part").exists());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
