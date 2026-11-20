//! 主視窗版面（參考 Windows 11 設定頁）：
//! 上方標題列、左側設定欄（設定卡片）、中間檔案清單、右側預覽、下方動作列。

use crate::convert::{self, ExistPolicy, OutFormat, Rotation};
use crate::icons::{Icons, img};
use crate::settings::{self, CardKind};
use crate::{App, Item, Status, help, theme};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

const SIDEBAR_WIDTH: f32 = 400.0;

/// 清單列上的操作
enum RowAction {
    None,
    Select,
    Remove,
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        self.poll_job();
        let running = self.running();

        // 拖放檔案／資料夾
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if !running {
            for p in dropped {
                self.add_path(&p);
            }
        }

        let panel = egui::Frame::side_top_panel(root.style()).inner_margin(egui::Margin::symmetric(20, 12));
        egui::Panel::top("header").frame(panel).show(root, |ui| {
            self.header_ui(ui, running);
            ui.add_space(6.0);
            self.progress_ui(ui);
        });

        let side = egui::Frame::side_top_panel(root.style()).inner_margin(egui::Margin::symmetric(16, 8));
        egui::Panel::left("settings").frame(side).resizable(false).exact_size(SIDEBAR_WIDTH).show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(SIDEBAR_WIDTH - 40.0);
                self.sources_ui(ui, running);
                ui.add_enabled_ui(!running, |ui| self.settings_ui(ui));
            });
        });
        egui::Panel::right("preview").frame(side).resizable(true).default_size(320.0).show(root, |ui| {
            self.preview_ui(ui);
        });

        let central = egui::Frame::central_panel(root.style()).inner_margin(egui::Margin::symmetric(8, 8));
        egui::CentralPanel::default().frame(central).show(root, |ui| self.list_ui(ui, running));

        help::show(ctx, &self.icons, &mut self.show_help);
        self.persist_settings();
    }
}

