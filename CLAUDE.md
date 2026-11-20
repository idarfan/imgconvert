# ImgConvert — Claude Code 工作守則

Windows 11 桌面程式，把 AVIF/WebP 批次轉成 PNG（無損）或 JPG。用 Rust + egui 寫成，在 WSL2 交叉編譯成單一 exe，並用 NSIS 打包安裝程式。

## 指令

| 用途 | 指令 |
|---|---|
| 單元測試 | `cargo test --release` |
| 在 WSLg 跑 Linux 版 GUI | `cargo run --release` |
| 完整打包（測試、編譯 Windows 版、產生安裝程式、zip） | `./scripts/build-windows.sh` |
| 第一次建環境 | `./scripts/setup-wsl2.sh` |
| 在 Windows 實機測試 | `./dist/ImgConvert.exe [資料夾]` |

如果要單獨跑 `cargo build --target x86_64-pc-windows-gnu`，必須先設好下面的環境變數，不然 dav1d 會連結失敗：

```bash
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH_x86_64_pc_windows_gnu="$HOME/.local/win-deps/lib/pkgconfig"
export SYSTEM_DEPS_DAV1D_LINK=static
```

建議直接用 `build-windows.sh`，它已經設好這些變數。

## 結構

- `src/convert.rs`：轉檔核心（掃描、解碼、旋轉、編碼、路徑與同名處理），不依賴 GUI，單元測試也在這裡
- `src/main.rs`：App 狀態、背景批次執行緒（rayon）、中文字型載入
- `src/view.rs`：主視窗版面（參考 Windows 11 設定頁：標題列、左側設定卡片、清單、預覽、動作列）
- `src/icons.rs` + `assets/icons/`：3D 圖示（Microsoft Fluent Emoji 3D，MIT）。新增圖示從 github.com/microsoft/fluentui-emoji 的 `3D` 資料夾取用，並在 `icon_set!` 登記；授權檔必須隨安裝程式和 zip 發佈
- `src/theme.rs`：配色（跟隨系統淺色／深色）、字級，以及卡片、分段按鈕、主按鈕、狀態標籤等共用元件
- `src/help.rs`：「說明」對話框的操作說明文字與版本號（取自 `Cargo.toml`）。改了操作方式要同步更新這裡
- MP4 to AVIF 分頁：
  - `src/video.rs`：純邏輯，包含 ffprobe 解析、A/B 範圍、ffmpeg 參數、進度與錯誤訊息，改行為必須補測試
  - `src/ffmpeg.rs`：開程序呼叫 ffmpeg／ffprobe，先找 exe 旁的 `ffmpeg\`，找不到才用 PATH
  - `src/player.rs`、`src/timeline.rs`、`src/video_view.rs`：播放器狀態、時間軸元件（含音量波形）、分頁版面
  - `src/audio.rs`：預覽聲音。`cpal` 只是 Windows 依賴（`[target.'cfg(windows)'.dependencies]`），Linux 版是不出聲的替代實作，所以 WSL 不需要裝 ALSA 開發套件。聲音同時當播放時鐘，畫面依它送出影格
  - `src/waveform.rs`：波形計算，用 ffmpeg 把音軌轉成單聲道 8 kHz 後每 10 毫秒取峰值與 RMS
  - ffmpeg 由 `scripts/fetch-ffmpeg.sh` 下載固定版本（LGPL 3.0+ shared，含 AMF），放在 `dist/ffmpeg/` 隨安裝程式發佈。它不連結進 exe，所以規則 2 只管 ImgConvert.exe。換 ffmpeg 版本要更新 SHA256，並重新確認沒有 `--enable-gpl`
  - AMD `av1_amf` 的已知行為：寬高不是 16 的倍數時會自行補邊（1080 → 1082）而檔頭仍寫 1080，Firefox、FastStone 會打不開。所以 `Mode::output_size` 一律取 16 的倍數（模式 1 置中裁切、模式 2 縮放），轉完用 ffprobe 檢查實際大小。目前只支援 AMD GPU，不改用 CPU（使用者決定）
  - 影格率 15／30／60 fps，預設 15（使用者決定），用 `fps=` 濾鏡；原片較慢時不加
- `build.rs`：目標是 Windows 時嵌入 exe 圖示和版本資訊（winresource 搭配 mingw windres）
- `installer/installer.nsi`：NSIS 安裝程式，裝在使用者目錄，不需要系統管理員權限
- `scripts/`：WSL2 環境建置、Windows 打包
- `tests/fixtures/`：測試圖。`avifdec_ref.png` 由 libavif 官方的 avifdec 產生，用來交叉驗證，不准改

## 不可違反的規則

1. **PNG 輸出必須逐像素無損**：保留原本的色深（10/12-bit AVIF 輸出 16-bit PNG）和透明通道。PNG 路徑禁止任何縮放、色彩轉換或降到 8-bit。
2. **exe 只能依賴 Windows 內建 DLL**：`build-windows.sh` 的 DLL 檢查必須通過。禁止拿掉 `.cargo/config.toml` 裡的 `+crt-static`，也禁止改用動態連結 dav1d。
3. **改動 `convert.rs` 的行為必須補測試**，而且 `cargo test --release` 要 0 failures 才算完成。
4. **新增依賴要先問使用者**，並附上套件名、授權、維護者。不接受以下兩類：
   - AGPL/GPL 授權：程式要轉發給別人，例如 zenavif 就因為 AGPL 被排除
   - 中國團隊開發的專案
5. JPG 是有損格式，UI 文字和說明不准宣稱 JPG 無損。
6. 安裝程式的捷徑檔名一律用 ASCII（Wine 實測過，中文 .lnk 檔名會建立失敗）。
7. 這是 Windows 程式：路徑要用 `Path`/`PathBuf` 處理，禁止寫死 `/` 或 `\`；檔名可能有中文和空白。

## 版號與發佈

- 改版號只改 `Cargo.toml` 的 `version`，安裝程式會自動讀取。
- 發佈前要做三件事：
  - 跑 `./scripts/build-windows.sh` 全程通過
  - 用 `./dist/ImgConvert.exe` 實際轉一個含子資料夾的測試資料夾
  - 安裝和解除安裝各試一次

## 已知限制（改善前先跟使用者確認方向）

- 動態 AVIF/WebP 只轉第一格
- 未嵌入 ICC 色彩描述檔，廣色域圖的顏色可能偏淡
- AVIF 的 irot/imir 旋轉資訊沒有套用（只套用 EXIF 方向）
- 安裝程式未簽章，會出現 SmartScreen 警告
