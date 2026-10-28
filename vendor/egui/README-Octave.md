# Octave vendored renderer

Eight runtime crates from emilk/egui commit c93e8863e958507056415e2a6dace56e23ba5f3a, originally evaluated under upstream PR8577. This is a pinned patched snapshot, not the published0.36.2 release despite its upstream version label.

Source: https://github.com/emilk/egui/tree/c93e8863e958507056415e2a6dace56e23ba5f3a
Upstream license: MIT OR Apache-2.0. Both license texts are retained in each crate. Fonts retain their upstream font licenses.

Octave changes: the five-file packaging/rtl-candidate/octave-rtl-fixes.patch adds bidi line fixes, logical/visual cluster preservation, emoji/text family routing, color composition and grapheme selection/wrapping fixes. Additionally egui/src/containers/scene.rs removes the deprecated core::f32 module import; primitive f32::EPSILON is equivalent.

Manifests are normalized from the upstream workspace: inherited package/dependency/lint values are expanded; these eight crates use relative sibling paths; other optional dependencies resolve from the registry. Dev dependencies and explicit bench/example targets are omitted, automatic examples/benches disabled, publishing disabled, and each crate has its own workspace boundary. The Octave workspace tests the application and its renderer gates, not the entire upstream development workspace. No research-directory path is required to build.

Source integrity: SOURCE-HASHES.json records SHA256 of UTF-8 text normalized to LF (binary assets unchanged) for every retained upstream source/asset other than normalized Cargo.toml and added license copies. After changing renderer source, update the tracked patch/provenance and hashes. Shipping paths use the root Cargo.toml overrides and Cargo.lock. The Rust color_fonts feature is enabled explicitly; no JavaScript runtime is introduced.
