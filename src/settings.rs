//! 需要跨次啟動記住的設定：左側設定卡片的排列順序、輸出位置。
//! 存成 UTF-8 純文字的 key=value 檔，不額外依賴序列化套件。
//! Windows：%APPDATA%\ImgConvert\settings.txt；其他系統：$XDG_CONFIG_HOME（或 ~/.config）/imgconvert/settings.txt

use std::path::PathBuf;

const FILE_NAME: &str = "settings.txt";
const KEY_ORDER: &str = "card_order";
const KEY_SAME_DIR: &str = "same_dir";
const KEY_OUT_DIR: &str = "out_dir";

/// 左側「輸出設定」裡可以拖曳排序的卡片
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardKind {
    Format,
    Location,
    Exist,
    Rotate,
}

pub const CARD_COUNT: usize = 4;
pub const DEFAULT_ORDER: [CardKind; CARD_COUNT] = [CardKind::Format, CardKind::Location, CardKind::Exist, CardKind::Rotate];

impl CardKind {
    fn key(self) -> &'static str {
        match self {
            CardKind::Format => "format",
            CardKind::Location => "location",
            CardKind::Exist => "exist",
            CardKind::Rotate => "rotate",
        }
    }

    fn from_key(s: &str) -> Option<Self> {
        DEFAULT_ORDER.into_iter().find(|k| k.key() == s)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    pub card_order: [CardKind; CARD_COUNT],
    /// true = 與原檔相同資料夾；預設 false（指定資料夾）
    pub same_dir: bool,
    pub out_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { card_order: DEFAULT_ORDER, same_dir: false, out_dir: None }
    }
}

impl Settings {
    pub fn to_text(&self) -> String {
        let order: Vec<&str> = self.card_order.iter().map(|k| k.key()).collect();
        let mut s = format!("{KEY_ORDER}={}\n{KEY_SAME_DIR}={}\n", order.join(","), self.same_dir);
        if let Some(d) = &self.out_dir {
            s.push_str(&format!("{KEY_OUT_DIR}={}\n", d.to_string_lossy()));
        }
        s
    }

    /// 寬鬆解析：不認得的行略過，壞掉的值用預設值
    pub fn from_text(text: &str) -> Self {
        let base = Self::default();
        text.lines().filter_map(|l| l.split_once('=')).fold(base, |acc, (k, v)| match k.trim() {
            KEY_ORDER => Self { card_order: parse_order(v), ..acc },
            KEY_SAME_DIR => Self { same_dir: v.trim() == "true", ..acc },
            KEY_OUT_DIR if !v.trim().is_empty() => Self { out_dir: Some(PathBuf::from(v.trim())), ..acc },
            _ => acc,
        })
    }

    pub fn load() -> Self {
        file_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Self::from_text(&t)).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = file_path().ok_or("找不到使用者設定資料夾")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("無法建立 {}：{e}", dir.display()))?;
        }
        std::fs::write(&path, self.to_text()).map_err(|e| format!("無法寫入 {}：{e}", path.display()))
    }
}

/// 解析順序；重複的去掉、缺少的依預設順序補在後面，保證四張卡片都在
pub fn parse_order(s: &str) -> [CardKind; CARD_COUNT] {
    let listed = s.split(',').filter_map(|p| CardKind::from_key(p.trim()));
    let mut seen: Vec<CardKind> = Vec::with_capacity(CARD_COUNT);
    for k in listed.chain(DEFAULT_ORDER) {
        if !seen.contains(&k) {
            seen.push(k);
        }
    }
    seen.try_into().unwrap_or(DEFAULT_ORDER)
}

/// 把 `card` 移到 `insert_at`（0..=CARD_COUNT，指「插在第幾張卡片之前」），回傳新的順序
pub fn move_card(order: [CardKind; CARD_COUNT], card: CardKind, insert_at: usize) -> [CardKind; CARD_COUNT] {
    let Some(from) = order.iter().position(|&k| k == card) else { return order };
    let mut rest: Vec<CardKind> = order.into_iter().filter(|&k| k != card).collect();
    // 拿掉自己之後，原本在它後面的位置都往前一格
    let at = if insert_at > from { insert_at - 1 } else { insert_at }.min(rest.len());
    rest.insert(at, card);
    rest.try_into().unwrap_or(order)
}

fn file_path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("ImgConvert")
    } else {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        base.join("imgconvert")
    };
    Some(dir.join(FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::CardKind::*;
    use super::*;

    #[test]
    fn default_is_custom_folder() {
        let s = Settings::default();
        assert!(!s.same_dir);
        assert_eq!(s.out_dir, None);
        assert_eq!(s.card_order, DEFAULT_ORDER);
    }

    #[test]
    fn roundtrip_with_chinese_path() {
        let s = Settings {
            card_order: [Rotate, Format, Exist, Location],
            same_dir: true,
            out_dir: Some(PathBuf::from("D:").join("我的 圖片").join("輸出")),
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
    }

    #[test]
    fn empty_or_garbage_falls_back_to_default() {
        assert_eq!(Settings::from_text(""), Settings::default());
        assert_eq!(Settings::from_text("亂碼\nsame_dir=maybe\nfoo=bar"), Settings::default());
    }

    #[test]
    fn parse_order_dedups_and_fills_missing() {
        assert_eq!(parse_order("exist,exist,bogus"), [Exist, Format, Location, Rotate]);
        assert_eq!(parse_order(""), DEFAULT_ORDER);
    }

    #[test]
    fn move_card_down_and_up() {
        // 第一張拖到第三張之前
        assert_eq!(move_card(DEFAULT_ORDER, Format, 2), [Location, Format, Exist, Rotate]);
        // 拖到最後
        assert_eq!(move_card(DEFAULT_ORDER, Format, 4), [Location, Exist, Rotate, Format]);
        // 最後一張拖到最前
        assert_eq!(move_card(DEFAULT_ORDER, Rotate, 0), [Rotate, Format, Location, Exist]);
        // 放回原位不變
        assert_eq!(move_card(DEFAULT_ORDER, Exist, 2), DEFAULT_ORDER);
        assert_eq!(move_card(DEFAULT_ORDER, Exist, 3), DEFAULT_ORDER);
    }
}
