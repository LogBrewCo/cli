#!/bin/bash -eu
# Builds every fuzz target in fuzz/ with the sanitizer flags ClusterFuzzLite sets.
cd "$SRC/cli"
cargo fuzz build -O
for target in $(cargo fuzz list); do
  cp "fuzz/target/x86_64-unknown-linux-gnu/release/$target" "$OUT/"
done
