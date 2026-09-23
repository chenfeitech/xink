#!/usr/bin/env bash
#
# xink — macOS 安装包构建脚本
#
# 职责：编译 → 生成图标(.icns) → 组装 .app Bundle → 签名 → 打包 .dmg / .pkg
#
# 用法：
#   ./scripts/build_macos.sh                 # 编译 release 并产出 .app + .dmg
#   ./scripts/build_macos.sh --pkg           # 额外产出 .pkg 安装包
#   ./scripts/build_macos.sh --skip-build    # 跳过编译（用已有的 target 产物）
#   ./scripts/build_macos.sh --no-dmg        # 只产出 .app
#   PROFILE=debug ./scripts/build_macos.sh --skip-build   # 用 debug 产物试跑打包流程
#
# 可用环境变量覆盖：APP_NAME / BUNDLE_ID / VERSION / ICON_SRC / PROFILE

set -euo pipefail

# ── 配置 ──────────────────────────────────────────────────────
APP_NAME="${APP_NAME:-xink}"
BUNDLE_ID="${BUNDLE_ID:-com.helight.xink}"
ICON_SRC="${ICON_SRC:-resources/AppIcon.png}"
PROFILE="${PROFILE:-release}"
MIN_MACOS="${MIN_MACOS:-11.0}"

# ── 参数 ──────────────────────────────────────────────────────
DO_BUILD=1
DO_DMG=1
DO_PKG=0

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \?//'
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) DO_BUILD=0; shift ;;
    --no-dmg)     DO_DMG=0;   shift ;;
    --pkg)        DO_PKG=1;   shift ;;
    -h|--help)    usage ;;
    *) echo "未知参数: $1" >&2; usage ;;
  esac
done

# ── 路径 ──────────────────────────────────────────────────────
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${BUILD_DIR:-$ROOT_DIR/build}"
APP_DIR="$BUILD_DIR/$APP_NAME.app"
CONTENTS="$APP_DIR/Contents"

cd "$ROOT_DIR"

# 版本号取自 Cargo.toml，避免两处维护
if [[ -z "${VERSION:-}" ]]; then
  VERSION="$(sed -nE 's/^version[[:space:]]*=[[:space:]]*"(.*)"/\1/p' Cargo.toml | head -1)"
fi
if [[ -z "$VERSION" ]]; then
  echo "无法从 Cargo.toml 解析版本号" >&2
  exit 1
fi

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m警告:\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

# 校验图标资源确实被内嵌进二进制。
#
# gpui-kit-assets 用 rust-embed 提供 IconName 的 SVG；rust-embed 默认只在
# release 内嵌，debug 构建改为运行时按编译期记录的绝对路径读盘。那样打出来的包
# 在本机正常、拷到别的机器图标会全部消失。这里用二进制里 SVG 的数量做兜底检查
# （默认图标集约 100 个，内嵌后应有上百处；未内嵌时只有我们内联工具栏的几处）。
verify_embedded_icons() {
  local count
  count="$(strings "$BINARY" 2>/dev/null | grep -c 'http://www.w3.org/2000/svg' || true)"
  if [[ "${count:-0}" -lt 50 ]]; then
    die "图标资源未内嵌（二进制内仅 ${count:-0} 处 SVG）。
  这样打出的包在其它机器上会丢失全部 IconName 图标。
  排查：确认 Cargo.toml 中 rust-embed 已启用 debug-embed，或改用 release 构建。"
  fi
  log "图标资源已内嵌（二进制内检测到 ${count} 处 SVG）"
}

# ── 0. 依赖检查 ───────────────────────────────────────────────
command -v iconutil >/dev/null || die "需要 iconutil（Xcode 命令行工具：xcode-select --install）"
command -v sips >/dev/null || die "需要 sips（macOS 自带）"
if [[ "$DO_DMG" -eq 1 ]]; then
  command -v hdiutil >/dev/null || die "需要 hdiutil（macOS 自带）"
fi
if [[ "$DO_PKG" -eq 1 ]]; then
  command -v pkgbuild >/dev/null || die "需要 pkgbuild（Xcode 命令行工具）"
fi

# ── 1. 编译 ───────────────────────────────────────────────────
BINARY="$ROOT_DIR/target/$PROFILE/$APP_NAME"
if [[ "$DO_BUILD" -eq 1 ]]; then
  log "编译 ($PROFILE)…"
  cargo build --"$PROFILE" --bin "$APP_NAME"