impl App {
    /// 標題列分三欄：左邊 App 名稱、正中間「開始轉換」、右邊版本與說明
    fn header_ui(&mut self, ui: &mut egui::Ui, running: bool) {
        ui.columns(3, |cols| {
            cols[0].horizontal(|ui| {
                ui.add(img(&self.icons.logo, 36.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("ImgConvert 圖片轉檔器").size(19.0).strong());
                    ui.weak("AVIF / WebP 批次轉成 PNG（無損）或 JPG");
                });
            });
            cols[1].vertical_centered(|ui| self.start_button_ui(ui, running));
            // 先用 horizontal 固定成一列的高度，再由右往左排，避免撐高整個標題列
            cols[2].horizontal(|ui| {
                ui.set_min_height(40.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let hint = format!("操作說明與版本資訊（v{}）", help::VERSION);
                    if theme::icon_button(ui, &self.icons.info, "說明").on_hover_text(hint).clicked() {
                        self.show_help = true;
                    }
                    ui.weak(format!("v{}", help::VERSION));
                });
            });
        });
    }

    fn start_button_ui(&mut self, ui: &mut egui::Ui, running: bool) {
        if running {
            if theme::secondary_button(ui, Some(&self.icons.stop), "取消").clicked() {
                if let Some(j) = &self.job {
                    j.cancel.store(true, Ordering::Relaxed);
                }
            }
        } else if theme::primary_button(ui, Some(&self.icons.play), "開始轉換", !self.items.is_empty()).clicked() {
            self.start(ui.ctx());
        }
    }

    fn sources_ui(&mut self, ui: &mut egui::Ui, running: bool) {
        theme::section_label(ui, "加入圖片");
        ui.add_enabled_ui(!running, |ui| {
            let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            ui.horizontal(|ui| {
                if theme::tile(ui, &self.icons.folder, "選擇資料夾", w, egui::Sense::click()).clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.add_path(&d);
                    }
                }
                if theme::tile(ui, &self.icons.picture, "加入檔案", w, egui::Sense::click()).clicked() {
                    let filter = rfd::FileDialog::new().add_filter("AVIF / WebP", &convert::INPUT_EXTS);
                    for f in filter.pick_files().unwrap_or_default() {
                        self.add_path(&f);
                    }
                }
            });
            ui.add_space(2.0);
            theme::toggle(ui, &mut self.recursive, "包含子資料夾");
        });
    }

    /// 四張設定卡片依 `card_order` 排列；按住右上角把手可拖曳調整順序
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "輸出設定");
        let ctx = ui.ctx().clone();
        let dragging = egui::DragAndDrop::payload::<CardKind>(&ctx).map(|p| *p);
        if dragging.is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        let pointer = ctx.pointer_interact_pos();
        let released = ctx.input(|i| i.pointer.any_released());
        let gap = ui.spacing().item_spacing.y;
        let mut drop_at: Option<usize> = None;
        for (idx, kind) in self.card_order.into_iter().enumerate() {
            let (rect, grip) = self.card_ui(ui, kind, dragging == Some(kind));
            grip.dnd_set_drag_payload(kind);
            // 指標落在這張卡片（含下方間距）時，依上半／下半決定插在前面或後面
            let zone = rect.expand2(egui::vec2(0.0, gap * 0.5));
            if let (Some(_), Some(pos)) = (dragging, pointer.filter(|p| zone.contains(*p))) {
                let after = pos.y > rect.center().y;
                let y = if after { rect.bottom() + gap * 0.5 } else { rect.top() - gap * 0.5 };
                theme::drop_indicator(ui, rect.left(), rect.right(), y);
                drop_at = Some(idx + usize::from(after));
            }
        }
        if released && let Some(kind) = egui::DragAndDrop::take_payload::<CardKind>(&ctx) {
            if let Some(at) = drop_at {
                self.card_order = settings::move_card(self.card_order, *kind, at);
            }
        }
    }

    fn card_ui(&mut self, ui: &mut egui::Ui, kind: CardKind, dimmed: bool) -> (egui::Rect, egui::Response) {
        let App { icons, opts, same_dir, out_dir, global_rot, items, .. } = self;
        let grip_id = egui::Id::new(("settings_card_grip", kind as u8));
        let card = |ui: &mut egui::Ui, icon, title, desc: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            theme::draggable_settings_card(ui, grip_id, dimmed, icon, title, desc, |ui| add(ui))
        };
        match kind {
            CardKind::Format => card(ui, &icons.palette, "輸出格式", "PNG 逐像素無損並保留透明；JPG 檔案較小但有損", &mut |ui| {
                theme::segmented(ui, &mut opts.format, &[(OutFormat::Png, "PNG（無損）"), (OutFormat::Jpg, "JPG（有損壓縮）")]);
                if opts.format == OutFormat::Jpg {
                    ui.add(egui::Slider::new(&mut opts.jpg_quality, 1..=100).text("品質"));
                }
            }),
            CardKind::Location => {
                let desc = match (*same_dir, out_dir.is_some()) {
                    (true, _) => "轉好的檔案存在原檔旁邊".to_string(),
                    (false, true) => "存到指定資料夾，並保留子資料夾結構".to_string(),
                    (false, false) => format!("未選資料夾時，自動存到來源資料夾下的「{}」", convert::DEFAULT_SUBDIR),
                };
                card(ui, &icons.outbox, "輸出位置", &desc, &mut |ui| out_location_ui(ui, same_dir, out_dir))
            }
            CardKind::Exist => card(ui, &icons.dividers, "同名檔案", "輸出位置已有同名檔案時的處理方式", &mut |ui| {
                let choices = [(ExistPolicy::Rename, "自動改名"), (ExistPolicy::Overwrite, "覆蓋"), (ExistPolicy::Skip, "略過")];
                theme::segmented(ui, &mut opts.exist, &choices);
            }),
            CardKind::Rotate => card(ui, &icons.rotate, "全部旋轉", "套用到所有檔案；清單每列也能個別旋轉", &mut |ui| {
                rotation_ui(ui, global_rot, items);
            }),
        }
    }

    /// 標題列第二排：左邊狀態訊息，右邊進度條（尚未轉換時進度條為空）
    fn progress_ui(&self, ui: &mut egui::Ui) {
        let (frac, text) = match &self.job {
            Some(j) if j.total > 0 => {
                let d = j.done.load(Ordering::Relaxed);
                (d as f32 / j.total as f32, format!("{d} / {}（{:.0}%）", j.total, d as f32 * 100.0 / j.total as f32))
            }
            _ => (0.0, "尚未開始".to_string()),
        };
        ui.horizontal(|ui| {
            let msg_w = (ui.available_width() * 0.4).min(460.0);
            ui.allocate_ui(egui::vec2(msg_w, 20.0), |ui| {
                ui.set_width(msg_w);
                ui.add(egui::Label::new(&self.message).truncate());
            });
            let bar = egui::ProgressBar::new(frac).text(text).desired_height(20.0).corner_radius(10);
            ui.add(if frac > 0.0 { bar } else { bar.fill(egui::Color32::TRANSPARENT) });
        });
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, running: bool) {
        theme::card(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.label(RichText::new("檔案清單").strong().size(16.0));
                theme::chip(ui, &format!("{} 個", self.items.len()), theme::accent(ui));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let enabled = !running && !self.items.is_empty();
                    if ui.add_enabled_ui(enabled, |ui| theme::icon_button(ui, &self.icons.trash, "清除清單")).inner.clicked() {
                        self.items.clear();
                        self.selected = None;
                        self.preview = None;
                        self.job = None;
                    }
                });
            });
            ui.add_space(4.0);
            if self.items.is_empty() {
                empty_state_ui(ui, &self.icons);
            } else {
                self.rows_ui(ui, running);
            }
        });
    }

    fn rows_ui(&mut self, ui: &mut egui::Ui, running: bool) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let row_h = ui.spacing().interact_size.y + 4.0;
        let mut action: Option<(usize, RowAction)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, self.items.len(), |ui, range| {
            for i in range {
                let selected = self.selected == Some(i);
                match row_ui(ui, &self.icons, &mut self.items[i], i, selected, running) {
                    RowAction::None => {}
                    a => action = Some((i, a)),
                }
            }
        });
        match action {
            Some((i, RowAction::Remove)) => {
                self.items.remove(i);
                self.selected = None;
                self.preview = None;
            }
            Some((i, RowAction::Select)) => self.select(i, ui.ctx()),
            _ => {}
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "預覽");
        let Some(idx) = self.selected.filter(|&i| i < self.items.len()) else {
            placeholder_ui(ui, &self.icons.picture, "點選清單中的檔案即可預覽");
            return;
        };
        let rot = self.global_rot.add(self.items[idx].rot);
        let Some(pv) = &mut self.preview else { return };
        if let Some(rx) = &pv.rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(img) => pv.img = Some(img),
                    Err(e) => pv.err = Some(e),
                }
                pv.rx = None;
            }
        }
        if let Some(e) = &pv.err {
            ui.colored_label(theme::ERR, e);
            return;
        }
        let Some(image) = &pv.img else {
            ui.centered_and_justified(|ui| ui.spinner());
            return;
        };
        if pv.tex.as_ref().is_none_or(|(r, _)| *r != rot) {
            let rgba = rot.apply(image.clone()).to_rgba8();
            let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
            pv.tex = Some((rot, ui.ctx().load_texture("preview", ci, egui::TextureOptions::LINEAR)));
        }
        let Some((_, tex)) = pv.tex.as_ref() else { return };
        let name = pv.path.file_name().unwrap_or_default().to_string_lossy();
        ui.add(egui::Label::new(RichText::new(name.as_ref()).strong()).truncate());
        ui.weak(format!("輸出時順時針旋轉 {}", rot.label()));
        ui.add_space(4.0);
        let avail = ui.available_size();
        let size = tex.size_vec2();
        let scale = (avail.x / size.x).min(avail.y / size.y).min(1.0);
        ui.vertical_centered(|ui| ui.add(egui::Image::new(tex).fit_to_exact_size(size * scale).corner_radius(8)));
    }
}

