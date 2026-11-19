#!/usr/bin/env bash
# 交叉編譯 Windows 版 ImgConvert.exe，並產生安裝程式與免安裝 zip 到 dist/
set -euo pipefail
cd "$(dirname "$0")/.."

WIN_DEPS="${WIN_DEPS:-$HOME/.local/win-deps}"
TARGET=x86_64-pc-windows-gnu
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"

[ -f "$WIN_DEPS/lib/libdav1d.a" ] || { echo "找不到 $WIN_DEPS/lib/libdav1d.a，請先執行 scripts/setup-wsl2.sh"; exit 1; }

# 讓 dav1d-sys 找到 Windows 版靜態 dav1d
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH_x86_64_pc_windows_gnu="$WIN_DEPS/lib/pkgconfig"
export SYSTEM_DEPS_DAV1D_LINK=static

echo "==> 執行單元測試（Linux 原生）"
cargo test --release

echo "==> 編譯 Windows 版 v$VERSION"
# EXTRA_CARGO_ARGS 一般留空；僅在沒有 rustup 目標的環境才需要 -Zbuild-std
cargo build --release --target "$TARGET" ${EXTRA_CARGO_ARGS:-} ${CARGO_TARGET_DIR:+--target-dir "$CARGO_TARGET_DIR"}
OUT="${CARGO_TARGET_DIR:-target}/$TARGET/release/ImgConvert.exe"

mkdir -p dist
cp "$OUT" dist/ImgConvert.exe

echo "==> 檢查 DLL 相依（應只有 Windows 內建 DLL）"
x86_64-w64-mingw32-objdump -p dist/ImgConvert.exe | awk '/DLL Name/{print "    "$3}' | sort -fu
if x86_64-w64-mingw32-objdump -p dist/ImgConvert.exe | grep -qiE 'DLL Name: (libgcc|libwinpthread|libstdc|libdav1d)'; then
  echo "錯誤：exe 依賴非系統 DLL，轉發到別台電腦會無法執行"; exit 1
fi

echo "==> 產生安裝程式"
(cd installer && makensis -V2 -DVERSION="$VERSION" -DEXE=../dist/ImgConvert.exe installer.nsi)

echo "==> 產生免安裝版 zip"
rm -f "dist/ImgConvert-Portable-$VERSION.zip"
(cd dist && python3 -c "import zipfile,sys; z=zipfile.ZipFile(sys.argv[1],'w',zipfile.ZIP_DEFLATED); z.write('ImgConvert.exe'); z.write(sys.argv[2], 'THIRD-PARTY-LICENSES.txt'); z.close()" "ImgConvert-Portable-$VERSION.zip" ../assets/icons/LICENSE-fluentui-emoji.txt)

# 新版都產生成功後才刪舊版（中途失敗時保留舊版）
echo "==> 刪除舊版安裝程式與 zip"
for f in dist/ImgConvert-Setup-*.exe dist/ImgConvert-Portable-*.zip; do
  [ -e "$f" ] || continue
  case "$f" in
    "dist/ImgConvert-Setup-$VERSION.exe" | "dist/ImgConvert-Portable-$VERSION.zip") ;;
    *) rm -v "$f" ;;
  esac
done

ls -lh dist/
echo
echo "完成。在 WSL2 可直接執行 ./dist/ImgConvert.exe 測試；"
echo "複製到 Windows 桌面： cp dist/ImgConvert-Setup-$VERSION.exe /mnt/c/Users/\$(cmd.exe /c 'echo %USERNAME%' 2>/dev/null | tr -d '\r')/Desktop/"
