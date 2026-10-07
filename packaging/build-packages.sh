#!/usr/bin/env bash
# Builds the Linux packages (.deb and AppImage) in an Ubuntu 22.04 container:
#   packaging/build-packages.sh      ->  dist/project-iris_<version>_amd64.deb
#                                         dist/project-iris-<version>-x86_64.AppImage
# Needs Docker. Downloads and build output are cached in Docker volumes between runs.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
docker build -t project-iris-packager "$repo/packaging"
docker run --rm \
    -v "$repo:/src" \
    -v project-iris-cargo-registry:/root/.cargo/registry \
    -v project-iris-build:/build \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    project-iris-packager /src/packaging/package.sh
