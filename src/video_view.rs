//! 「MP4 to AVIF」分頁：左側選影片與轉檔模式，中間播放影片、時間軸選 A／B 範圍。

use crate::convert::{self, ExistPolicy};
use crate::ffmpeg::{self, EncodeJob};
use crate::icons::img;
use crate::player::{Player, Wave};
use crate::settings::{Card, VideoCard};
use crate::timeline::WaveView;
use crate::video::{self, Mode, format_time};
use crate::{App, theme, timeline};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

/// 播放區下方控制列與時間軸大約佔的高度
const CONTROLS_H: f32 = 240.0;

pub struct VideoTab {
    pub mode: Mode,
    /// 輸出影格率（15／30／60），預設 15
    pub fps: u32,
    pub player: Option<Player>,
    pub job: Option<EncodeJob>,
    /// 預覽音量 0–100（%），會記在設定檔
    pub volume: u8,
    pub muted: bool,
}

impl VideoTab {
    pub fn new(volume: u8) -> Self {
        Self { mode: Mode::default(), fps: video::DEFAULT_FPS, player: None, job: None, volume, muted: false }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// 實際送給喇叭的音量 0.0–1.0
    fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume as f32 / 100.0 }
    }
}

impl App {
    pub fn open_video(&mut self, path: &Path, ctx: &egui::Context) {
        if self.video.running() {
            return;
        }
        self.video.player = Some(Player::open(path.to_path_buf(), ctx, self.video.effective_volume()));
        self.tab = crate::Tab::Video;
        self.message = format!("讀取影片：{}", path.file_name().unwrap_or_default().to_string_lossy());
    }

    /// 每幀呼叫：更新播放器、收轉檔結果
    pub fn poll_video(&mut self, ctx: &egui::Context) {
        if let Some(p) = &mut self.video.player {
            p.poll(ctx);
        }
        let Some(job) = &self.video.job else { return };
        let Some(result) = job.take_result() else { return };
        let secs = job.started.elapsed().as_secs_f32();
        self.message = match result {
            Ok(done) => {
                let size = std::fs::metadata(&done.path).map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
                let warn = done.warning.map(|w| format!(" {w}")).unwrap_or_default();
                format!("完成：{}（{size:.1} MB），耗時 {secs:.1} 秒。{warn}", done.path.display())
            }
            Err(e) if job.is_cancelled() => format!("{e}。"),
            Err(e) => e,
        };
        self.video.job = None;
    }

    /// 依「輸出位置」「同名檔案」設定決定輸出檔，然後開始轉檔
    pub fn start_video(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.video.player else { return };
        let Some(info) = p.info else { return };
        p.pause();
        let base = match (self.same_dir, &self.out_dir) {
            (true, _) => p.path.parent().map(Path::to_path_buf).unwrap_or_default(),
            (false, Some(d)) => d.clone(),
            (false, None) => p.path.parent().map(|d| d.join(convert::DEFAULT_SUBDIR)).unwrap_or_default(),
        };
        let mut dst = video::output_path(&p.path, base, p.sel);
        if dst.exists() {
            match self.opts.exist {
                ExistPolicy::Skip => {
                    self.message = format!("已有同名檔案，略過：{}", dst.display());
                    return;
                }
                ExistPolicy::Overwrite => {}
                ExistPolicy::Rename => dst = convert::unique_path(&dst),
            }
        }
        let c = ctx.clone();
        let (mode, fps) = (self.video.mode, self.video.fps);
        self.video.job = Some(ffmpeg::encode(&p.path, dst, p.sel, mode, fps, &info, move || c.request_repaint()));
        self.message = "使用 AMD 顯示卡轉換中…".into();
    }

    pub fn video_progress(&self) -> Option<(f32, String)> {
        let job = self.video.job.as_ref()?;
        let f = job.progress();
        Some((f, format!("{} / {}（{:.0}%）", format_time(job.total_secs * f as f64), format_time(job.total_secs), f * 100.0)))
    }

