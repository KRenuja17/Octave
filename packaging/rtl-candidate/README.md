# Isolated RTL renderer patch

`octave-rtl-fixes.patch` applies to egui candidate commit
`c93e8863e958507056415e2a6dace56e23ba5f3a` (upstream PR 8577), not the published
0.36.2 crates. It is retained for isolated evaluation and is not applied by
Octave's Cargo configuration or packaging scripts.

## Automatic routing and wrapping checkpoint — 28 October 2026

Fourteen explicit gates now pass. Octave registers the bundled, unmodified Noto
font in `Octave Emoji`, separate from the normal proportional chain. The isolated
renderer routes complete emoji-presentation graphemes to that family and explicit
VS15 text presentation to `Octave Text` (which excludes the color emoji face).
Ordinary text, digits and spaces keep their metrics even after emoji cache warmup.
Routing applies only to proportional text; explicit/custom and monospace families
keep their own policy. Missing named families leave the original path intact.

The row breaker now only records grapheme boundaries. Oversized graphemes overrun
the requested width rather than detaching continuation characters onto another
row. Cross-row tests cover widths25/80/160, explicit newlines, mixed RTL text,
combining marks and joined emoji, including partial selections and unselected
neighbors. Painting preserves logical copy ranges. Native inspection uses only
the actual Octave theme and normal proportional labels, with no fixture font
overrides, and confirms flags, monochrome VS15/color VS16 and cross-row highlights.

The 10,730,124-byte font, copyright/provenance notice and OFL license are now in
`apps/octave-desktop/assets/fonts`. `Build-Package.ps1` appends both notices to
distribution notices; its exact notice block was executed and checked. No package
was built in this pass. The prior external font environment variable is no longer
required by the gates. Color features and the patched renderer remain isolated;
shipping dependency adoption, keyboard/IME and screen-reader qualification remain
open. Font registration alone does not enable the new routing in published egui.

## Color and selection checkpoint — 28 October 2026

The latest isolated patch passes twelve explicit renderer gates. It composites
multi-glyph color clusters into one cached bitmap, retains logical continuations,
expands partial-grapheme selection backgrounds to the visible grapheme, and avoids
tinting color glyphs with selection foreground colors. Painting does not change
the logical cursor or copied-text range. Cross-row graphemes and editing/navigation
semantics still need separate qualification.

For the color gates, add `epaint = { version = "=0.36.2", features = ["color_fonts"] }`
to the **external snapshot's** desktop dependencies, using the existing candidate
override. Resolve its external lockfile offline. Current gates use the bundled
font described above. The color feature remains external to shipping dependencies.

The test font is from [Noto Emoji](https://github.com/googlefonts/noto-emoji/tree/99701e07d3d1fe1bdbcdb14964547f0fd592966f),
`2D/fonts/NotoColorEmoji.ttf`, SHA256
`15671215ab769fdc7162a045d56fd7d7e477c51b04e6b3c761d914d8fdd6cc44`.
At that checkpoint, its adjacent SIL OFL license and source metadata were retained
with the research font outside the project. The subsequent routing checkpoint
bundles the unmodified asset and notices; no system font installation is performed.

Native inspection confirms colored joined emoji and Sri Lankan/Japanese/US flags,
including an unmodified-color selected emoji and a composed Segoe family. The final
capture scopes Noto to fixture rows: making it the global first font altered normal
spacing/digits. Production emoji font routing, distribution/licensing integration,
cross-platform coverage, IME, screen-reader behavior and optimized performance
remain open. These results qualify an isolated renderer, not a shipping release.

## Earlier checkpoints

The patch renders multi-glyph clusters into a single bitmap while retaining their
logical characters, applies Unicode L1 levels after wrapping, and paints separated
visual selection spans instead of one rectangle between logical endpoints.

In a separate checkout of the exact candidate, run `git apply --check` followed
by `git apply` with the absolute patch path. Point an external Octave snapshot's
egui-family overrides to that checkout. The candidate also requires screenshot
callback API adaptation; it is not a drop-in shipping dependency replacement.

Run the explicit tests independently of the ordinary suite:

```text
cargo test -p octave-desktop --locked --offline rtl_renderer_ -- --ignored --nocapture
```

All six original gates passed on the patched Windows evaluation: basic ordering, Hebrew
cursor round trips, Arabic marked/wrapped logical text, wrapped whitespace, the
original mixed-selection failure and every selection range in three short fixtures.
The native fixture capture was also inspected. These are focused regressions,
not complete Unicode or screen-reader acceptance. Color/missing-glyph cluster
fallbacks, selection within a combining cluster, complex paragraph/ellipsis cases,
font diversity and large-text performance still need broader qualification.
The initial 27 October expansion passed nine of ten explicit checks. Mixed paragraphs,
directional isolates and installed fallback coverage pass. The emoji ZWJ fallback
fails logical-text preservation (`👩‍💻` becomes `👩👩💻`); the native capture also
showed missing emoji glyphs. Those failed results remain retained.

The subsequent emoji repair passes all ten explicit checks. A cluster containing
missing glyphs now emits one fallback unit and logical continuation characters,
instead of repeating its first character for leftover shaped glyphs. Octave loads
the installed Segoe UI Emoji font after Segoe UI and before bundled symbol fonts;
no font files are redistributed. The expanded gate checks family/ZWJ, skin tone,
flag, keycap, variation selector and deliberately unsupported sequences with both
installed and bundled-only fonts, across three widths. Logical text, cursor-end
counts and zero-width joiner/variation continuations pass.

Native capture shows monochrome emoji instead of the previous boxes for the
woman-at-computer/music fixtures. Flags appear as letter codes. Full-color cluster
composition, flag imagery and visual behavior across other font sets remain open;
these checks do not certify every emoji or selection within a grapheme.

L1 now copies only row-sized resolved levels/classes and advances the paragraph
index monotonically. An extracted Rust oracle test matches the original Unicode
routine over all character-boundary subranges of five fixtures, including isolates,
embeddings, tabs and paragraph breaks. The test is also retained in the patch.
Unique uncached workloads reach 62,890 characters / 6,001 rows: debug layout took
about 1.4–1.5 seconds on this machine. Earlier repeated-paragraph comparisons show
no convincing end-to-end speedup. These are diagnostic timings, not a release
performance acceptance result; optimized-build and interactive profiling remain open.

Upstream source is MIT OR Apache-2.0; the included context is used under MIT.
Its copyright and permission notice are preserved in `LICENSE-MIT.egui`.
The original candidate, failed results, patched source and native captures are
retained outside the project in the RTL research directory. Shipping dependencies,
the Octave palette, and the evaluation package remain unchanged.
