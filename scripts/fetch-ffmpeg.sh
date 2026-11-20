#!/usr/bin/env bash
# 下載固定版本的 Windows 版 ffmpeg（LGPL、共用 DLL 版，含 AMD AMF 硬體編碼），核對 SHA256，
# 解出 ffmpeg.exe、ffprobe.exe、DLL 與授權檔到 dist/ffmpeg/，給安裝程式與 zip 內附。
# 來源：github.com/FFmpeg-Builds/FFmpeg-Builds（打包者 BtbN，打包腳本 MIT）。
# 這個版本以 --enable-version3 編譯，授權為 LGPL 第 3 版以上（libavutil 自己回報 "LGPL version 3 or later"）；
# 沒有 --enable-gpl／--enable-nonfree，有 --enable-amf。
set -euo pipefail
cd "$(dirname "$0")/.."

TAG="autobuild-2026-11-19-12-39"
NAME="ffmpeg-n8.1.3-36-ge22a4510c9-win64-lgpl-shared-8.1"
SHA256="ae947c6f7f87d19cf3de80e448409419a9e81a18987d55ead133c55c2db29e9c"
URL="https://github.com/FFmpeg-Builds/FFmpeg-Builds/releases/download/$TAG/$NAME.zip"
# 自動建置的 release 過一陣子會被上游刪除，所以下載後保留在快取，之後不再重新下載
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/imgconvert"
ZIP="$CACHE/$NAME.zip"
OUT=dist/ffmpeg

mkdir -p "$CACHE"
if [ ! -f "$ZIP" ]; then
  echo "==> 下載 $NAME.zip"
  curl -fL --retry 3 -o "$ZIP.part" "$URL"
  mv "$ZIP.part" "$ZIP"
fi
echo "$SHA256  $ZIP" | sha256sum -c --quiet || { echo "錯誤：$ZIP 的 SHA256 不符，請刪除後重試"; exit 1; }

rm -rf "$OUT" && mkdir -p "$OUT"
unzip -q -j -o "$ZIP" "$NAME/bin/ffmpeg.exe" "$NAME/bin/ffprobe.exe" "$NAME/bin/*.dll" "$NAME/LICENSE.txt" -d "$OUT"
cat > "$OUT/SOURCE.txt" <<EOF
這個資料夾的 ffmpeg / ffprobe 及 DLL 以 GNU LGPL 第 3 版或更新版本授權（見 LICENSE.txt），
ImgConvert 以另開程序的方式呼叫它們；你可以自行換成其他相容版本的 ffmpeg。
版本：$NAME
下載：$URL
FFmpeg 原始碼：https://github.com/FFmpeg/FFmpeg/commit/e22a4510c9
打包腳本原始碼（MIT）：https://github.com/FFmpeg-Builds/FFmpeg-Builds/tree/$TAG
EOF
echo "==> ffmpeg 已放到 $OUT（$(du -sh "$OUT" | cut -f1)）"
