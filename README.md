# ImgConvert 圖片轉檔器

Windows 11 桌面程式：把 AVIF / WebP 批次轉成 PNG（無損）或 JPG。Rust + egui，單一 exe，不需安裝任何執行環境。

## 功能

- 選資料夾（可含子資料夾）、加入檔案、拖曳檔案／資料夾到視窗、拖到 exe 圖示上、右鍵「傳送到」
- 輸出 PNG：像素逐位元保留，10/12-bit AVIF 輸出 16-bit PNG，保留透明
- 輸出 JPG：品質 1–100（預設 95）；透明區域以白底合成
- 旋轉：全部左／右轉 90°，或每個檔案單獨旋轉（兩者疊加），右側即時預覽
- 自動套用檔案內 EXIF 方向資訊
- 多核心平行轉換、進度條、可中途取消、失敗原因逐檔顯示
- 輸出到原資料夾，或指定資料夾並保留子資料夾結構
- 左側設定卡片可拖曳排序；卡片順序與輸出位置會記住（存在 `%APPDATA%\ImgConvert\settings.txt`）
- 同名檔：自動改名 `name (1).png`／覆蓋／略過；先寫暫存檔再改名，中斷不留半個檔

## 限制

- JPG 本身是有損格式，「無損」只適用 PNG
- 動態 AVIF／WebP 只轉第一格
- 未嵌入 ICC 色彩描述檔；Display-P3 等廣色域圖在部分看圖軟體顏色會略淡

## 在 WSL2 建置

專案請放在 WSL 的 Linux 檔案系統（例如 `~/imgconvert`），不要放 `/mnt/c`，編譯會慢很多。

```bash
cd ~/imgconvert
./scripts/setup-wsl2.sh      # 第一次：裝 Rust、Windows 目標、MinGW、NSIS、編譯靜態 dav1d（約 5–10 分鐘）
./scripts/build-windows.sh   # 測試 → 編譯 → 產生安裝程式
```

產出（`dist/`）：

| 檔案 | 用途 |
|---|---|
| `ImgConvert-Setup-1.0.0.exe` | 安裝程式，轉發給別人用這個 |
| `ImgConvert-Portable-1.0.0.zip` | 免安裝版 |
| `ImgConvert.exe` | 主程式本體 |

在 WSL2 直接測：`./dist/ImgConvert.exe`（WSL 會叫 Windows 執行）。
在 WSL2 內跑 Linux 版 GUI（WSLg）：`cargo run --release`。
只跑轉檔測試：`cargo test --release`。

### 環境說明

| 元件 | 用途 |
|---|---|
| `rustup target x86_64-pc-windows-gnu` | 在 Linux 交叉編譯 Windows exe |
| `gcc-mingw-w64-x86-64` | Windows 連結器、windres（嵌入 exe 圖示） |
| `nasm` `meson` `ninja` | 編譯 dav1d（AVIF 解碼器，含 SIMD 加速） |
| `nsis`（makensis） | 在 Linux 產生 Windows 安裝程式 |
| `~/.local/win-deps` | Windows 版靜態 libdav1d.a，靜態連入 exe |

exe 只依賴 Windows 內建 DLL（`build-windows.sh` 會自動檢查），可直接拷到別台電腦執行。

## 安裝程式

- 安裝到 `%LOCALAPPDATA%\Programs\ImgConvert`，不需系統管理員權限
- 可選：桌面捷徑、右鍵「傳送到」
- 「設定 → 應用程式」可解除安裝

未做程式碼簽章，別人第一次執行會出現 SmartScreen「Windows 已保護您的電腦」→ 按「其他資訊 → 仍要執行」。要消除需購買程式碼簽章憑證，以 `osslsigncode` 在 WSL2 簽署 exe 與安裝程式。

## 專案結構

```
src/convert.rs        轉檔核心（解碼／旋轉／編碼）＋單元測試
src/main.rs           egui 介面、背景批次執行緒
src/theme.rs          介面配色（跟隨 Windows 淺色／深色）與共用元件
src/help.rs           「說明」對話框（操作說明、版本號）
src/settings.rs       記住卡片順序與輸出位置（純文字設定檔）＋單元測試
src/view.rs           主視窗版面（標題列、設定欄、清單、預覽、動作列）
src/icons.rs          3D 圖示載入（Microsoft Fluent Emoji 3D，MIT）
assets/icons/         3D 圖示 PNG 與授權檔
build.rs              嵌入 exe 圖示與版本資訊
installer/installer.nsi  NSIS 安裝程式腳本
scripts/              WSL2 環境建置、Windows 打包
tests/fixtures/       測試圖（avifdec_ref.png 由 libavif 官方解碼器產生，用來交叉驗證）
```

