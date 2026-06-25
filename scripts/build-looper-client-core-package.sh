#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$ROOT_DIR/crates/looper-client-core"
PACKAGE_DIR="$ROOT_DIR/swift/LooperClientCore"
BINDINGS_DIR="$PACKAGE_DIR/Sources/LooperClientCore/Generated"
FRAMEWORKS_DIR="$PACKAGE_DIR/Frameworks"
XCFRAMEWORK_DIR="$FRAMEWORKS_DIR/LooperClientCoreFFI.xcframework"
HOST_LIBRARY="target/release/liblooper_client_core.dylib"

IOS_TARGETS=(
  aarch64-apple-ios
  aarch64-apple-ios-sim
)

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

for target in "${IOS_TARGETS[@]}"; do
  rustup target add "$target"
  cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --target "$target" --release
done

mkdir -p "$FRAMEWORKS_DIR"
if [[ -e "$XCFRAMEWORK_DIR" ]]; then
  echo "Replacing $XCFRAMEWORK_DIR"
  rm -R "$XCFRAMEWORK_DIR"
fi

echo "Creating $XCFRAMEWORK_DIR"
xcodebuild -create-xcframework \
  -library "$CRATE_DIR/target/aarch64-apple-ios/release/liblooper_client_core.a" \
  -headers "$BINDINGS_DIR" \
  -library "$CRATE_DIR/target/aarch64-apple-ios-sim/release/liblooper_client_core.a" \
  -headers "$BINDINGS_DIR" \
  -output "$XCFRAMEWORK_DIR"

echo "Created $XCFRAMEWORK_DIR"
