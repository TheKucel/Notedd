#!/bin/bash
# Builds Notedd.app (Apple Silicon + Intel in one binary), its installer Notedd Setup.app and
# notedd-setup-macos.zip (the one file a release ships for the Mac) in app/target/macos.
# Needs Xcode or its Command Line Tools, and: rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=target/macos
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

export MACOSX_DEPLOYMENT_TARGET=13.0
for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$t"
done

app="$stage/Notedd.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/VERSION/$version/g" macos/Info.plist > "$app/Contents/Info.plist"
lipo -create target/aarch64-apple-darwin/release/notedd target/x86_64-apple-darwin/release/notedd \
    -output "$app/Contents/MacOS/Notedd"

# Supply every standard and Retina size so Finder can choose the right representation.
iconset="$stage/Notedd.iconset"
mkdir -p "$iconset"
sips -s format png ../assets/notedd.ico --out "$stage/icon.png" >/dev/null
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$stage/icon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$stage/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Notedd.icns"

# ad hoc signature: Apple Silicon runs nothing unsigned; not notarized
codesign --force --sign - "$app"
codesign --verify --strict "$app"

setup="$stage/Notedd Setup.app"
setup_resources="$setup/Contents/Resources"
mkdir -p "$setup/Contents/MacOS" "$setup_resources"
cp macos/installer/Info.plist "$setup/Contents/Info.plist"
for key in CFBundleShortVersionString CFBundleVersion LSMinimumSystemVersion; do
    plutil -insert "$key" -string "$(plutil -extract "$key" raw "$app/Contents/Info.plist")" "$setup/Contents/Info.plist"
done
cp "$app/Contents/Resources/Notedd.icns" "$setup_resources/"
cp ../assets/fonts/OFL-Geologica.txt "$setup_resources/"
ditto -c -k --keepParent "$app" "$setup_resources/Notedd-payload.zip"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked --target "$target" --features macos-installer --bin notedd-setup-macos
done
lipo -create target/aarch64-apple-darwin/release/notedd-setup-macos target/x86_64-apple-darwin/release/notedd-setup-macos -output "$setup/Contents/MacOS/Notedd Setup"
codesign --force --sign - "$setup"
codesign --verify --strict "$setup"
NOTEDD_SETUP_TEST_DIR="$stage/self-test" "$setup/Contents/MacOS/Notedd Setup" --self-test

rm -rf "$out"
mkdir -p "$out"
ditto "$app" "$out/Notedd.app"
ditto "$setup" "$out/Notedd Setup.app"
ditto -c -k --keepParent "$setup" "$out/notedd-setup-macos.zip"
printf '%s\n' "$PWD/$out/Notedd.app" "$PWD/$out/Notedd Setup.app" "$PWD/$out/notedd-setup-macos.zip"
