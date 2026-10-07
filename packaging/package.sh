#!/usr/bin/env bash
# Runs inside the packaging container (see build-packages.sh): builds Project Iris with
# LibRaw linked statically and writes the .deb and the AppImage to dist/.
set -euo pipefail
cd /src
export CARGO_TARGET_DIR=/build/target LIBRAW_STATIC=1
# Packages ship stripped binaries without debug info.
export CARGO_PROFILE_RELEASE_DEBUG=false CARGO_PROFILE_RELEASE_STRIP=true
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

cargo build --release --locked -p iris-app -p iris-cli
mkdir -p dist

# .deb: dependencies are worked out from the binaries (dpkg-shlibdeps).
cargo deb -p iris-app --no-build --locked --output "dist/project-iris_${version}_amd64.deb"

# AppImage: graphics drivers, Wayland/X11 and glibc come from the host.
appdir=/build/AppDir
rm -rf "$appdir"
export LINUXDEPLOY_OUTPUT_VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1
linuxdeploy --appdir "$appdir" \
    --executable "$CARGO_TARGET_DIR/release/iris" \
    --executable "$CARGO_TARGET_DIR/release/iris-cli" \
    --desktop-file packaging/project-iris.desktop \
    --icon-file packaging/project-iris.png \
    --icon-file packaging/project-iris.svg \
    --output appimage
mv Project_Iris-*-x86_64.AppImage "dist/project-iris-${version}-x86_64.AppImage"

chown -R "${HOST_UID:-0}:${HOST_GID:-0}" dist
ls -l dist