    pub fn video_sidebar_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.video.running();
        theme::section_label(ui, "加入影片");
        ui.add_enabled_ui(!running, |ui| {
            let w = ui.available_width();
            if theme::tile(ui, &self.icons.movie, "選擇 MP4 影片", w, egui::Sense::click()).clicked() {
                let dlg = rfd::FileDialog::new().add_filter("影片（MP4 / MOV）", &video::VIDEO_EXTS);
                if let Some(f) = dlg.pick_file() {
                    self.open_video(&f, ui.ctx());
                }
            }
        });
        ui.add_space(2.0);
        ui.weak("也可以直接把影片拖曳到視窗");
        theme::section_label(ui, "轉檔設定");
        ui.add_enabled_ui(!running, |ui| self.video_cards_ui(ui));
    }

    /// 三張設定卡片依 `video_card_order` 排列；和圖片分頁一樣按住右上角把手拖曳調整順序
    fn video_cards_ui(&mut self, ui: &mut egui::Ui) {
        let order = self.video_card_order;
        if let Some(o) = crate::view::reorder_cards(ui, order, |ui, kind, dimmed| self.video_card_ui(ui, kind, dimmed)) {
            self.video_card_order = o;
        }
    }

    fn video_card_ui(&mut self, ui: &mut egui::Ui, kind: VideoCard, dimmed: bool) -> (egui::Rect, egui::Response) {
        let App { icons, video, same_dir, out_dir, opts, .. } = self;
        let grip_id = egui::Id::unique(("video_card_grip", kind.index()));
        let card = |ui: &mut egui::Ui, icon, title, desc: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            theme::draggable_settings_card(ui, grip_id, dimmed, icon, title, desc, |ui| add(ui))
        };
        match kind {
            VideoCard::Mode => {
                let desc = mode_desc(video);
                card(ui, &icons.gear, "轉檔模式", &desc, &mut |ui| mode_controls_ui(ui, video))
            }
            VideoCard::Location => card(ui, &icons.outbox, "輸出位置", "與圖片轉檔共用同一個設定", &mut |ui| {
                crate::view::out_location_ui(ui, same_dir, out_dir);
            }),
            VideoCard::Exist => card(ui, &icons.dividers, "同名檔案", "輸出位置已有同名檔案時的處理方式", &mut |ui| {
                let choices = [(ExistPolicy::Rename, "自動改名"), (ExistPolicy::Overwrite, "覆蓋"), (ExistPolicy::Skip, "略過")];
                theme::segmented(ui, &mut opts.exist, &choices);
            }),
        }
    }
}

fn mode_desc(video: &VideoTab) -> String {
    match video.player.as_ref().and_then(|p| p.info) {
        Some(i) => {
            let (w, h) = video.mode.output_size(&i);
            format!("AMD 顯示卡硬體編碼，輸出 {w}×{h}、{:.0} fps 動態 AVIF（無聲音）", video::output_fps(video.fps, &i))
        }
        None => "AMD 顯示卡硬體編碼，輸出動態 AVIF（無聲音）".into(),
    }
}

fn mode_controls_ui(ui: &mut egui::Ui, video: &mut VideoTab) {
    theme::segmented(ui, &mut video.mode, &[(Mode::Original, "模式 1：原尺寸"), (Mode::P720, "模式 2：720p")]);
    let fps_choices = video::FPS_CHOICES.map(|f| (f, format!("{f} fps")));
    let fps_opts: Vec<(u32, &str)> = fps_choices.iter().map(|(f, s)| (*f, s.as_str())).collect();
    theme::segmented(ui, &mut video.fps, &fps_opts);
    ui.weak("寬高會取到 16 的倍數（AMD 編碼器限制），例如 1080p 上下各裁 4 像素成 1072；原影片影格率較低時維持原影格率。");
}

impl App {
    /// 中間區：影片畫面＋控制列＋時間軸
    pub fn video_main_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.video.running();
        let ctx = ui.ctx().clone();
        let vt = &mut self.video;
        theme::card(ui, |ui| {
            ui.set_min_height(ui.available_height());
            let Some(p) = &mut vt.player else {
                video_empty_ui(ui, &self.icons);
                return;
            };
            header_ui(ui, p);
            if let Some(e) = &p.err {
                ui.colored_label(theme::ERR, e);
            }
            if let Some(n) = &p.audio_note {
                ui.weak(format!("聲音無法播放：{n}（畫面照常播放）"));
            }
            let Some(info) = p.info else {
                if p.err.is_none() {
                    ui.centered_and_justified(|ui| ui.spinner());
                }
                return;
            };
            frame_ui(ui, p);
            ui.add_enabled_ui(!running, |ui| {
                controls_ui(ui, p, &ctx, &mut vt.volume, &mut vt.muted);
                let wave = match &p.wave {
                    Wave::Ready(bins) => WaveView::Bins(bins),
                    Wave::Loading(_) => WaveView::Message("分析音訊波形中…"),
                    Wave::NoAudio => WaveView::Message("這部影片沒有聲音"),
                    Wave::Failed(e) => WaveView::Message(e),
                };
                let out = timeline::show(ui, info.duration, p.pos, p.sel, wave);
                if let Some(s) = out.sel {
                    p.sel = s;
                }
                if let Some(t) = out.seek {
                    p.seek(t, &ctx);
                }
                selection_ui(ui, p);
            });
            if !running {
                shortcuts(ui, p, &ctx, &mut vt.muted);
            }
            p.set_volume(if vt.muted { 0.0 } else { vt.volume as f32 / 100.0 });
        });
    }
}

fn header_ui(ui: &mut egui::Ui, p: &Player) {
    ui.horizontal(|ui| {
        let name = p.path.file_name().unwrap_or_default().to_string_lossy();
        ui.add(egui::Label::new(RichText::new(name.as_ref()).strong().size(16.0)).truncate())
            .on_hover_text(p.path.display().to_string());
        if let Some(i) = p.info {
            let chips = [format!("{}×{}", i.width, i.height), format!("{:.2} fps", i.fps), format!("長度 {}", format_time(i.duration))];
            for c in chips {
                theme::chip(ui, &c, theme::accent(ui));
            }
        }
    });
    ui.add_space(4.0);
}

