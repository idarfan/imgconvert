//! MP4 → AVIF 的純邏輯（不碰 GUI、不開程序），方便單元測試：
//! ffprobe 輸出解析、A/B 範圍、ffmpeg 參數、進度解析、錯誤訊息轉換。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// 可讀入的影片副檔名
pub const VIDEO_EXTS: [&str; 3] = ["mp4", "m4v", "mov"];
/// 右側預覽畫面的高度上限（像素）；只影響預覽，不影響輸出
pub const PREVIEW_MAX_H: u32 = 540;
/// 預覽播放的影格率
pub const PREVIEW_FPS: f64 = 30.0;
/// AMD AV1 硬體編碼品質（qvbr 1–51，越大畫質越好、檔案越大）
const QVBR_LEVEL: &str = "24";
/// A、B 點最小間距（秒）
pub const MIN_SPAN: f64 = 0.1;

pub fn is_video_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    /// 已套用旋轉資訊後的寬高（手機直拍影片會對調）
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub fps: f64,
    /// 有沒有音軌（另外用 ffprobe 查，見 `has_audio`）
    pub has_audio: bool,
}

/// 解析 `ffprobe -of default=nw=1` 的 key=value 輸出；同一個 key 取第一次出現的值
pub fn parse_probe(text: &str) -> Result<VideoInfo, String> {
    let get = |key: &str| {
        text.lines()
            .filter_map(|l| l.trim().split_once('='))
            .find(|(k, v)| *k == key && *v != "N/A")
            .map(|(_, v)| v.trim())
    };
    let num = |key: &str| get(key).and_then(|v| v.parse::<f64>().ok());
    let (Some(w), Some(h)) = (num("width"), num("height")) else {
        return Err("找不到影像軌，這可能不是影片檔".into());
    };
    let duration = num("duration").filter(|d| *d > 0.0).ok_or("無法取得影片長度")?;
    let fps = get("avg_frame_rate").and_then(parse_ratio).filter(|f| *f > 0.0).unwrap_or(30.0);
    let rotated = num("rotation").is_some_and(|r| (r.abs() as i64) % 180 == 90);
    let (width, height) = if rotated { (h as u32, w as u32) } else { (w as u32, h as u32) };
    Ok(VideoInfo { width, height, duration, fps, has_audio: false })
}

/// 查音軌的 ffprobe 參數；輸出有 `codec_type=audio` 就表示有聲音
pub fn probe_audio_args(src: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> =
        ["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_type", "-of", "default=nw=1"].map(OsString::from).to_vec();
    args.push(src.as_os_str().to_owned());
    args
}

pub fn has_audio(probe_output: &str) -> bool {
    probe_output.lines().any(|l| l.trim() == "codec_type=audio")
}

#[cfg_attr(not(windows), allow(dead_code))] // 只有 Windows 版會播放聲音
/// 播放聲音：從 start 開始，轉成喇叭要的取樣率與聲道數，輸出 32-bit 浮點 PCM 到 stdout
pub fn audio_args(src: &Path, start: f64, rate: u32, channels: u32) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error"].map(OsString::from).to_vec();
    args.extend(["-ss".into(), ts(start).into(), "-i".into(), src.as_os_str().to_owned()]);
    args.extend(["-vn".into(), "-ac".into(), channels.to_string().into(), "-ar".into(), rate.to_string().into()]);
    args.extend(["-f", "f32le", "pipe:1"].map(OsString::from));
    args
}

/// 計算波形：整條音軌混成單聲道、8 kHz、16-bit
pub fn waveform_args(src: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-i"].map(OsString::from).to_vec();
    args.push(src.as_os_str().to_owned());
    let rate = crate::waveform::RATE.to_string();
    args.extend(["-vn".into(), "-ac".into(), "1".into(), "-ar".into(), rate.into()]);
    args.extend(["-f", "s16le", "pipe:1"].map(OsString::from));
    args
}

fn parse_ratio(s: &str) -> Option<f64> {
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
            (b != 0.0).then(|| a / b)
        }
        None => s.parse().ok(),
    }
}

/// 預覽畫面大小：高度不超過 PREVIEW_MAX_H，寬高都取偶數（rawvideo 好讀、縮放濾鏡也要求）
pub fn preview_size(w: u32, h: u32) -> (u32, u32) {
    let even = |v: u32| (v.max(2) / 2) * 2;
    if h <= PREVIEW_MAX_H {
        return (even(w), even(h));
    }
    let pw = (w as f64 * PREVIEW_MAX_H as f64 / h as f64).round() as u32;
    (even(pw), even(PREVIEW_MAX_H))
}

