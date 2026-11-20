//! 3D 圖示：Microsoft Fluent Emoji 3D（MIT 授權，見 assets/icons/LICENSE-fluentui-emoji.txt）。
//! 圖檔編進 exe，啟動時載入一次成 GPU 貼圖；用 mipmap 縮小，避免鋸齒。

use eframe::egui;

macro_rules! icon_set {
    ($($field:ident => $file:literal),* $(,)?) => {
        pub struct Icons {
            $(pub $field: egui::TextureHandle,)*
        }

        impl Icons {
            pub fn load(ctx: &egui::Context) -> Self {
                Self { $($field: load(ctx, stringify!($field), include_bytes!(concat!("../assets/icons/", $file))),)* }
            }
        }

        #[cfg(test)]
        const ALL: &[(&str, &[u8])] = &[$((stringify!($field), include_bytes!(concat!("../assets/icons/", $file))),)*];
    };
}

icon_set! {
    folder => "file_folder_3d.png",
    picture => "framed_picture_3d.png",
    trash => "wastebasket_3d.png",
    info => "information_3d.png",
    palette => "artist_palette_3d.png",
    outbox => "outbox_tray_3d.png",
    dividers => "card_index_dividers_3d.png",
    rotate => "counterclockwise_arrows_button_3d.png",
    play => "play_button_3d.png",
    stop => "stop_button_3d.png",
    bulb => "light_bulb_3d.png",
    inbox => "inbox_tray_3d.png",
    gear => "gear_3d.png",
    check => "check_mark_button_3d.png",
    cross => "cross_mark_3d.png",
    movie => "clapper_board_3d.png",
    logo => "../icon.png",
}

fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let rgba = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

fn load(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    // 圖檔是編譯時嵌入的，解碼失敗代表打包錯誤；退回 1×1 透明圖，不讓程式崩潰
    let img = decode(bytes).unwrap_or_else(|e| {
        eprintln!("圖示 {name} 解碼失敗：{e}");
        egui::ColorImage::new([1, 1], vec![egui::Color32::TRANSPARENT])
    });
    let opts = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    ctx.load_texture(name, img, opts)
}

/// 指定顯示大小（邏輯像素）的圖示
pub fn img(tex: &egui::TextureHandle, size: f32) -> egui::Image<'static> {
    egui::Image::new(egui::load::SizedTexture::new(tex.id(), egui::vec2(size, size)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_embedded_icons_decode() {
        for (name, bytes) in super::ALL {
            let img = super::decode(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(img.size[0] >= 64 && img.size[1] >= 64, "{name} 太小");
        }
    }
}
