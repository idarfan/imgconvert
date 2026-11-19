fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // 目標為 Windows 時嵌入 exe 圖示與版本資訊（交叉編譯時使用 mingw windres）
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "ImgConvert")
            .set("FileDescription", "AVIF/WebP 轉 PNG/JPG 工具")
            .set_language(0x0404); // zh-TW
        if let Err(e) = res.compile() {
            println!("cargo:warning=圖示嵌入失敗：{e}");
        }
    }
}