fn out_location_ui(ui: &mut egui::Ui, same_dir: &mut bool, out_dir: &mut Option<PathBuf>) {
    theme::segmented(ui, same_dir, &[(true, "與原檔相同資料夾"), (false, "指定資料夾")]);
    if *same_dir {
        return;
    }
    ui.horizontal(|ui| {
        if ui.button("選擇資料夾…").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                *out_dir = Some(d);
            }
        }
        if out_dir.is_some() && ui.button("清除").on_hover_text("改回預設：來源資料夾下的「已轉換好」").clicked() {
            *out_dir = None;
        }
    });
    if let Some(d) = out_dir {
        let path = d.display().to_string();
        ui.add(egui::Label::new(RichText::new(&path).small()).truncate()).on_hover_text(path);
    }
}

fn rotation_ui(ui: &mut egui::Ui, global_rot: &mut Rotation, items: &mut [Item]) {
    ui.horizontal(|ui| {
        if ui.button("↺ 左轉 90°").clicked() {
            *global_rot = global_rot.ccw();
        }
        if ui.button("↻ 右轉 90°").clicked() {
            *global_rot = global_rot.cw();
        }
        if ui.button("重設").on_hover_text("全部和每個檔案的旋轉都歸零").clicked() {
            *global_rot = Rotation::R0;
            for it in items.iter_mut() {
                it.rot = Rotation::R0;
            }
        }
    });
    ui.weak(format!("目前：順時針 {}", global_rot.label()));
}