fn frame_ui(ui: &mut egui::Ui, p: &Player) {
    let avail = egui::vec2(ui.available_width(), (ui.available_height() - CONTROLS_H).max(120.0));
    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::same(8), egui::Color32::BLACK);
    if let Some(tex) = &p.tex {
        let size = tex.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        let r = egui::Rect::from_center_size(rect.center(), size * scale);
        egui::Image::new(tex).paint_at(ui, r);
    } else {
        ui.put(rect, egui::Spinner::new());
    }
}

fn controls_ui(ui: &mut egui::Ui, p: &mut Player, ctx: &egui::Context, volume: &mut u8, muted: &mut bool) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let label = if p.playing { "⏸ 暫停" } else { "▶ 播放" };
        if ui.button(label).on_hover_text("空白鍵").clicked() {
            p.toggle_play(ctx);
        }
        if ui.button("▶ 播放範圍").on_hover_text("從 A 播到 B").clicked() {
            p.play_selection(ctx);
        }
        ui.label(RichText::new(format!("{} / {}", format_time(p.pos), format_time(p.duration()))).monospace());
        ui.separator();
        let d = p.duration();
        if ui.button("設為 A 點").on_hover_text("把目前位置設為起點（快速鍵 I）").clicked() {
            p.sel = p.sel.with_a(p.pos, d);
        }
        if ui.button("設為 B 點").on_hover_text("把目前位置設為終點（快速鍵 O）").clicked() {
            p.sel = p.sel.with_b(p.pos, d);
        }
        if ui.button("全選").on_hover_text("範圍改回整段影片").clicked() {
            p.sel = video::Selection::full(d);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| volume_ui(ui, volume, muted));
    });
    ui.add_space(4.0);
}

/// 音量：靜音按鈕＋滑桿（版面由右往左排，所以先放滑桿）
fn volume_ui(ui: &mut egui::Ui, volume: &mut u8, muted: &mut bool) {
    ui.spacing_mut().slider_width = 110.0;
    let slider = ui.add(egui::Slider::new(volume, 0..=100).suffix("%").show_value(true));
    if slider.changed() {
        *muted = false;
    }
    let icon = if *muted || *volume == 0 { "🔇" } else { "🔊" };
    if ui.button(icon).on_hover_text("靜音（快速鍵 M）").clicked() {
        *muted = !*muted;
    }
}

fn selection_ui(ui: &mut egui::Ui, p: &mut Player) {
    let s = p.sel;
    ui.horizontal(|ui| {
        let accent = theme::accent(ui);
        ui.label(RichText::new(format!("A {}", format_time(s.a))).color(accent).monospace());
        ui.label("→");
        ui.label(RichText::new(format!("B {}", format_time(s.b))).color(accent).monospace());
        ui.weak(format!("共 {:.2} 秒", s.span()));
    });
    ui.weak("拖曳時間軸下方軌道可選範圍，A、B 端點可個別拖動；拖上方細條可移動播放位置。");
}

/// 空白鍵播放／暫停，I／O 設 A／B 點，← → 前後 1 秒，M 靜音
fn shortcuts(ui: &egui::Ui, p: &mut Player, ctx: &egui::Context, muted: &mut bool) {
    if ctx.egui_wants_keyboard_input() || p.info.is_none() {
        return;
    }
    let (space, i, o, left, right, m) = ui.input(|inp| {
        (
            inp.key_pressed(egui::Key::Space),
            inp.key_pressed(egui::Key::I),
            inp.key_pressed(egui::Key::O),
            inp.key_pressed(egui::Key::ArrowLeft),
            inp.key_pressed(egui::Key::ArrowRight),
            inp.key_pressed(egui::Key::M),
        )
    });
    if m {
        *muted = !*muted;
    }
    let d = p.duration();
    if space {
        p.toggle_play(ctx);
    }
    if i {
        p.sel = p.sel.with_a(p.pos, d);
    }
    if o {
        p.sel = p.sel.with_b(p.pos, d);
    }
    if left || right {
        let t = p.pos + if right { 1.0 } else { -1.0 };
        p.seek(t, ctx);
    }
}

fn video_empty_ui(ui: &mut egui::Ui, icons: &crate::icons::Icons) {
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    let color = if hovering { theme::accent(ui) } else { ui.visuals().weak_text_color() };
    let size = ui.available_size() - egui::Vec2::splat(4.0);
    egui::Frame::NONE.stroke(egui::Stroke::new(if hovering { 2.0 } else { 1.0 }, color)).corner_radius(12).show(ui, |ui| {
        ui.set_min_size(size);
        ui.vertical_centered(|ui| {
            ui.add_space((size.y * 0.5 - 80.0).max(8.0));
            ui.add(img(&icons.movie, 96.0));
            ui.add_space(6.0);
            ui.label(RichText::new("把 MP4 影片拖曳到這裡").size(17.0).strong());
            ui.weak("或使用左側的「選擇 MP4 影片」；選好範圍後按上方「開始轉換」");
        });
    });
}

/// 拖放進來的檔案裡第一個影片
pub fn first_video(paths: &[PathBuf]) -> Option<&PathBuf> {
    paths.iter().find(|p| video::is_video_file(p))
}
