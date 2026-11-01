# Octave player preprocessor subset

Source: https://github.com/ahaoboy/ytdlp-ejs
Revision: `11fdb193d7be383fd0bb26c00e20faa1ca281263` (MIT, copyright 2026 ahaoboy).

Only `src/builtin/preprocessor/{mod.rs,extract_shared.rs}` and the two browser
polyfill strings are retained. Octave removes tracing and its input previews,
changes module paths, pins SWC dependencies and supplies a minimal error type.
No upstream CLI, runtime adapters, tests, downloads or external-runtime features
are included. The original MIT license is retained. Google player JavaScript is
not redistributed.

This subset parses and rewrites public JavaScript; it does not execute it.
`octave-signature` owns the separate process and restricted Boa context. Public
upstream diagnostics must not cross that process's fixed error boundary.
