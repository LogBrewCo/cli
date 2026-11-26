# ClusterFuzzLite builds the fuzz targets in fuzz/ in this image.
FROM gcr.io/oss-fuzz-base/base-builder-rust:v1@sha256:8c2ccb74bb4b63c10c47e8f91c6633181fa00ca4cc4a4751dd390443e6cc76aa
# The image's nightly is older than the CLI's newest dependencies need.
ENV RUSTUP_TOOLCHAIN=nightly-2026-09-23
RUN rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal
COPY . $SRC/cli
WORKDIR $SRC/cli
COPY .clusterfuzzlite/build.sh $SRC/
