# Pinned libtorrent metainfo profiler

This independently authored adapter runs Tactical 081's generated bencode
fixtures through the libtorrent revision pinned in `reference/pins.toml`. It
uses libtorrent's caller-owned span path for `explicit` inputs and its BEP 9
decode limits for raw `peer` info dictionaries.

Build the pinned static library and this adapter in disposable build
directories:

```bash
git -C reference/libtorrent submodule update --init deps/try_signal
cmake -S reference/libtorrent -B /tmp/rstorrent-libtorrent-build -G Ninja \
  -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
  -Dbuild_tests=OFF -Dbuild_examples=OFF -Dbuild_tools=OFF \
  -Dpython-bindings=OFF -Ddeprecated-functions=OFF
cmake --build /tmp/rstorrent-libtorrent-build \
  --target torrent-rasterbar
cmake -S scripts/libtorrent-metainfo-profile \
  -B /tmp/rstorrent-libtorrent-profile-build -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DLibtorrentRasterbar_DIR=/tmp/rstorrent-libtorrent-build/LibtorrentRasterbar
cmake --build /tmp/rstorrent-libtorrent-profile-build
```

Build RSTorrent's fixture generator and profiler, generate an input once, and
pass the same bytes to both subprocesses:

```bash
cargo build --release -p rstorrent-engine \
  --bin rstorrent-metainfo-compare
target/release/rstorrent-metainfo-compare generate \
  structure-outer /tmp/structure.torrent 2999984
target/release/rstorrent-metainfo-compare profile \
  explicit /tmp/structure.torrent
/tmp/rstorrent-libtorrent-profile-build/libtorrent-metainfo-profile \
  explicit /tmp/structure.torrent
```

The adapter is comparison tooling, not a runtime dependency. Generated
fixtures and build output remain outside the repository.
