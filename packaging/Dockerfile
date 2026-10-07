# Build environment for the Linux packages. Ubuntu 22.04 keeps the glibc requirement low,
# so the .deb and the AppImage run on it and on newer distributions.
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates file dpkg-dev \
        clang libclang-dev liblcms2-dev libjpeg-turbo8-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
# LibRaw from source, the same version as on current distributions (Ubuntu 22.04's 0.20
# renders colours differently and knows fewer cameras). Built static only, with OpenMP,
# libjpeg (lossy DNG), LittleCMS and zlib (deflate DNG).
ARG LIBRAW_VERSION=0.22.2
ARG LIBRAW_SHA256=de86b035655accff8d4010f1a221fdf50d353cb7b1422ba26f14a0db92612cfa
RUN curl -sSfL -o /tmp/libraw.tar.gz https://www.libraw.org/data/LibRaw-${LIBRAW_VERSION}.tar.gz \
    && echo "${LIBRAW_SHA256}  /tmp/libraw.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/libraw.tar.gz -C /tmp && cd /tmp/LibRaw-${LIBRAW_VERSION} \
    && ./configure --prefix=/opt/libraw --enable-static --disable-shared --disable-examples \
        --enable-openmp --enable-jpeg --enable-lcms --enable-zlib >/dev/null \
    && make -j"$(nproc)" >/dev/null && make install >/dev/null \
    && rm -rf /tmp/libraw.tar.gz /tmp/LibRaw-${LIBRAW_VERSION}
ENV PKG_CONFIG_PATH=/opt/libraw/lib/pkgconfig
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install cargo-deb --locked
# linuxdeploy (with its AppImage output plugin), unpacked so it runs without FUSE.
RUN curl -sSfL -o /tmp/linuxdeploy.AppImage \
        https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage \
    && chmod +x /tmp/linuxdeploy.AppImage \
    && cd /opt && /tmp/linuxdeploy.AppImage --appimage-extract >/dev/null \
    && mv squashfs-root linuxdeploy && ln -s /opt/linuxdeploy/AppRun /usr/local/bin/linuxdeploy \
    && rm /tmp/linuxdeploy.AppImage