改版號：修改 `Cargo.toml` 的 `version`，重新執行 `build-windows.sh`。

## 變更記錄

### 2026-11-20 — v1.0.2：設定卡片可拖曳排序、記住輸出位置

- 左側「輸出格式／輸出位置／同名檔案／全部旋轉」四張卡片右上角新增六點把手，按住拖曳即可調整順序，藍線標示放下位置。
- 輸出位置預設改為「指定資料夾」。
- 卡片順序、輸出位置（含選好的資料夾）一有變動就存到 `%APPDATA%\ImgConvert\settings.txt`，重開程式沿用；解除安裝時一併刪除。
- 新增 `src/settings.rs`（純文字 key=value，不加新依賴）與 5 個單元測試。

### 2026-11-19 — 未選輸出資料夾時自動存到「已轉換好」

- 輸出位置選「指定資料夾」但沒有選路徑時，按「開始轉換」不再跳出錯誤，改為自動在來源資料夾底下建立「已轉換好」子資料夾並存入（含子資料夾時保留原結構）。
- 已選路徑時新增「清除」按鈕，可改回預設行為。

### 2026-11-19 — 介面改版：卡片式版面

- 輸出設定集中在圓角卡片裡，原本的圓點選項改成分段按鈕；字級和間距加大。
- 「開始轉換」改成右下角強調色的主按鈕；還沒開始轉換時不顯示進度條。
- 檔案清單改成斑馬紋，每列右側用標籤顯示狀態（待轉換／完成／略過／失敗），失敗原因移到滑鼠提示。
- 清單空白時顯示拖放提示區，拖著檔案經過視窗時會亮起來。
- 配色跟隨 Windows 淺色／深色設定切換；新增 `src/theme.rs`。

### 2026-11-19 — 新增「說明」對話框與版本號

- 工具列右側新增「❓ 說明」按鈕（轉換中也能按），開啟操作說明對話框；按「關閉」、Esc 或點對話框外面會關閉。
- 工具列、視窗標題和說明對話框都會顯示版本號，自動取自 `Cargo.toml` 的 `version`。
- 說明內容放在新檔 `src/help.rs`。

### 2026-11-19 — 介面重新設計：Windows 11 設定頁風格＋3D 圖示

- 版面參考 Windows 11 設定頁（SettingsCard）：上方標題列、左側設定欄、中間檔案清單、右側預覽、下方動作列；每項設定是一張卡片，包含 3D 圖示、標題、說明和控制項。
- 「選擇資料夾」「加入檔案」改成大圖示磚塊；「包含子資料夾」改成開關。
- 3D 圖示採用 Microsoft Fluent Emoji 3D（MIT 授權），圖檔在 `assets/icons/`，編進 exe；授權檔會以 `THIRD-PARTY-LICENSES.txt` 隨安裝程式和 zip 一起發佈。
- 說明對話框重新排版：「快速上手」三步驟、每段一張有圖示的卡片、條列懸掛縮排、捲軸不再蓋住文字。
- 預設視窗加大為 1200×780。
- 介面程式碼從 `main.rs` 拆到 `src/view.rs`，圖示載入在 `src/icons.rs`。

### 2026-11-19 — v1.0.1：開始轉換移到標題列正中間

- 版本升到 1.0.1。
- 「開始轉換」（轉換中為「取消」）移到標題列正中間；標題列第二排顯示狀態訊息和轉換進度條；移除底部動作列。
- 說明對話框底部加上作者聯絡信箱（可點擊開啟郵件程式）。
- 程式啟動時視窗自動最大化。
- `build-windows.sh` 產生新版後會自動刪除 `dist/` 裡舊版的安裝程式與 zip。
- 新增有損＋透明通道 WebP 的測試圖 `tests/fixtures/lossy_alpha.webp` 和測試，確認 WebP → PNG 逐位元保留像素與透明度（WebP 轉 PNG 原本就支援）。
