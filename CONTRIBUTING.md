# Contributing

Use Conventional Commits for new commits: `type(scope): description`.
Write an imperative, lowercase description without a trailing period, and aim
for a subject of 72 characters or fewer. Examples:

- `fix(player): preserve the consumed position on retry`
- `test(player): add a real-time muted playback soak`
- `docs(release): record playback reliability evidence`

Use `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, or `chore` as
appropriate. Use a specific scope such as `player`, `provider`, `desktop`, or
`packaging`. Separate an optional body from the subject with a blank line;
explain the behavior, reason, and relevant validation, with lines wrapped near
72 characters. Mark breaking changes with `!` and explain them in the body.

Keep the configured Git identity. Do not add generated co-author attribution or
rewrite existing history without an explicit request. Keep research, profiles,
builds, packages, and measurement logs outside the source repository.
