#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p target
stage=$(mktemp -d "$PWD/target/build.XXXXXX")
trap 'rm -rf "$stage"' EXIT

app="$stage/Notedd.app"
resources="$app/Contents/Resources"
mkdir -p "$app/Contents/MacOS" "$resources"
cp Info.plist "$app/Contents/Info.plist"
cp ../assets/fonts/{Geologica.ttf,OFL-Geologica.txt} "$resources/"

sips -s format png ../assets/notedd.ico --out "$stage/ic08.png" >/dev/null
sips -z 32 32 "$stage/ic08.png" --out "$stage/icp5.png" >/dev/null
sips -z 128 128 "$stage/ic08.png" --out "$stage/ic07.png" >/dev/null
# ICNS stores each PNG in a chunk with a four-byte type and big-endian length.
perl -e '
    my $chunks = "";
    for my $path (@ARGV) {
        open my $file, "<:raw", $path or die "$path: $!";
        my $png = do { local $/; <$file> };
        my ($type) = $path =~ m{/([a-z0-9]{4})\.png$};
        $chunks .= pack("a4N", $type, 8 + length $png) . $png;
    }
    binmode STDOUT;
    print pack("a4N", "icns", 8 + length $chunks), $chunks;
' "$stage"/{icp5,ic07,ic08}.png > "$resources/Notedd.icns"

setup="$stage/Notedd Setup.app"
setup_resources="$setup/Contents/Resources"
mkdir -p "$setup/Contents/MacOS" "$setup_resources"
cp Setup/Info.plist "$setup/Contents/Info.plist"
for key in CFBundleShortVersionString CFBundleVersion LSMinimumSystemVersion; do
    plutil -insert "$key" -string "$(plutil -extract "$key" raw "$app/Contents/Info.plist")" "$setup/Contents/Info.plist"
done
cp "$resources"/{Geologica.ttf,OFL-Geologica.txt,Notedd.icns} "$setup_resources/"
cp -R Setup/*.lproj "$setup_resources/"

for arch in arm64 x86_64; do
    rust_arch="$arch"
    if [[ "$arch" == arm64 ]]; then rust_arch=aarch64; fi
    rust_target="$rust_arch-apple-darwin"
    library="$PWD/target/rust/$rust_target/release/libnotedd_core.a"
    if [[ "${NOTEDD_USE_PREBUILT_CORE:-0}" == 1 ]]; then
        if [[ ! -f "$library" ]] || find ../core/src ../core/Cargo.toml ../core/Cargo.lock -type f -newer "$library" -print -quit | grep -q .; then
            printf 'Prebuilt Rust library for %s is missing or older than the core sources. Install Cargo and run a normal build.\n' "$rust_target" >&2
            exit 1
        fi
        printf 'Using prebuilt Rust library: %s\n' "$library"
    else
        MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --locked --release --manifest-path ../core/Cargo.toml \
            --features macos-bridge --target "$rust_target" --target-dir "$PWD/target/rust"
    fi
    xcrun swiftc -O -target "$arch-apple-macosx13.0" -module-cache-path "$PWD/target/module-cache" \
        -import-objc-header Bridge/NoteddCore.h -o "$stage/Notedd-$arch" Sources/*.swift \
        "$library"
    xcrun swiftc -O -target "$arch-apple-macosx13.0" -module-cache-path "$PWD/target/module-cache" \
        -o "$stage/Setup-$arch" Setup/*.swift Sources/{FolderChrome,FolderWindow,Geologica,Motion,Shortcuts}.swift
done
lipo -create "$stage/Notedd-arm64" "$stage/Notedd-x86_64" -output "$app/Contents/MacOS/Notedd"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
ditto -c -k --keepParent "$app" "$setup_resources/Notedd-payload.zip"

lipo -create "$stage/Setup-arm64" "$stage/Setup-x86_64" -output "$setup/Contents/MacOS/Notedd Setup"
codesign --force --sign - "$setup"
codesign --verify --strict "$setup"
ditto -c -k --keepParent "$setup" "$stage/notedd-setup-macos.zip"

ditto "$app" target/Notedd.app
ditto "$setup" "target/Notedd Setup.app"
mv "$stage/notedd-setup-macos.zip" target/notedd-setup-macos.zip
printf '%s\n' "$PWD/target/Notedd.app" "$PWD/target/Notedd Setup.app" "$PWD/target/notedd-setup-macos.zip"
