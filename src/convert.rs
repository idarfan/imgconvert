//! 轉檔核心：解碼 AVIF/WebP → 旋轉 → 編碼 PNG/JPG。與 GUI 無關，可單獨測試。

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ImageEncoder, ImageReader, Rgb, RgbImage};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const INPUT_EXTS: [&str; 2] = ["avif", "webp"];

/// 選「指定資料夾」但沒選路徑時，自動建立在來源資料夾底下的子資料夾名稱
pub const DEFAULT_SUBDIR: &str = "已轉換好";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OutFormat {
    #[default]
    Png,
    Jpg,
}

impl OutFormat {
    pub fn ext(self) -> &'static str {
        match self {
            OutFormat::Png => "png",
            OutFormat::Jpg => "jpg",
        }
    }
}

/// 順時針旋轉角度（90 度倍數，像素層級無損）
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn cw(self) -> Self {
        match self {
            Rotation::R0 => Rotation::R90,
            Rotation::R90 => Rotation::R180,
            Rotation::R180 => Rotation::R270,
            Rotation::R270 => Rotation::R0,
        }
    }
    pub fn ccw(self) -> Self {
        self.cw().cw().cw()
    }
    /// 兩個旋轉相加
    pub fn add(self, other: Rotation) -> Self {
        let mut r = self;
        for _ in 0..other.steps() {
            r = r.cw();
        }
        r
    }
    pub fn steps(self) -> u8 {
        self as u8
    }
    pub fn label(self) -> &'static str {
        match self {
            Rotation::R0 => "0°",
            Rotation::R90 => "90°",
            Rotation::R180 => "180°",
            Rotation::R270 => "270°",
        }
    }
    pub fn apply(self, img: DynamicImage) -> DynamicImage {
        match self {
            Rotation::R0 => img,
            Rotation::R90 => img.rotate90(),
            Rotation::R180 => img.rotate180(),
            Rotation::R270 => img.rotate270(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExistPolicy {
    /// 自動改名：name (1).png
    #[default]
    Rename,
    Overwrite,
    Skip,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub format: OutFormat,
    /// JPG 品質 1–100
    pub jpg_quality: u8,
    /// None = 輸出到來源檔同資料夾
    pub out_dir: Option<PathBuf>,
    /// out_dir 為 None 時才生效：輸出到「來源根資料夾/sub_dir」（保留子資料夾結構）
    pub sub_dir: Option<String>,
    pub exist: ExistPolicy,
}

impl Default for Options {
    fn default() -> Self {
        Self { format: OutFormat::Png, jpg_quality: 95, out_dir: None, sub_dir: None, exist: ExistPolicy::Rename }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Written(PathBuf),
    Skipped(PathBuf),
}

pub fn is_input_file(p: &Path) -> bool {
    p.is_file()
        && p.extension()
            .and_then(|e| e.to_str())
            .map(|e| INPUT_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
            .unwrap_or(false)
}

/// 掃描資料夾，回傳 (根目錄, 檔案) 清單；根目錄用於保留子資料夾結構
pub fn scan(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let walker = WalkDir::new(dir).max_depth(if recursive { usize::MAX } else { 1 });
    let mut v: Vec<PathBuf> = walker
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| is_input_file(p))
        .collect();
    v.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    v
}

/// 自然排序：a_2 排在 a_10 之前（同檔案總管）
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n
                };
                let (na, nb) = (take(&mut ai), take(&mut bi));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn decode(path: &Path) -> Result<DynamicImage, String> {
    // 以內容判斷格式（副檔名錯誤也能讀），失敗時退回副檔名判斷
    let reader = ImageReader::open(path).map_err(|e| format!("無法開啟：{e}"))?;
    let reader = reader.with_guessed_format().map_err(|e| format!("無法讀取：{e}"))?;
    let mut reader = if reader.format().is_none() {
        ImageReader::open(path).map_err(|e| format!("無法開啟：{e}"))?
    } else {
        reader
    };
    reader.no_limits();
    // 套用檔案內的 EXIF 方向資訊（例如手機拍攝的 WebP），使輸出與看圖軟體一致
    let mut dec = reader.into_decoder().map_err(|e| format!("解碼失敗：{e}"))?;
    let orient = image::ImageDecoder::orientation(&mut dec).unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(|e| format!("解碼失敗：{e}"))?;
    img.apply_orientation(orient);
    Ok(img)
}

/// 計算輸出路徑。root = 使用者選的資料夾（保留其下子資料夾結構）
pub fn output_path(src: &Path, root: Option<&Path>, opts: &Options) -> PathBuf {
    let stem = src.file_stem().unwrap_or_default();
    let parent = src.parent().map(Path::to_path_buf).unwrap_or_default();
    let rel_parent = || {
        root.and_then(|r| src.parent()?.strip_prefix(r).ok())
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    let dir = match (&opts.out_dir, &opts.sub_dir) {
        (Some(out), _) => out.join(rel_parent()),
        (None, Some(sub)) => match root {
            Some(r) if src.starts_with(r) => r.join(sub).join(rel_parent()),
            _ => parent.join(sub),
        },
        (None, None) => parent,
    };
    let mut p = dir.join(stem);
    p.set_extension(opts.format.ext());
    p
}

fn unique_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let ext = p.extension().unwrap_or_default().to_string_lossy().into_owned();
    let dir = p.parent().unwrap_or(Path::new(""));
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}).{ext}")))
        .find(|c| !c.exists())
        .unwrap()
}

/// JPG 不支援透明：與白底合成後轉 8-bit RGB
fn flatten_to_rgb8(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = px[3] as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        out.put_pixel(x, y, Rgb([blend(px[0]), blend(px[1]), blend(px[2])]));
    }
    out
}

