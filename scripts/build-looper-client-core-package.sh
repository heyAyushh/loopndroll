#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$ROOT_DIR/crates/looper-client-core"
PACKAGE_DIR="$ROOT_DIR/swift/LooperClientCore"
BINDINGS_DIR="$PACKAGE_DIR/Sources/LooperClientCore/Generated"
FRAMEWORKS_DIR="$PACKAGE_DIR/Frameworks"
XCFRAMEWORK_DIR="$FRAMEWORKS_DIR/LooperClientCoreFFI.xcframework"
FRAMEWORK_BUILD_DIR="$CRATE_DIR/target/looper-client-core-frameworks"
HOST_LIBRARY="target/release/liblooper_client_core.dylib"
FFI_MODULE_NAME="looper_client_coreFFI"
FFI_FRAMEWORK_NAME="${FFI_MODULE_NAME}.framework"
FFI_HEADER_NAME="looper_client_coreFFI.h"

strip_generated_whitespace() {
  local output_dir="$1"
  find "$output_dir" -type f \
    \( -name '*.h' -o -name '*.swift' -o -name '*.modulemap' \) \
    -exec perl -0pi -e 's/[ \t]+$//mg' {} +
}

IOS_TARGETS=(
  aarch64-apple-ios
  aarch64-apple-ios-sim
)

MACOS_TARGETS=(
  aarch64-apple-darwin
  x86_64-apple-darwin
)

build_apple_target() {
  local target="$1"
  local deployment_env_name="$2"
  local deployment_target="$3"

  rustup target add "$target"
  env "$deployment_env_name=$deployment_target" \
    cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --target "$target" --release
}

echo "Building looper-client-core host library for UniFFI metadata"
cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --release

echo "Generating Swift UniFFI bindings into $BINDINGS_DIR"
mkdir -p "$BINDINGS_DIR"
(
  cd "$CRATE_DIR"
  cargo run \
    --features uniffi-cli \
    --bin uniffi-bindgen \
    -- generate \
    --library \
    --crate looper_client_core \
    "$HOST_LIBRARY" \
    --language swift \
    --out-dir "$BINDINGS_DIR"
)
strip_generated_whitespace "$BINDINGS_DIR"
cp "$BINDINGS_DIR/looper_client_coreFFI.modulemap" "$BINDINGS_DIR/module.modulemap"

for target in "${IOS_TARGETS[@]}"; do
  build_apple_target "$target" IPHONEOS_DEPLOYMENT_TARGET 18.0
done

for target in "${MACOS_TARGETS[@]}"; do
  build_apple_target "$target" MACOSX_DEPLOYMENT_TARGET 15.0
done

mkdir -p "$FRAMEWORKS_DIR"
if [[ -e "$XCFRAMEWORK_DIR" ]]; then
  echo "Replacing $XCFRAMEWORK_DIR"
  rm -R "$XCFRAMEWORK_DIR"
fi
if [[ -e "$FRAMEWORK_BUILD_DIR" ]]; then
  rm -R "$FRAMEWORK_BUILD_DIR"
fi
mkdir -p "$FRAMEWORK_BUILD_DIR"

