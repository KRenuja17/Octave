# Octave MP4 startup and indexed-seek patches

Based on the published symphonia-format-isomp4 0.5.5 crate, SHA-256
243739585d11f81daf8dac8d9f3d18cc7898f6c09a259675fc364b382c30e0a5.
Upstream: https://github.com/pdeljanov/Symphonia. License: MPL-2.0, retained in
LICENSE and source headers. The normalized registry Cargo.toml is retained.

Octave changes src/demuxer.rs to stop eager scanning at the first media fragment
when ftyp and a fragmented moov are available and prebuild_seek_index is false.
Subsequent packets retain on-demand segment discovery.
Explicit eager indexing and nonfragmented/trailing-moov scans remain unchanged.
Fragmented-stream metadata located after the first media fragment is no longer
collected during initialization; movie-header metadata is retained.

Flat sidx indexes now accelerate seeks for seekable, single-track AAC streams
with an empty movie sample table, matching track ID/time base and zero index
origin. Checked offsets/durations must stay inside the resource. The selected
moof/mdat pair must match the reference duration/extent and contain every sample.
Each jump resets the local sample sequence while retaining absolute timestamps.
Unsupported fragment layouts restore the iterator and use sequential discovery;
transport failures are not retried. Nested indexes and other codecs/tracks keep
the existing path. The Sidx parser rejects offset arithmetic overflow.

Five synthetic tests cover bounded startup/seek reads across 2,000 fragments,
forward/backward seeks, packet bytes/timestamps, continuation across fragments,
inconsistent-index fallback, nested/truncated/overflowing references and trailing
movie metadata. No downloaded media is included.
Run: cargo test -p symphonia-format-isomp4 --locked octave_

The distribution's MPL-SOURCES.zip includes this exact modified source directory.
