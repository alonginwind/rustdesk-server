#!/bin/bash
if [ ! -d "target/libsodium" ]; then
    cp -r ~/.cargo/registry/src/index.crates.io-*/libsodium-sys-0.2.7/libsodium/ target/
fi
pushd target/libsodium/
./configure --host=x86_64-w64-mingw32 --enable-static --disable-shared --prefix=$(pwd)/build
make -j$(nproc)
make install
popd
