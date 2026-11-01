# Supplemental dependency notices

These are upstream license texts omitted from published crate archives. The
manifest maps each locked crate version to its `.cargo_vcs_info.json` revision,
immutable upstream URL and SHA-256. Packaging checks both revision and content;
new versions require deliberate review. Files are distribution assets, not
research output. Original terms and copyright statements are preserved.

Boa's MIT alternative is selected for the decoder distribution. SWC's published
crate VCS commit URLs returned 404, so those entries distinguish the checked
crate fingerprint from the license's immutable `v1.16.20` release commit. That
release's parser manifest confirms version 46.1.2. The provenance explanation is
also included in packaged notices; these entries do not claim the license was
retrieved from an unavailable crate commit.

The package also collects license/notice files supplied inside registry crates,
including bundled fonts, and includes checksum-verified, unmodified MPL crate
sources. Inventory covers a target-filtered workspace superset. This mechanism
does not select every SPDX alternative or complete a distribution/legal review.