else
  log "跳过编译，使用已有产物: $BINARY"
fi
[[ -f "$BINARY" ]] || die "找不到可执行文件: $BINARY"
verify_embedded_icons
if [[ "$PROFILE" != "release" ]]; then
  warn "正在打包 $PROFILE 构建（图标已内嵌，但分发建议用 release）"
fi

# ── 2. 生成 .icns ─────────────────────────────────────────────
ICONSET="$BUILD_DIR/$APP_NAME.iconset"
ICNS="$BUILD_DIR/$APP_NAME.icns"

if [[ ! -f "$ICON_SRC" ]]; then
  die "找不到图标源文件: $ICON_SRC（可用 ICON_SRC=路径 指定）"
fi

log "生成图标 AppIcon → $APP_NAME.icns"
rm -rf "$ICONSET" "$ICNS"
mkdir -p "$ICONSET"

# (目标边长, iconset 文件名)
gen_icon() {
  local size="$1" name="$2"
  sips -z "$size" "$size" "$ICON_SRC" --out "$ICONSET/$name" >/dev/null
}
gen_icon 16   icon_16x16.png
gen_icon 32   icon_16x16@2x.png
gen_icon 32   icon_32x32.png
gen_icon 64   icon_32x32@2x.png
gen_icon 128  icon_128x128.png
gen_icon 256  icon_128x128@2x.png
gen_icon 256  icon_256x256.png
gen_icon 512  icon_256x256@2x.png
gen_icon 512  icon_512x512.png
gen_icon 1024 icon_512x512@2x.png

iconutil -c icns "$ICONSET" -o "$ICNS" || die "iconutil 生成 .icns 失败"
rm -rf "$ICONSET"

# ── 3. 组装 .app Bundle ───────────────────────────────────────
log "组装 $APP_NAME.app"
rm -rf "$APP_DIR"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"

cp "$BINARY" "$CONTENTS/MacOS/$APP_NAME"
cp "$ICNS" "$CONTENTS/Resources/$APP_NAME.icns"

cat > "$CONTENTS/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key>
  <string>$APP_NAME</string>
  <key>CFBundleIconFile</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_MACOS</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.productivity</string>
</dict>
</plist>
PLIST

# ── 4. 签名 ───────────────────────────────────────────────────
# 没有开发者证书时用 ad-hoc 签名（"-"）：本机可正常运行，分发给别人仍需正式签名+公证
log "签名（ad-hoc）"
codesign --force --deep --sign - "$APP_DIR" || die "签名失败"

# ── 5. 打包 .dmg ──────────────────────────────────────────────
DMG="$BUILD_DIR/$APP_NAME-$VERSION.dmg"
if [[ "$DO_DMG" -eq 1 ]]; then
  log "打包 $APP_NAME-$VERSION.dmg"
  STAGING="$BUILD_DIR/dmg"
  rm -rf "$STAGING" "$DMG"
  mkdir -p "$STAGING"
  cp -R "$APP_DIR" "$STAGING/"
  ln -s /Applications "$STAGING/Applications"
  hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$STAGING" \
    -ov -format UDZO -imagekey zlib-level=9 \
    "$DMG" >/dev/null || die "hdiutil 打包失败"
  rm -rf "$STAGING"
fi

# ── 6. 可选：.pkg 安装包 ──────────────────────────────────────
PKG="$BUILD_DIR/$APP_NAME-$VERSION.pkg"
if [[ "$DO_PKG" -eq 1 ]]; then
  log "打包 $APP_NAME-$VERSION.pkg（安装到 /Applications）"
  rm -f "$PKG"
  pkgbuild \
    --identifier "$BUNDLE_ID" \
    --version "$VERSION" \
    --install-location /Applications \
    --component "$APP_DIR" \
    "$PKG" >/dev/null || die "pkgbuild 打包失败"
fi

# ── 完成 ──────────────────────────────────────────────────────
log "完成（版本 ${VERSION}）"
echo "  .app : $APP_DIR"
[[ "$DO_DMG" -eq 1 ]] && echo "  .dmg : $DMG"
[[ "$DO_PKG" -eq 1 ]] && echo "  .pkg : $PKG"
echo
echo "提示：ad-hoc 签名仅本机可用。正式分发需要："
echo "  1) codesign --sign 'Developer ID Application: ...' $APP_DIR"
echo "  2) 制作 dmg 后执行 xcrun notarytool submit ... 完成公证"
