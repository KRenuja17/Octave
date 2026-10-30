# Octave MP4 startup patch

Based on the published symphonia-format-isomp4 0.5.5 crate, SHA-256
243739585d11f81daf8dac8d9f3d18cc7898f6c09a259675fc364b382c30e0a5.
Upstream: https://github.com/pdeljanov/Symphonia. License: MPL-2.0, retained in
LICENSE and source headers. The normalized registry Cargo.toml is retained.

Octave changes src/demuxer.rs to stop eager scanning at the first media fragment
when ftyp and a fragmented moov are available and prebuild_seek_index is false.
Subsequent packets and seeks retain the existing on-demand segment discovery.
Explicit eager indexing and nonfragmented/trailing-moov scans remain unchanged.
Fragmented-stream metadata located after the first media fragment is no longer
collected during initialization; movie-header metadata is retained.

Synthetic tests measure reads across 2,000 fragments and retain coverage of a
nonfragmented file whose moov follows media. No downloaded media is included.
Run: cargo test -p symphonia-format-isomp4 --locked octave_

The distribution's MPL-SOURCES.zip includes this exact modified source directory.