write_framework_info_plist() {
  local plist_path="$1"
  local min_os_version="$2"
  local bundle_suffix="$3"

  cat >"$plist_path" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>${FFI_MODULE_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>dev.looper.clientcoreffi.${bundle_suffix}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${FFI_MODULE_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>FMWK</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>MinimumOSVersion</key>
  <string>${min_os_version}</string>
</dict>
</plist>
PLIST
}

write_framework_modulemap() {
  local modulemap_path="$1"

  cat >"$modulemap_path" <<MODULEMAP
framework module ${FFI_MODULE_NAME} {
  umbrella header "${FFI_HEADER_NAME}"
  export *
  module * { export * }
}
MODULEMAP
}

make_shallow_static_framework() {
  local target="$1"
  local min_os_version="$2"
  local framework_dir="$FRAMEWORK_BUILD_DIR/${target}/${FFI_FRAMEWORK_NAME}"
  local modules_dir="$framework_dir/Modules"
  local headers_dir="$framework_dir/Headers"
  local bundle_suffix
  bundle_suffix="$(printf '%s' "$target" | tr -cd '[:alnum:]')"

  mkdir -p "$modules_dir" "$headers_dir"
  cp "$CRATE_DIR/target/${target}/release/liblooper_client_core.a" \
    "$framework_dir/${FFI_MODULE_NAME}"
  cp "$BINDINGS_DIR/$FFI_HEADER_NAME" "$headers_dir/$FFI_HEADER_NAME"
  strip_generated_whitespace "$headers_dir"
  write_framework_modulemap "$modules_dir/module.modulemap"
  write_framework_info_plist "$framework_dir/Info.plist" "$min_os_version" "$bundle_suffix"
}

make_versioned_macos_static_framework() {
  local target="$1"
  local min_os_version="$2"
  local framework_dir="$FRAMEWORK_BUILD_DIR/${target}/${FFI_FRAMEWORK_NAME}"
  local version_dir="$framework_dir/Versions/A"
  local headers_dir="$version_dir/Headers"
  local modules_dir="$version_dir/Modules"
  local resources_dir="$version_dir/Resources"
  local bundle_suffix
  bundle_suffix="$(printf '%s' "$target" | tr -cd '[:alnum:]')"

  mkdir -p "$headers_dir" "$modules_dir" "$resources_dir"
  cp "$CRATE_DIR/target/${target}/release/liblooper_client_core.a" \
    "$version_dir/${FFI_MODULE_NAME}"
  cp "$BINDINGS_DIR/$FFI_HEADER_NAME" "$headers_dir/$FFI_HEADER_NAME"
  strip_generated_whitespace "$headers_dir"
  write_framework_modulemap "$modules_dir/module.modulemap"
  write_framework_info_plist \
    "$resources_dir/Info.plist" \
    "$min_os_version" \
    "$bundle_suffix"

  ln -s A "$framework_dir/Versions/Current"
  ln -s Versions/Current/Headers "$framework_dir/Headers"
  ln -s Versions/Current/Modules "$framework_dir/Modules"
  ln -s Versions/Current/Resources "$framework_dir/Resources"
  ln -s "Versions/Current/${FFI_MODULE_NAME}" "$framework_dir/${FFI_MODULE_NAME}"
}

make_universal_macos_static_framework() {
  local framework_parent="$FRAMEWORK_BUILD_DIR/macos-universal"
  local framework_dir="$framework_parent/$FFI_FRAMEWORK_NAME"
  local binary_path="$framework_dir/Versions/A/${FFI_MODULE_NAME}"

  rm -Rf "$framework_parent"
  mkdir -p "$framework_parent"
  cp -R "$FRAMEWORK_BUILD_DIR/aarch64-apple-darwin/$FFI_FRAMEWORK_NAME" "$framework_parent/"
  lipo -create \
    "$FRAMEWORK_BUILD_DIR/aarch64-apple-darwin/$FFI_FRAMEWORK_NAME/Versions/A/${FFI_MODULE_NAME}" \
    "$FRAMEWORK_BUILD_DIR/x86_64-apple-darwin/$FFI_FRAMEWORK_NAME/Versions/A/${FFI_MODULE_NAME}" \
    -output "$binary_path"
  write_framework_info_plist \
    "$framework_dir/Versions/A/Resources/Info.plist" \
    15.0 \
    macosuniversal
}

make_shallow_static_framework aarch64-apple-ios 18.0
make_shallow_static_framework aarch64-apple-ios-sim 18.0
make_versioned_macos_static_framework aarch64-apple-darwin 15.0
make_versioned_macos_static_framework x86_64-apple-darwin 15.0
make_universal_macos_static_framework

echo "Creating $XCFRAMEWORK_DIR"
xcodebuild -create-xcframework \
  -framework "$FRAMEWORK_BUILD_DIR/aarch64-apple-ios/$FFI_FRAMEWORK_NAME" \
  -framework "$FRAMEWORK_BUILD_DIR/aarch64-apple-ios-sim/$FFI_FRAMEWORK_NAME" \
  -framework "$FRAMEWORK_BUILD_DIR/macos-universal/$FFI_FRAMEWORK_NAME" \
  -output "$XCFRAMEWORK_DIR"

echo "Created $XCFRAMEWORK_DIR"