/// 狀態標籤；完成／失敗前面加上 3D 小圖示（版面為由右往左排）
fn status_chip(ui: &mut egui::Ui, icons: &Icons, status: &Status) -> egui::Response {
    let resp = match status {
        Status::Pending => theme::chip(ui, "待轉換", ui.visuals().weak_text_color()),
        Status::Done(p) => theme::chip(ui, "完成", theme::OK).on_hover_text(format!("→ {}", p.display())),
        Status::Skipped => theme::chip(ui, "略過", egui::Color32::GRAY),
        Status::Failed(e) => theme::chip(ui, "失敗", theme::ERR).on_hover_text(e),
    };
    match status {
        Status::Done(_) => _ = ui.add(img(&icons.check, 18.0)),
        Status::Failed(_) => _ = ui.add(img(&icons.cross, 18.0)),
        _ => {}
    }
    resp
}

fn row_ui(ui: &mut egui::Ui, icons: &Icons, it: &mut Item, i: usize, selected: bool, running: bool) -> RowAction {
    let mut action = RowAction::None;
    let fill = if i % 2 == 1 { ui.visuals().faint_bg_color } else { egui::Color32::TRANSPARENT };
    egui::Frame::NONE.fill(fill).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            ui.add_enabled_ui(!running, |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if ui.small_button("↺").on_hover_text("此檔左轉 90°").clicked() {
                    it.rot = it.rot.ccw();
                }
                if ui.small_button("↻").on_hover_text("此檔右轉 90°").clicked() {
                    it.rot = it.rot.cw();
                }
            });
            let name = it.path.file_name().unwrap_or_default().to_string_lossy();
            if ui.selectable_label(selected, name.as_ref()).on_hover_text(it.path.display().to_string()).clicked() {
                action = RowAction::Select;
            }
            if it.rot != Rotation::R0 {
                theme::chip(ui, &format!("↻ {}", it.rot.label()), theme::accent(ui));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = ui.add_enabled(!running, egui::Button::new("✖").small().frame(false));
                if remove.on_hover_text("從清單移除").clicked() {
                    action = RowAction::Remove;
                }
                status_chip(ui, icons, &it.status);
            });
        });
    });
    action
}

/// 置中的大圖示＋提示文字
fn placeholder_ui(ui: &mut egui::Ui, icon: &egui::TextureHandle, text: &str) {
    let h = ui.available_height();
    ui.vertical_centered(|ui| {
        ui.add_space((h * 0.5 - 60.0).max(8.0));
        ui.add(img(icon, 72.0).tint(egui::Color32::from_white_alpha(170)));
        ui.add_space(4.0);
        ui.weak(text);
    });
}

/// 清單空白時的拖放提示區；拖著檔案經過視窗時會以強調色標示
fn empty_state_ui(ui: &mut egui::Ui, icons: &Icons) {
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    let color = if hovering { theme::accent(ui) } else { ui.visuals().weak_text_color() };
    let size = ui.available_size() - egui::Vec2::splat(4.0);
    let fill = if hovering { theme::accent(ui).gamma_multiply(0.08) } else { egui::Color32::TRANSPARENT };
    egui::Frame::NONE
        .fill(fill)
        .stroke(egui::Stroke::new(if hovering { 2.0 } else { 1.0 }, color))
        .corner_radius(12)
        .show(ui, |ui| {
            ui.set_min_size(size);
            ui.vertical_centered(|ui| {
                ui.add_space((size.y * 0.5 - 80.0).max(8.0));
                ui.add(img(&icons.inbox, 96.0));
                ui.add_space(6.0);
                ui.label(RichText::new("把 AVIF / WebP 檔案或資料夾拖曳到這裡").size(17.0).strong());
                ui.weak("或使用左側的「選擇資料夾」「加入檔案」");
            });
        });
}
