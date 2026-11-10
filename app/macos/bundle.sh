#!/bin/bash
# Builds Notedd.app (Apple Silicon + Intel in one binary) and notedd-macos.zip in app/target/macos.
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

# the icon: the .ico's largest picture, in three sizes, packed as ICNS chunks (four-byte type, big-endian length)
sips -s format png ../assets/notedd.ico --out "$stage/ic08.png" >/dev/null
sips -z 32 32 "$stage/ic08.png" --out "$stage/icp5.png" >/dev/null
sips -z 128 128 "$stage/ic08.png" --out "$stage/ic07.png" >/dev/null
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
' "$stage"/{icp5,ic07,ic08}.png > "$app/Contents/Resources/Notedd.icns"

# ad hoc signature: Apple Silicon runs nothing unsigned; not notarized
codesign --force --sign - "$app"
codesign --verify --strict "$app"

rm -rf "$out"
mkdir -p "$out"
ditto "$app" "$out/Notedd.app"
ditto -c -k --keepParent "$app" "$out/notedd-macos.zip"
printf '%s\n' "$PWD/$out/Notedd.app" "$PWD/$out/notedd-macos.zip"
