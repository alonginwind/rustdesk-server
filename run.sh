#!/bin/bash
if [ "$1" == "linux" ]; then
    cargo build --release --all-features
elif [ "$1" == "windows" ]; then
    if [ ! -f "target/libsodium/build/lib/libsodium.a" ]; then
        mkdir -p target
        ./sodium.sh
    fi
    export SODIUM_LIB_DIR=$(pwd)/target/libsodium/build/lib
    cargo build --release --all-features --target=x86_64-pc-windows-gnu
fi