/// 要轉換的範圍 [a, b]（秒），永遠保持 0 ≤ a < b ≤ 影片長度
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub a: f64,
    pub b: f64,
}

impl Selection {
    pub fn full(duration: f64) -> Self {
        Self { a: 0.0, b: duration.max(MIN_SPAN) }
    }

    pub fn span(self) -> f64 {
        self.b - self.a
    }

    /// 設 A 點；若越過 B，B 改到影片結尾
    pub fn with_a(self, t: f64, duration: f64) -> Self {
        let a = t.clamp(0.0, (duration - MIN_SPAN).max(0.0));
        let b = if self.b - a < MIN_SPAN { duration } else { self.b };
        Self { a, b }
    }

    /// 設 B 點；若越過 A，A 改到影片開頭
    pub fn with_b(self, t: f64, duration: f64) -> Self {
        let b = t.clamp(MIN_SPAN.min(duration), duration);
        let a = if b - self.a < MIN_SPAN { 0.0 } else { self.a };
        Self { a, b }
    }

    /// 在時間軸上從 t0 拖到 t1 選出的新範圍（往左拖也可以）
    pub fn from_drag(t0: f64, t1: f64, duration: f64) -> Self {
        let (lo, hi) = (t0.min(t1).clamp(0.0, duration), t0.max(t1).clamp(0.0, duration));
        if hi - lo >= MIN_SPAN {
            return Self { a: lo, b: hi };
        }
        // 太短：從起點往後補足最小長度，貼到結尾時往前補
        let b = (lo + MIN_SPAN).min(duration);
        Self { a: (b - MIN_SPAN).max(0.0), b }
    }
}

/// 兩種轉檔模式（都使用 AMD 顯示卡硬體編碼）
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// 模式 1：原尺寸
    #[default]
    Original,
    /// 模式 2：縮成 720p（依原比例）
    P720,
}

/// AMD AV1 編碼器只有寬高都是 16 的倍數時才會照實輸出；否則會自行補邊（例如 1080 變 1082），
/// 造成檔頭記錄的大小與實際畫面不符，Firefox、FastStone 等嚴格的看圖程式會打不開
const ALIGN: u32 = 16;

fn align_down(v: u32) -> u32 {
    (v / ALIGN * ALIGN).max(ALIGN)
}

impl Mode {
    /// 輸出畫面大小，寬高都是 16 的倍數。
    /// 模式 1：往下取到 16 的倍數、置中裁掉多的（1080p → 1920×1072，上下各裁 4 像素）。
    /// 模式 2：高 720，寬依原比例取最接近的 16 的倍數；原本就不到 720 高時同模式 1（不放大）。
    pub fn output_size(self, info: &VideoInfo) -> (u32, u32) {
        match self {
            Mode::P720 if info.height > 720 => {
                let w = (info.width as f64 * 720.0 / info.height as f64 / ALIGN as f64).round() as u32 * ALIGN;
                (w.max(ALIGN), 720)
            }
            _ => (align_down(info.width), align_down(info.height)),
        }
    }
}

/// 可選的輸出影格率
pub const FPS_CHOICES: [u32; 3] = [15, 30, 60];
pub const DEFAULT_FPS: u32 = 15;

/// 實際輸出的影格率：原影片比選的慢時維持原影片（不重複影格）
pub fn output_fps(fps: u32, info: &VideoInfo) -> f64 {
    if info.fps > fps as f64 + 0.5 { fps as f64 } else { info.fps }
}

