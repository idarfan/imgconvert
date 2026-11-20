#![cfg_attr(windows, windows_subsystem = "windows")] // Windows 下不跳出命令列視窗

mod audio;
mod convert;
mod ffmpeg;
mod help;
mod icons;
mod player;
mod settings;
mod theme;
mod timeline;
mod video;
mod view;
mod waveform;
mod video_view;

use convert::{Options, Outcome, Rotation};
use eframe::egui;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const APP_NAME: &str = "ImgConvert 圖片轉檔器";
const APP_TITLE: &str = concat!("ImgConvert 圖片轉檔器 v", env!("CARGO_PKG_VERSION"));

#[derive(Clone, PartialEq)]
enum Status {
    Pending,
    Done(PathBuf),
    Skipped,
    Failed(String),
}

struct Item {
    path: PathBuf,
    /// 使用者選取的根資料夾，用來在輸出資料夾重建子目錄
    root: Option<PathBuf>,
    rot: Rotation,
    status: Status,
}

struct Job {
    total: usize,
    done: Arc<AtomicUsize>,
    ok: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    skipped: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    results: Arc<Mutex<Vec<(usize, Status)>>>,
    started: Instant,
    elapsed: Option<f32>,
}

struct Preview {
    path: PathBuf,
    img: Option<image::DynamicImage>,
    err: Option<String>,
    tex: Option<(Rotation, egui::TextureHandle)>,
    rx: Option<Receiver<Result<image::DynamicImage, String>>>,
}

/// 左側上方的分頁
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Images,
    Video,
}

