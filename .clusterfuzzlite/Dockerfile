# ClusterFuzzLite builds the fuzz targets in fuzz/ in this image.
FROM gcr.io/oss-fuzz-base/base-builder-rust:v1@sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f
# The image's nightly is older than the CLI's newest dependencies need.
ENV RUSTUP_TOOLCHAIN=nightly-2026-09-23
RUN rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal
COPY . $SRC/cli
WORKDIR $SRC/cli
COPY .clusterfuzzlite/build.sh $SRC/