/// ffmpeg 的影像濾鏡：先降影格率（減少後面要處理的畫面），再縮放或裁切成 16 的倍數
pub fn video_filter(mode: Mode, fps: u32, info: &VideoInfo) -> Option<String> {
    let mut parts = Vec::new();
    if info.fps > fps as f64 + 0.5 {
        parts.push(format!("fps={fps}"));
    }
    let (w, h) = mode.output_size(info);
    if mode == Mode::P720 && info.height > 720 {
        parts.push(format!("scale={w}:{h}"));
    } else if (w, h) != (info.width, info.height) {
        parts.push(format!("crop={w}:{h}")); // 預設置中裁切
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

/// 秒數寫成 ffmpeg 接受的字串（毫秒精度，固定用小數點）
pub fn ts(t: f64) -> String {
    format!("{:.3}", t.max(0.0))
}

/// 把影片範圍轉成動態 AVIF 的 ffmpeg 參數。`dst` 通常是 .part 暫存檔，所以明確指定 `-f avif`
pub fn encode_args(src: &Path, dst: &Path, sel: Selection, mode: Mode, fps: u32, info: &VideoInfo) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-y", "-loglevel", "error"].map(OsString::from).to_vec();
    args.extend(["-ss".into(), ts(sel.a).into(), "-to".into(), ts(sel.b).into(), "-i".into(), src.as_os_str().to_owned()]);
    if let Some(vf) = video_filter(mode, fps, info) {
        args.extend(["-vf".into(), vf.into()]);
    }
    let rest = [
        "-an", "-c:v", "av1_amf", "-usage", "transcoding", "-quality", "quality", "-rc", "qvbr",
        "-qvbr_quality_level", QVBR_LEVEL, "-progress", "pipe:1", "-nostats", "-f", "avif",
    ];
    args.extend(rest.map(OsString::from));
    args.push(dst.as_os_str().to_owned());
    args
}

/// 解碼預覽影格的參數：從 start 開始輸出 RGBA rawvideo 到 stdout；frames = Some(1) 只取一格
pub fn preview_args(src: &Path, start: f64, size: (u32, u32), frames: Option<u32>) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error"].map(OsString::from).to_vec();
    args.extend(["-ss".into(), ts(start).into(), "-i".into(), src.as_os_str().to_owned(), "-an".into()]);
    let vf = format!("fps={PREVIEW_FPS},scale={}:{}:flags=bilinear", size.0, size.1);
    args.extend(["-vf".into(), vf.into()]);
    if let Some(n) = frames {
        args.extend(["-frames:v".into(), n.to_string().into()]);
    }
    args.extend(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"].map(OsString::from));
    args
}

pub fn probe_args(src: &Path) -> Vec<OsString> {
    let entries = "stream=width,height,avg_frame_rate:stream_side_data=rotation:format=duration";
    let mut args: Vec<OsString> = ["-v", "error", "-select_streams", "v:0", "-show_entries", entries, "-of", "default=nw=1"]
        .map(OsString::from)
        .to_vec();
    args.push(src.as_os_str().to_owned());
    args
}

/// `-progress` 輸出的一行；回傳已編碼到的秒數
pub fn parse_progress(line: &str) -> Option<f64> {
    let (k, v) = line.trim().split_once('=')?;
    // out_time_us 與 out_time_ms 在 ffmpeg 裡其實都是微秒
    matches!(k, "out_time_us" | "out_time_ms").then(|| v.parse::<i64>().ok()).flatten().filter(|v| *v >= 0).map(|us| us as f64 / 1e6)
}

/// 把 ffmpeg 的錯誤輸出轉成使用者看得懂的訊息
pub fn friendly_error(stderr: &str) -> String {
    let s = stderr.to_ascii_lowercase();
    let lines: Vec<&str> = stderr.lines().map(strip_log_prefix).filter(|l| !l.is_empty()).collect();
    let detail = lines.last().copied().unwrap_or("");
    if s.contains("amf") && (s.contains("dll") || s.contains("failed") || s.contains("not supported") || s.contains("unknown encoder")) {
        // 顯示第一行提到 AMF 的原因，最後一行通常只是「沒有寫出任何東西」
        let why = lines.iter().find(|l| l.to_ascii_lowercase().contains("amf")).copied().unwrap_or(detail);
        return format!("無法使用 AMD 顯示卡的 AV1 硬體編碼。需要 Radeon RX 7000 系列以上的顯示卡並安裝最新驅動程式。（{why}）");
    }
    if s.contains("unknown encoder") || s.contains("encoder not found") {
        return format!("這個 ffmpeg 沒有 AMD AV1 編碼器（{detail}）");
    }
    if detail.is_empty() { "轉換失敗（ffmpeg 沒有回報原因）".into() } else { format!("轉換失敗：{detail}") }
}

/// 去掉 ffmpeg 記錄行開頭的 `[av1_amf @ 0x1234] ` 之類標記（可能有好幾個）
fn strip_log_prefix(line: &str) -> &str {
    let mut l = line.trim();
    while l.starts_with('[') && let Some(end) = l.find("] ") {
        l = l[end + 2..].trim_start();
    }
    l
}

/// 輸出路徑：沿用圖片的輸出位置規則，檔名加上範圍，例如 `clip_00m05s-00m12s.avif`
pub fn output_path(src: &Path, base_dir: PathBuf, sel: Selection) -> PathBuf {
    let stem = src.file_stem().unwrap_or_default().to_string_lossy();
    base_dir.join(format!("{stem}_{}-{}.avif", short_time(sel.a), short_time(sel.b)))
}

fn short_time(t: f64) -> String {
    let s = t.max(0.0).floor() as u64;
    format!("{:02}m{:02}s", s / 60, s % 60)
}

/// 顯示用時間：mm:ss.cc（超過一小時加上小時）
pub fn format_time(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    let (h, m, s, c) = (cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100);
    if h > 0 { format!("{h}:{m:02}:{s:02}.{c:02}") } else { format!("{m:02}:{s:02}.{c:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: VideoInfo = VideoInfo { width: 1920, height: 1080, duration: 10.0, fps: 30.0, has_audio: true };

    #[test]
    fn audio_detection_and_args() {
        assert!(has_audio("codec_type=audio\n"));
        assert!(!has_audio(""));
        let a = strs(&audio_args(Path::new("a.mp4"), 3.0, 48000, 2)).join(" ");
        assert!(a.contains("-ss 3.000 -i a.mp4 -vn -ac 2 -ar 48000 -f f32le pipe:1"), "{a}");
        let w = strs(&waveform_args(Path::new("a.mp4"))).join(" ");
        assert!(w.ends_with("-i a.mp4 -vn -ac 1 -ar 8000 -f s16le pipe:1"), "{w}");
    }

    fn strs(v: &[OsString]) -> Vec<String> {
        v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn probe_parses_landscape() {
        let out = "width=1920\nheight=1080\navg_frame_rate=30000/1001\nduration=12.345000\n";
        let info = parse_probe(out).unwrap();
        assert_eq!((info.width, info.height), (1920, 1080));
        assert!((info.duration - 12.345).abs() < 1e-9);
        assert!((info.fps - 29.97).abs() < 0.01);
    }

    #[test]
    fn probe_swaps_size_for_rotated_phone_video() {
        let out = "width=1920\nheight=1080\navg_frame_rate=30/1\nrotation=-90\nduration=3.0\n";
        let info = parse_probe(out).unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
    }

    #[test]
    fn probe_errors_without_video_or_duration() {
        assert!(parse_probe("duration=3.0\n").is_err());
        assert!(parse_probe("width=10\nheight=10\nduration=N/A\n").is_err());
        let info = parse_probe("width=10\nheight=10\navg_frame_rate=0/0\nduration=1\n").unwrap();
        assert_eq!(info.fps, 30.0, "影格率無效時退回 30");
    }

    #[test]
    fn preview_size_limits_height_and_keeps_even() {
        assert_eq!(preview_size(1920, 1080), (960, 540));
        assert_eq!(preview_size(1080, 1920), (304, 540));
        assert_eq!(preview_size(641, 361), (640, 360));
    }

    #[test]
    fn selection_set_a_and_b() {
        let s = Selection::full(10.0);
        let s = s.with_a(2.0, 10.0).with_b(5.0, 10.0);
        assert_eq!(s, Selection { a: 2.0, b: 5.0 });
        // A 越過 B：B 退到結尾；B 越過 A：A 退到開頭
        assert_eq!(s.with_a(7.0, 10.0), Selection { a: 7.0, b: 10.0 });
        assert_eq!(s.with_b(1.0, 10.0), Selection { a: 0.0, b: 1.0 });
        // 超出範圍會被夾住
        assert_eq!(s.with_a(-3.0, 10.0).a, 0.0);
        assert_eq!(s.with_b(99.0, 10.0).b, 10.0);
    }

    #[test]
    fn selection_from_drag_any_direction() {
        assert_eq!(Selection::from_drag(6.0, 2.0, 10.0), Selection { a: 2.0, b: 6.0 });
        assert_eq!(Selection::from_drag(-1.0, 20.0, 10.0), Selection { a: 0.0, b: 10.0 });
        let tiny = Selection::from_drag(9.98, 10.0, 10.0);
        assert!((tiny.span() - MIN_SPAN).abs() < 1e-9 && tiny.b == 10.0);
    }

    #[test]
    fn mode_output_sizes_are_multiples_of_16() {
        // 1080 不是 16 的倍數：裁成 1072，不再讓 AMD 編碼器自己補成 1082
        assert_eq!(Mode::Original.output_size(&HD), (1920, 1072));
        assert_eq!(Mode::P720.output_size(&HD), (1280, 720));
        let small = VideoInfo { width: 1280, height: 720, ..HD };
        assert_eq!(Mode::P720.output_size(&small), (1280, 720), "不放大");
        let tall = VideoInfo { width: 1080, height: 1920, ..HD };
        assert_eq!(Mode::Original.output_size(&tall), (1072, 1920));
        assert_eq!(Mode::P720.output_size(&tall), (400, 720), "405 取最接近的 16 倍數");
        for info in [HD, small, tall, VideoInfo { width: 1000, height: 750, ..HD }] {
            for m in [Mode::Original, Mode::P720] {
                let (w, h) = m.output_size(&info);
                assert!(w % 16 == 0 && h % 16 == 0, "{m:?} {info:?} → {w}×{h}");
            }
        }
    }

    #[test]
    fn filter_chain_fps_then_size() {
        let hd60 = VideoInfo { fps: 59.94, ..HD };
        assert_eq!(video_filter(Mode::Original, 15, &hd60).as_deref(), Some("fps=15,crop=1920:1072"));
        assert_eq!(video_filter(Mode::P720, 30, &hd60).as_deref(), Some("fps=30,scale=1280:720"));
        // 選 60、原片 59.94：不改影格率
        assert_eq!(video_filter(Mode::P720, 60, &hd60).as_deref(), Some("scale=1280:720"));
        // 原片 30、選 60：不重複影格
        assert_eq!(output_fps(60, &HD), 30.0);
        assert_eq!(output_fps(15, &hd60), 15.0);
        // 已經是 16 的倍數又不降影格率：不需要濾鏡
        let exact = VideoInfo { width: 1280, height: 720, ..HD };
        assert_eq!(video_filter(Mode::Original, 30, &exact), None);
    }

    #[test]
    fn encode_args_mode1_and_mode2() {
        let sel = Selection { a: 1.5, b: 4.25 };
        let src = Path::new("影片 1.mp4");
        let a1 = strs(&encode_args(src, Path::new("out.avif.part"), sel, Mode::Original, 15, &HD));
        let joined = a1.join(" ");
        assert!(joined.contains("-ss 1.500 -to 4.250 -i 影片 1.mp4"));
        assert!(joined.contains("-vf fps=15,crop=1920:1072"), "{joined}");
        assert!(joined.contains("-c:v av1_amf") && joined.contains("-rc qvbr -qvbr_quality_level 24"));
        assert!(joined.contains("-an") && joined.ends_with("-f avif out.avif.part"));
        let a2 = strs(&encode_args(src, Path::new("o"), sel, Mode::P720, 30, &HD));
        let i = a2.iter().position(|s| s == "-vf").unwrap();
        assert_eq!(a2[i + 1], "scale=1280:720");
        // 濾鏡必須在 -i 之後（作用於輸出）
        assert!(i > a2.iter().position(|s| s == "-i").unwrap());
    }

    #[test]
    fn preview_args_single_frame() {
        let a = strs(&preview_args(Path::new("a.mp4"), 2.0, (960, 540), Some(1))).join(" ");
        assert!(a.contains("-ss 2.000 -i a.mp4"));
        assert!(a.contains("scale=960:540") && a.contains("-frames:v 1"));
        assert!(a.ends_with("-pix_fmt rgba -f rawvideo pipe:1"));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("out_time_us=2500000"), Some(2.5));
        assert_eq!(parse_progress("out_time_ms=1000000\r"), Some(1.0));
        assert_eq!(parse_progress("out_time_us=N/A"), None);
        assert_eq!(parse_progress("out_time_us=-9223372036854775807"), None);
        assert_eq!(parse_progress("frame=12"), None);
    }

    #[test]
    fn friendly_errors() {
        // 實際在沒有 AMD 驅動的環境跑出來的錯誤（節錄）
        let amf = "[AMF @ 0x74bf] DLL amfrt64.dll failed to open\n[vost#0:0/av1_amf @ 0x5] [enc:av1_amf @ 0x5] Error while opening encoder\n[out#0/avif @ 0x5] Nothing was written into output file";
        let msg = friendly_error(amf);
        assert!(msg.contains("AMD 顯示卡") && msg.contains("（DLL amfrt64.dll failed to open）"), "{msg}");
        assert!(friendly_error("").contains("沒有回報原因"));
        assert!(friendly_error("in.mp4: No such file or directory\n").contains("No such file"));
    }

    #[test]
    fn output_name_and_time_format() {
        let p = output_path(Path::new("/v/旅行 影片.mp4"), PathBuf::from("/out"), Selection { a: 65.9, b: 130.0 });
        assert_eq!(p, Path::new("/out").join("旅行 影片_01m05s-02m10s.avif"));
        assert_eq!(format_time(83.456), "01:23.46");
        assert_eq!(format_time(3725.0), "1:02:05.00");
        assert!(is_video_file(Path::new("x.MP4")) && !is_video_file(Path::new("x.avif")));
    }
}
