//! 時間軸元件：上方細條拖曳＝移動播放位置；下方軌道拖曳＝選範圍，A／B 端點可各自拖動微調；點一下＝跳到該時間。

use crate::theme;
use crate::video::Selection;
use crate::waveform::{self, Bin};
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};

const HEIGHT: f32 = 100.0;
/// 波形每一欄的寬度（像素）
const WAVE_STEP: f32 = 2.0;
/// 選取範圍內的波形顏色：峰值（橘）、平均音量（亮黃）
const WAVE_PEAK: Color32 = Color32::from_rgb(232, 150, 40);
const WAVE_RMS: Color32 = Color32::from_rgb(255, 205, 110);

/// 軌道裡要畫什麼：音量波形，或一行說明文字（分析中、沒有聲音…）
pub enum WaveView<'a> {
    Bins(&'a [Bin]),
    Message(&'a str),
}
/// 上方播放位置拖曳區的高度
const RULER_H: f32 = 16.0;
/// 左右留白，讓 A／B 把手在最邊緣也點得到
const PAD: f32 = 10.0;
/// 抓 A／B 把手的容許距離（像素）
const GRAB_TOL: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragKind {
    Playhead,
    A,
    B,
    /// 從這個時間開始拖出新範圍
    NewRange(f64),
}

/// 時間軸這一幀的結果：呼叫端據此更新播放器
#[derive(Default)]
pub struct Output {
    pub seek: Option<f64>,
    pub sel: Option<Selection>,
}

pub fn time_at(x: f32, left: f32, right: f32, duration: f64) -> f64 {
    let w = (right - left).max(1.0);
    (((x - left) / w).clamp(0.0, 1.0) as f64) * duration
}

pub fn x_at(t: f64, left: f32, right: f32, duration: f64) -> f32 {
    if duration <= 0.0 {
        return left;
    }
    left + (right - left) * (t / duration).clamp(0.0, 1.0) as f32
}

/// 按下的位置要拖什麼：上方細條拖播放位置；靠近 A／B 拖那個端點（取較近者）；其他位置拖出新範圍
pub fn pick(x: f32, in_ruler: bool, ax: f32, bx: f32, t: f64) -> DragKind {
    if in_ruler {
        return DragKind::Playhead;
    }
    let (da, db) = ((x - ax).abs(), (x - bx).abs());
    match (da <= GRAB_TOL, db <= GRAB_TOL) {
        // A、B 重疊時，依按在哪一邊決定，往外拖才拉得開
        (true, true) if (da - db).abs() < f32::EPSILON => {
            if x < ax { DragKind::A } else { DragKind::B }
        }
        (true, true) => if da < db { DragKind::A } else { DragKind::B },
        (true, false) => DragKind::A,
        (false, true) => DragKind::B,
        _ => DragKind::NewRange(t),
    }
}

pub fn show(ui: &mut egui::Ui, duration: f64, pos: f64, sel: Selection, wave: WaveView) -> Output {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), egui::Sense::click_and_drag());
    let (left, right) = (rect.left() + PAD, rect.right() - PAD);
    let (ax, bx) = (x_at(sel.a, left, right, duration), x_at(sel.b, left, right, duration));
    let id = resp.id.with("drag_kind");
    let mut out = Output::default();

    if let Some(p) = resp.interact_pointer_pos() {
        let t = time_at(p.x, left, right, duration);
        if resp.drag_started() {
            let kind = pick(p.x, p.y < rect.top() + RULER_H, ax, bx, t);
            ui.data_mut(|d| d.insert_temp(id, kind));
        }
        let kind = ui.data(|d| d.get_temp::<DragKind>(id));
        if resp.dragged() {
            match kind {
                Some(DragKind::A) => out.sel = Some(sel.with_a(t, duration)),
                Some(DragKind::B) => out.sel = Some(sel.with_b(t, duration)),
                Some(DragKind::NewRange(t0)) => out.sel = Some(Selection::from_drag(t0, t, duration)),
                Some(DragKind::Playhead) | None => {}
            }
            // 拖到哪就顯示哪一格，方便對準畫面
            out.seek = Some(match out.sel {
                Some(s) if kind == Some(DragKind::A) => s.a,
                Some(s) if kind == Some(DragKind::B) => s.b,
                _ => t,
            });
        } else if resp.clicked() {
            out.seek = Some(t);
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<DragKind>(id));
    }

    let hover_kind = resp.hover_pos().map(|p| pick(p.x, p.y < rect.top() + RULER_H, ax, bx, 0.0));
    let cursor = match hover_kind {
        Some(DragKind::A | DragKind::B) => egui::CursorIcon::ResizeHorizontal,
        Some(_) => egui::CursorIcon::PointingHand,
        None => egui::CursorIcon::Default,
    };
    let resp = resp.on_hover_cursor(cursor);
    if let Some(p) = resp.hover_pos() {
        resp.on_hover_text_at_pointer(crate::video::format_time(time_at(p.x, left, right, duration)));
    }

    paint(ui, rect, left, right, ax, bx, x_at(pos, left, right, duration), &wave, duration);
    out
}