struct App {
    tab: Tab,
    video: video_view::VideoTab,
    items: Vec<Item>,
    recursive: bool,
    global_rot: Rotation,
    opts: Options,
    same_dir: bool,
    out_dir: Option<PathBuf>,
    /// 左側設定卡片的排列順序（可拖曳調整）
    card_order: [settings::CardKind; settings::CARD_COUNT],
    /// 影片分頁設定卡片的排列順序
    video_card_order: [settings::VideoCard; settings::VIDEO_CARD_COUNT],
    /// 上次寫入磁碟的設定；與目前狀態不同時才重新存檔
    saved: settings::Settings,
    job: Option<Job>,
    selected: Option<usize>,
    preview: Option<Preview>,
    message: String,
    show_help: bool,
    icons: icons::Icons,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_cjk_font(&cc.egui_ctx);
        theme::install(&cc.egui_ctx);
        let saved = settings::Settings::load();
        let mut app = Self {
            tab: Tab::Images,
            video: video_view::VideoTab::new(saved.volume),
            items: Vec::new(),
            recursive: true,
            global_rot: Rotation::R0,
            opts: Options::default(),
            same_dir: saved.same_dir,
            out_dir: saved.out_dir.clone(),
            card_order: saved.card_order,
            video_card_order: saved.video_card_order,
            saved,
            job: None,
            selected: None,
            preview: None,
            message: "請選擇資料夾，或把檔案／資料夾拖曳到視窗中。".into(),
            show_help: false,
            icons: icons::Icons::load(&cc.egui_ctx),
        };
        // 支援把檔案／資料夾拖到 exe 圖示上開啟，或命令列參數
        for arg in std::env::args_os().skip(1) {
            let p = Path::new(&arg);
            if p.is_file() && video::is_video_file(p) {
                app.open_video(p, &cc.egui_ctx);
            } else {
                app.add_path(p);
            }
        }
        app
    }

    fn current_settings(&self) -> settings::Settings {
        settings::Settings {
            card_order: self.card_order,
            video_card_order: self.video_card_order,
            same_dir: self.same_dir,
            out_dir: self.out_dir.clone(),
            volume: self.video.volume,
        }
    }

    /// 設定有變動就立刻存檔，關閉程式（甚至當掉）也不會遺失
    fn persist_settings(&mut self) {
        let now = self.current_settings();
        if now == self.saved {
            return;
        }
        if let Err(e) = now.save() {
            self.message = format!("設定未能儲存：{e}");
        }
        self.saved = now;
    }

    fn running(&self) -> bool {
        self.job.as_ref().is_some_and(|j| !j.finished.load(Ordering::Relaxed))
    }

    fn add_path(&mut self, p: &Path) {
        let before = self.items.len();
        if p.is_dir() {
            for f in convert::scan(p, self.recursive) {
                self.push(f, Some(p.to_path_buf()));
            }
        } else if convert::is_input_file(p) {
            self.push(p.to_path_buf(), p.parent().map(Path::to_path_buf));
        }
        let added = self.items.len() - before;
        self.message = format!("加入 {added} 個檔案，共 {} 個。", self.items.len());
    }

    fn push(&mut self, path: PathBuf, root: Option<PathBuf>) {
        if !self.items.iter().any(|i| i.path == path) {
            self.items.push(Item { path, root, rot: Rotation::R0, status: Status::Pending });
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let mut opts = self.opts.clone();
        opts.out_dir = if self.same_dir { None } else { self.out_dir.clone() };
        // 選「指定資料夾」但沒選路徑：自動輸出到來源資料夾底下的「已轉換好」
        opts.sub_dir = (!self.same_dir && opts.out_dir.is_none()).then(|| convert::DEFAULT_SUBDIR.to_string());
        for it in &mut self.items {
            it.status = Status::Pending;
        }
        let tasks: Vec<(usize, PathBuf, Option<PathBuf>, Rotation)> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| (i, it.path.clone(), it.root.clone(), self.global_rot.add(it.rot)))
            .collect();
        let job = Job {
            total: tasks.len(),
            done: Default::default(),
            ok: Default::default(),
            failed: Default::default(),
            skipped: Default::default(),
            cancel: Default::default(),
            finished: Default::default(),
            results: Default::default(),
            started: Instant::now(),
            elapsed: None,
        };
        let (done, ok, failed, skipped, cancel, finished, results) = (
            job.done.clone(),
            job.ok.clone(),
            job.failed.clone(),
            job.skipped.clone(),
            job.cancel.clone(),
            job.finished.clone(),
            job.results.clone(),
        );
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            tasks.par_iter().for_each(|(i, path, root, rot)| {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let st = match convert::convert_file(path, root.as_deref(), *rot, &opts) {
                    Ok(Outcome::Written(p)) => {
                        ok.fetch_add(1, Ordering::Relaxed);
                        Status::Done(p)
                    }
                    Ok(Outcome::Skipped(_)) => {
                        skipped.fetch_add(1, Ordering::Relaxed);
                        Status::Skipped
                    }
                    Err(e) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        Status::Failed(e)
                    }
                };
                results.lock().unwrap().push((*i, st));
                done.fetch_add(1, Ordering::Relaxed);
                ctx.request_repaint();
            });
            finished.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
        self.job = Some(job);
        self.message = "轉換中…".into();
    }

    /// 把背景執行緒結果同步回清單
    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        for (i, st) in job.results.lock().unwrap().drain(..) {
            if let Some(it) = self.items.get_mut(i) {
                it.status = st;
            }
        }
        if job.finished.load(Ordering::Relaxed) && job.elapsed.is_none() {
            let secs = job.started.elapsed().as_secs_f32();
            job.elapsed = Some(secs);
            let cancelled = job.cancel.load(Ordering::Relaxed);
            self.message = format!(
                "{}：成功 {}、失敗 {}、略過 {}，耗時 {:.1} 秒。",
                if cancelled { "已取消" } else { "完成" },
                job.ok.load(Ordering::Relaxed),
                job.failed.load(Ordering::Relaxed),
                job.skipped.load(Ordering::Relaxed),
                secs
            );
        }
    }

    fn select(&mut self, idx: usize, ctx: &egui::Context) {
        self.selected = Some(idx);
        let path = self.items[idx].path.clone();
        if self.preview.as_ref().is_some_and(|p| p.path == path) {
            return;
        }
        let (tx, rx) = channel();
        let p2 = path.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = convert::decode(&p2).map(|img| img.thumbnail(640, 640));
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.preview = Some(Preview { path, img: None, err: None, tex: None, rx: Some(rx) });
    }
}
/// egui 內建字型沒有中文：載入 Windows 系統中文字型作為後備
fn install_cjk_font(ctx: &egui::Context) {
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\msjh.ttc", 0),   // 微軟正黑體
        (r"C:\Windows\Fonts\msyh.ttc", 0),   // 微軟雅黑
        (r"C:\Windows\Fonts\mingliu.ttc", 0), // 細明體
        (r"C:\Windows\Fonts\YuGothM.ttc", 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ];
    for (path, index) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            let mut data = egui::FontData::from_owned(bytes);
            data.index = *index;
            fonts.font_data.insert("cjk".into(), Arc::new(data));
            // 放在第一個拉丁字型之後、emoji 字型之前，避免全形標點被 emoji 字型搶走
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                let list = fonts.families.entry(fam).or_default();
                let pos = list.len().min(1);
                list.insert(pos, "cjk".into());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

fn main() -> eframe::Result {
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png")).ok().map(|i| {
        let i = i.to_rgba8();
        egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() }
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_inner_size([1200.0, 780.0])
        .with_min_inner_size([980.0, 620.0])
        .with_maximized(true) // 一開啟就最大化填滿螢幕（保留標題列與工作列）
        .with_drag_and_drop(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
