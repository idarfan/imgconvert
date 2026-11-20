//! 「說明」對話框：快速上手、操作說明與版本資訊。版本號自動取自 Cargo.toml。

use crate::icons::{Icons, img};
use crate::theme;
use eframe::egui::{self, RichText};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHOR_EMAIL: &str = "mr.idarfan@gmail.com";
const WIDTH: f32 = 680.0;

const ADD: &[&str] = &[
    "按「選擇資料夾」加入整個資料夾；打開「包含子資料夾」會連子資料夾一起掃描。",
    "按「加入檔案」可一次選多個 AVIF / WebP 檔。",
    "也可以把檔案或資料夾直接拖曳到視窗，或拖到 ImgConvert.exe 圖示上開啟。",
];
const FORMAT: &[&str] = &[
    "PNG（無損）：逐像素保留原圖，包含透明背景；10/12-bit AVIF 會輸出 16-bit PNG。",
    "JPG（有損壓縮）：檔案較小但會損失畫質，可調整品質；透明處會補白底。",
];
const OUTPUT: &[&str] = &[
    "與原檔相同資料夾：轉好的檔案存在原檔旁邊。",
    "指定資料夾：存到你選的資料夾，並保留原本的子資料夾結構。",
    "選了「指定資料夾」但沒選路徑時，會自動在來源資料夾底下建立「已轉換好」並存入。",
    "預設為「指定資料夾」；輸出位置與選好的資料夾會自動記住，下次開啟沿用。",
    "同名檔案：自動改名會存成「名稱 (1).png」；也可以選擇覆蓋或略過。",
];
const LAYOUT: &[&str] = &[
    "左側「輸出設定」的四張卡片可以自由調整順序：按住卡片右上角的六點把手拖曳，藍線標示放下的位置。",
    "排好的順序會自動記住，下次開啟維持相同排列。",
];
const ROTATE: &[&str] = &[
    "「全部旋轉」套用到所有檔案；清單每列的 ↺ ↻ 只轉那一個檔案，兩者會相加。",
    "點選檔名可以在右側預覽旋轉後的樣子。",
];
const RUN: &[&str] = &[
    "按視窗上方正中間的「開始轉換」，轉換中同一個位置會變成「取消」。",
    "標題列下方那一排會顯示轉換進度（已完成 / 總數與百分比）。",
    "每列右側標籤會顯示待轉換、完成、略過或失敗；滑鼠移到「失敗」上可看原因。",
];
const LIMITS: &[&str] = &["動態 AVIF / WebP 只會轉第一格。", "未嵌入 ICC 色彩描述檔，廣色域圖的顏色可能偏淡。"];

/// 顯示說明對話框；按「關閉」、Esc 或點對話框外面會關閉
pub fn show(ctx: &egui::Context, icons: &Icons, open: &mut bool) {
    if !*open {
        return;
    }
    let mut close = false;
    let resp = egui::Modal::new(egui::Id::unique("imgconvert_help")).show(ctx, |ui| {
        ui.set_width(WIDTH);
        header_ui(ui, icons);
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
            ui.set_width(WIDTH - 16.0); // 留出捲軸的位置，文字不會被蓋住
            quick_start_ui(ui, icons);
            for (icon, title, lines) in [
                (&icons.folder, "加入檔案", ADD),
                (&icons.palette, "輸出格式", FORMAT),
                (&icons.outbox, "輸出位置與同名檔案", OUTPUT),
                (&icons.rotate, "旋轉與預覽", ROTATE),
                (&icons.gear, "調整卡片順序", LAYOUT),
                (&icons.play, "開始轉換", RUN),
                (&icons.bulb, "已知限制", LIMITS),
            ] {
                section_ui(ui, icon, title, lines);
            }
        });
        ui.add_space(8.0);
        close = footer_ui(ui);
    });
    if close || resp.should_close() {
        *open = false;
    }
}

fn header_ui(ui: &mut egui::Ui, icons: &Icons) {
    ui.horizontal(|ui| {
        ui.add(img(&icons.logo, 48.0));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new("ImgConvert 圖片轉檔器").size(20.0).strong());
                theme::chip(ui, &format!("v{VERSION}"), theme::accent(ui));
            });
            ui.weak("把 AVIF / WebP 批次轉成 PNG（無損）或 JPG");
        });
    });
}

fn quick_start_ui(ui: &mut egui::Ui, icons: &Icons) {
    theme::section_label(ui, "快速上手");
    let steps = [(&icons.folder, "1. 加入檔案"), (&icons.gear, "2. 選擇輸出設定"), (&icons.play, "3. 開始轉換")];
    let gap = ui.spacing().item_spacing.x;
    let w = (ui.available_width() - gap * 2.0) / 3.0;
    ui.horizontal(|ui| {
        for (icon, text) in steps {
            theme::tile(ui, icon, text, w, egui::Sense::hover());
        }
    });
    ui.add_space(6.0);
}

fn section_ui(ui: &mut egui::Ui, icon: &egui::TextureHandle, title: &str, lines: &[&str]) {
    ui.add_space(4.0);
    theme::card(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.add(img(icon, 36.0));
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(RichText::new(title).strong().size(16.0));
                for line in lines {
                    // 圓點與文字分開放，換行時文字對齊（懸掛縮排）
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new("•").color(theme::accent(ui)));
                        ui.add(egui::Label::new(*line).wrap());
                    });
                }
            });
        });
    });
}

fn footer_ui(ui: &mut egui::Ui) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.small(RichText::new("作者：").weak());
                ui.hyperlink_to(RichText::new(AUTHOR_EMAIL).small(), format!("mailto:{AUTHOR_EMAIL}"));
            });
            ui.small(RichText::new(format!("版本 {VERSION}　·　授權 MIT")).weak());
            ui.small(RichText::new("3D 圖示：Microsoft Fluent Emoji（MIT 授權）").weak());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = theme::primary_button(ui, None, "關閉", true).clicked();
        });
    });
    close
}