#[allow(clippy::too_many_arguments)]
fn paint(ui: &egui::Ui, rect: Rect, left: f32, right: f32, ax: f32, bx: f32, px: f32, wave: &WaveView, duration: f64) {
    let p = ui.painter();
    let accent = theme::accent(ui);
    let v = ui.visuals();
    // 上方播放位置細條
    let ruler = Rect::from_x_y_ranges(left..=right, (rect.top() + 6.0)..=(rect.top() + 10.0));
    p.rect_filled(ruler, CornerRadius::same(2), v.widgets.inactive.bg_fill);
    // 下方軌道與選取範圍
    let track = Rect::from_x_y_ranges(left..=right, (rect.top() + RULER_H + 4.0)..=(rect.bottom() - 4.0));
    p.rect_filled(track, CornerRadius::same(6), v.widgets.inactive.weak_bg_fill);
    let range = Rect::from_x_y_ranges(ax..=bx, track.y_range());
    p.rect_filled(range, CornerRadius::ZERO, accent.gamma_multiply(0.25));
    match wave {
        WaveView::Bins(bins) => paint_wave(ui, track, bins, duration, ax..=bx),
        WaveView::Message(text) => {
            let font = egui::FontId::proportional(12.0);
            p.text(track.center(), egui::Align2::CENTER_CENTER, *text, font, v.weak_text_color());
        }
    }
    for (x, label) in [(ax, "A"), (bx, "B")] {
        handle(ui, x, track, label, accent);
    }
    // 播放位置：一條線貫穿上下，頂端一個三角形
    let line = Stroke::new(2.0, if v.dark_mode { Color32::WHITE } else { Color32::BLACK });
    p.line_segment([Pos2::new(px, rect.top() + 2.0), Pos2::new(px, rect.bottom() - 2.0)], line);
    let tri = vec![Pos2::new(px - 6.0, rect.top()), Pos2::new(px + 6.0, rect.top()), Pos2::new(px, rect.top() + 8.0)];
    p.add(egui::Shape::convex_polygon(tri, line.color, Stroke::NONE));
}

/// 音量波形：每 2 像素一欄，上下對稱；外圈是峰值，內圈是平均音量。
/// 以整條音軌最大峰值當滿格，小聲的影片也看得清楚。範圍外用灰色。
fn paint_wave(ui: &egui::Ui, track: Rect, bins: &[Bin], duration: f64, range: std::ops::RangeInclusive<f32>) {
    let p = ui.painter();
    let max = bins.iter().fold(0.0f32, |m, b| m.max(b.peak)).max(0.01);
    let half = track.height() * 0.5 - 3.0;
    let mid = track.center().y;
    let dim = ui.visuals().weak_text_color();
    let mut x = track.left();
    while x < track.right() {
        let t0 = time_at(x, track.left(), track.right(), duration);
        let t1 = time_at(x + WAVE_STEP, track.left(), track.right(), duration);
        let b = waveform::column(bins, t0, t1);
        let inside = range.contains(&x);
        let (peak_c, rms_c) = if inside { (WAVE_PEAK, WAVE_RMS) } else { (dim.gamma_multiply(0.6), dim) };
        let (hp, hr) = ((b.peak / max).min(1.0) * half, (b.rms / max).min(1.0) * half);
        let cx = x + WAVE_STEP * 0.5;
        p.line_segment([Pos2::new(cx, mid - hp), Pos2::new(cx, mid + hp.max(0.5))], Stroke::new(WAVE_STEP - 0.5, peak_c));
        p.line_segment([Pos2::new(cx, mid - hr), Pos2::new(cx, mid + hr)], Stroke::new(WAVE_STEP - 0.5, rms_c));
        x += WAVE_STEP;
    }
}

fn handle(ui: &egui::Ui, x: f32, track: Rect, label: &str, color: Color32) {
    let p = ui.painter();
    let bar = Rect::from_center_size(Pos2::new(x, track.center().y), Vec2::new(6.0, track.height() + 4.0));
    p.rect_filled(bar, CornerRadius::same(3), color);
    let font = egui::FontId::proportional(11.0);
    p.text(Pos2::new(x, track.center().y), egui::Align2::CENTER_CENTER, label, font, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_and_x_roundtrip_and_clamp() {
        assert_eq!(time_at(60.0, 10.0, 110.0, 20.0), 10.0);
        assert_eq!(time_at(-50.0, 10.0, 110.0, 20.0), 0.0);
        assert_eq!(time_at(500.0, 10.0, 110.0, 20.0), 20.0);
        assert_eq!(x_at(10.0, 10.0, 110.0, 20.0), 60.0);
        assert_eq!(x_at(5.0, 10.0, 110.0, 0.0), 10.0, "長度 0 不除以零");
    }

    #[test]
    fn pick_targets() {
        assert_eq!(pick(50.0, true, 50.0, 80.0, 1.0), DragKind::Playhead);
        assert_eq!(pick(53.0, false, 50.0, 80.0, 1.0), DragKind::A);
        assert_eq!(pick(76.0, false, 50.0, 80.0, 1.0), DragKind::B);
        assert_eq!(pick(65.0, false, 50.0, 80.0, 2.5), DragKind::NewRange(2.5));
        // 很靠近時取較近的一端
        assert_eq!(pick(56.0, false, 50.0, 60.0, 0.0), DragKind::B);
        // 完全重疊：按左邊拉 A、按右邊拉 B
        assert_eq!(pick(47.0, false, 50.0, 50.0, 0.0), DragKind::A);
        assert_eq!(pick(53.0, false, 50.0, 50.0, 0.0), DragKind::B);
    }
}
