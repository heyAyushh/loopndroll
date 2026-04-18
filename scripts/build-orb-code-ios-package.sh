#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$ROOT_DIR/crates/orb-code"
HEADER_DIR="$CRATE_DIR/include"
BUILD_DIR="$ROOT_DIR/.build/orb-code-ios"
IOS_TARGET_DIR="$BUILD_DIR/targets"
XCFRAMEWORK_DIR="$ROOT_DIR/ios/OrbCodeKit/Frameworks/OrbCodeFFI.xcframework"

rustup target add aarch64-apple-ios aarch64-apple-ios-sim

cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --target aarch64-apple-ios --release
cargo build --manifest-path "$CRATE_DIR/Cargo.toml" --target aarch64-apple-ios-sim --release

rm -rf "$XCFRAMEWORK_DIR"
mkdir -p "$IOS_TARGET_DIR"

xcodebuild -create-xcframework \
  -library "$CRATE_DIR/target/aarch64-apple-ios/release/liborb_code.a" \
  -headers "$HEADER_DIR" \
  -library "$CRATE_DIR/target/aarch64-apple-ios-sim/release/liborb_code.a" \
  -headers "$HEADER_DIR" \
  -output "$XCFRAMEWORK_DIR"

echo "Created $XCFRAMEWORK_DIR"