pub fn encode(img: &DynamicImage, dst: &Path, opts: &Options) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("無法建立資料夾：{e}"))?;
    }
    // 先寫暫存檔再改名，避免中斷時留下半個檔案
    let tmp = dst.with_extension(format!("{}.part", opts.format.ext()));
    let res = (|| -> Result<(), String> {
        let f = File::create(&tmp).map_err(|e| format!("無法寫入：{e}"))?;
        let mut w = BufWriter::new(f);
        match opts.format {
            OutFormat::Png => {
                // PNG 無損：保留原色深（10/12-bit AVIF 解出 16-bit 即存 16-bit）
                let enc = PngEncoder::new_with_quality(&mut w, CompressionType::Default, FilterType::Adaptive);
                img.write_with_encoder(enc).map_err(|e| format!("PNG 編碼失敗：{e}"))?;
            }
            OutFormat::Jpg => {
                let rgb = flatten_to_rgb8(img);
                let enc = JpegEncoder::new_with_quality(&mut w, opts.jpg_quality.clamp(1, 100));
                enc.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
                    .map_err(|e| format!("JPG 編碼失敗：{e}"))?;
            }
        }
        w.flush().map_err(|e| format!("寫入失敗：{e}"))?;
        Ok(())
    })();
    match res {
        Ok(()) => {
            if dst.exists() {
                let _ = fs::remove_file(dst);
            }
            fs::rename(&tmp, dst).map_err(|e| format!("改名失敗：{e}"))
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// 轉換單一檔案
pub fn convert_file(src: &Path, root: Option<&Path>, rot: Rotation, opts: &Options) -> Result<Outcome, String> {
    let mut dst = output_path(src, root, opts);
    if dst.exists() {
        match opts.exist {
            ExistPolicy::Skip => return Ok(Outcome::Skipped(dst)),
            ExistPolicy::Overwrite => {}
            ExistPolicy::Rename => dst = unique_path(&dst),
        }
    }
    let img = rot.apply(decode(src)?);
    encode(&img, &dst, opts)?;
    Ok(Outcome::Written(dst))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("imgconvert-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn webp_lossless_to_png_is_pixel_exact() {
        let src = fixtures().join("lossless.webp");
        let reference = image::open(fixtures().join("reference.png")).unwrap().to_rgba8();
        let out = tmpdir("webp");
        let opts = Options { out_dir: Some(out.clone()), ..Default::default() };
        let Outcome::Written(dst) = convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap() else { panic!() };
        let got = image::open(&dst).unwrap().to_rgba8();
        assert_eq!(got.dimensions(), reference.dimensions());
        assert!(got.as_raw() == reference.as_raw(), "像素必須完全相同");
    }

    #[test]
    fn webp_lossy_with_alpha_to_png_keeps_pixels_and_alpha() {
        // lossy_alpha.webp：有損 VP8 + ALPH 透明通道（網路上最常見的 WebP 形式）
        let src = fixtures().join("lossy_alpha.webp");
        let decoded = decode(&src).unwrap();
        assert!(decoded.color().has_alpha(), "須保留透明通道");
        let out = tmpdir("webp-lossy");
        let opts = Options { out_dir: Some(out), ..Default::default() };
        let Outcome::Written(dst) = convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap() else { panic!() };
        assert_eq!(dst.extension().unwrap(), "png");
        let got = image::open(&dst).unwrap();
        assert_eq!(got.color(), decoded.color());
        assert_eq!(got.as_bytes(), decoded.as_bytes(), "PNG 須與解碼結果逐位元相同");
        let rgba = got.to_rgba8();
        assert_eq!(rgba.get_pixel(0, 0)[3], 0, "角落應為透明");
        assert_eq!(rgba.get_pixel(32, 24)[3], 255, "中心應不透明");
    }

    #[test]
    fn avif_decodes_and_png_matches_decoder_output() {
        let src = fixtures().join("sample.avif");
        let decoded = decode(&src).unwrap();
        let out = tmpdir("avif");
        let opts = Options { out_dir: Some(out.clone()), ..Default::default() };
        let Outcome::Written(dst) = convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap() else { panic!() };
        let got = image::open(&dst).unwrap();
        assert_eq!(got.color(), decoded.color(), "色深/通道須保留");
        assert_eq!(got.as_bytes(), decoded.as_bytes(), "PNG 須與解碼結果逐位元相同");
    }

    #[test]
    fn avif_matches_independent_decoder() {
        // avifdec_ref.png 由 libavif 官方 avifdec 產生；YUV→RGB 實作不同允許 ±3
        let ours = decode(&fixtures().join("sample.avif")).unwrap().to_rgba8();
        let reference = image::open(fixtures().join("avifdec_ref.png")).unwrap().to_rgba8();
        assert_eq!(ours.dimensions(), reference.dimensions());
        let max = ours.as_raw().iter().zip(reference.as_raw()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(max <= 3, "max diff {max}");
    }

    #[test]
    fn avif_10bit_keeps_16bit_png() {
        let src = fixtures().join("sample10.avif");
        let decoded = decode(&src).unwrap();
        let out = tmpdir("avif10");
        let opts = Options { out_dir: Some(out), ..Default::default() };
        let Outcome::Written(dst) = convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap() else { panic!() };
        let got = image::open(&dst).unwrap();
        assert!(matches!(got.color(), image::ColorType::Rgb16 | image::ColorType::Rgba16), "got {:?}", got.color());
        assert_eq!(got.as_bytes(), decoded.as_bytes());
    }

    #[test]
    fn rotation_90_swaps_dimensions_and_pixels() {
        let src = fixtures().join("lossless.webp");
        let base = decode(&src).unwrap().to_rgba8();
        let out = tmpdir("rot");
        let opts = Options { out_dir: Some(out), ..Default::default() };
        let Outcome::Written(dst) = convert_file(&src, Some(&fixtures()), Rotation::R90, &opts).unwrap() else { panic!() };
        let got = image::open(&dst).unwrap().to_rgba8();
        let (w, h) = base.dimensions();
        assert_eq!(got.dimensions(), (h, w));
        // 順時針 90°：原 (0,0) → (h-1, 0)
        assert_eq!(got.get_pixel(h - 1, 0), base.get_pixel(0, 0));
        assert_eq!(Rotation::R270.add(Rotation::R180), Rotation::R90);
        assert_eq!(Rotation::R0.ccw(), Rotation::R270);
    }

    #[test]
    fn jpg_output_and_exist_policies() {
        let src = fixtures().join("lossless.webp");
        let out = tmpdir("jpg");
        let mut opts = Options { format: OutFormat::Jpg, out_dir: Some(out.clone()), ..Default::default() };
        let Outcome::Written(a) = convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap() else { panic!() };
        assert_eq!(a.extension().unwrap(), "jpg");
        assert_eq!(image::ImageReader::open(&a).unwrap().with_guessed_format().unwrap().format(), Some(image::ImageFormat::Jpeg));
        let Outcome::Written(b) = convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap() else { panic!() };
        assert!(b.to_string_lossy().ends_with("lossless (1).jpg"));
        opts.exist = ExistPolicy::Skip;
        assert!(matches!(convert_file(&src, Some(&fixtures()), Rotation::R0, &opts).unwrap(), Outcome::Skipped(_)));
        assert!(!out.join("lossless.jpg.part").exists());
    }

    #[test]
    fn subfolder_structure_preserved() {
        let root = tmpdir("tree-src");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::copy(fixtures().join("lossless.webp"), root.join("a/b/x.WEBP")).unwrap();
        fs::copy(fixtures().join("sample.avif"), root.join("top.avif")).unwrap();
        fs::write(root.join("note.txt"), "x").unwrap();
        let files = scan(&root, true);
        assert_eq!(files.len(), 2);
        assert_eq!(scan(&root, false).len(), 1);
        let out = tmpdir("tree-out");
        let opts = Options { out_dir: Some(out.clone()), ..Default::default() };
        for f in &files {
            convert_file(f, Some(&root), Rotation::R0, &opts).unwrap();
        }
        assert!(out.join("a/b/x.png").exists());
        assert!(out.join("top.png").exists());
    }

    #[test]
    fn default_subdir_under_source_root() {
        let root = tmpdir("subdir-src");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::copy(fixtures().join("lossless.webp"), root.join("a/x.webp")).unwrap();
        fs::copy(fixtures().join("sample.avif"), root.join("top 圖.avif")).unwrap();
        let opts = Options { sub_dir: Some(DEFAULT_SUBDIR.into()), ..Default::default() };
        for f in scan(&root, true) {
            convert_file(&f, Some(&root), Rotation::R0, &opts).unwrap();
        }
        let out = root.join(DEFAULT_SUBDIR);
        assert!(out.join("top 圖.png").exists());
        assert!(out.join("a").join("x.png").exists());
        // 沒有 root（單一檔案）時，放在檔案所在資料夾底下
        let lone = root.join("a").join("x.webp");
        assert_eq!(output_path(&lone, None, &opts), root.join("a").join(DEFAULT_SUBDIR).join("x.png"));
        // 指定了輸出資料夾時，以指定資料夾優先
        let custom = Options { out_dir: Some(root.join("c")), ..opts.clone() };
        assert_eq!(output_path(&lone, Some(&root), &custom), root.join("c").join("a").join("x.png"));
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["a_10.avif", "a_2.avif", "B_1.webp", "a_1.avif"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a_1.avif", "a_2.avif", "a_10.avif", "B_1.webp"]);
    }

    #[test]
    fn corrupt_file_reports_error() {
        let d = tmpdir("bad");
        let p = d.join("bad.avif");
        fs::write(&p, b"not an image").unwrap();
        assert!(convert_file(&p, None, Rotation::R0, &Options::default()).is_err());
        assert!(!d.join("bad.png").exists());
    }
}
