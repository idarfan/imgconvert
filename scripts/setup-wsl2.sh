#!/usr/bin/env bash
# 在 WSL2 (Ubuntu 22.04/24.04) 建立 ImgConvert 的交叉編譯環境：
#   Rust + Windows 目標、MinGW-w64、NSIS、靜態 dav1d（AVIF 解碼）
# 可重複執行（已裝好的步驟會略過）。
set -euo pipefail

DAV1D_VERSION="1.5.1"
WIN_DEPS="${WIN_DEPS:-$HOME/.local/win-deps}"   # Windows 用的靜態函式庫安裝位置

echo "==> [1/4] 安裝系統套件"
sudo apt-get update -q
sudo apt-get install -y -q \
  build-essential pkg-config git curl ca-certificates \
  gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 \
  nasm meson ninja-build nsis \
  libdav1d-dev libgtk-3-dev libxkbcommon-dev   # 後三者：在 WSL2 內直接跑測試／GUI（WSLg）用

echo "==> [2/4] 安裝 Rust 與 Windows 目標"
if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"
rustup update stable
rustup target add x86_64-pc-windows-gnu

echo "==> [3/4] 交叉編譯靜態 dav1d ${DAV1D_VERSION}（Windows x64）"
if [ -f "$WIN_DEPS/lib/libdav1d.a" ]; then
  echo "    已存在 $WIN_DEPS/lib/libdav1d.a，略過"
else
  SRC="$(mktemp -d)"
  git clone -q --depth 1 --branch "$DAV1D_VERSION" https://github.com/videolan/dav1d.git "$SRC/dav1d"
  cat > "$SRC/mingw-cross.txt" <<'EOF'
[binaries]
c = 'x86_64-w64-mingw32-gcc'
cpp = 'x86_64-w64-mingw32-g++'
ar = 'x86_64-w64-mingw32-ar'
strip = 'x86_64-w64-mingw32-strip'
windres = 'x86_64-w64-mingw32-windres'
nasm = 'nasm'
pkg-config = 'pkg-config'

[host_machine]
system = 'windows'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF
  meson setup "$SRC/build" "$SRC/dav1d" --cross-file "$SRC/mingw-cross.txt" \
    --prefix="$WIN_DEPS" --libdir=lib --buildtype=release \
    -Ddefault_library=static -Denable_tools=false -Denable_tests=false
  ninja -C "$SRC/build" install
  rm -rf "$SRC"
fi

echo "==> [4/4] 檢查"
rustc --version
rustup target list --installed | grep -q x86_64-pc-windows-gnu && echo "    Windows 目標 OK"
x86_64-w64-mingw32-gcc --version | head -1
makensis -VERSION
PKG_CONFIG_PATH="$WIN_DEPS/lib/pkgconfig" pkg-config --modversion dav1d | sed 's/^/    dav1d (Windows) /'

echo
echo "完成。接著在專案目錄執行： ./scripts/build-windows.sh"
