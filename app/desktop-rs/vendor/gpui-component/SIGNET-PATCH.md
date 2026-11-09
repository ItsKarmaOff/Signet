# Signet patch

This is gpui-component 0.9.0 from crates.io (Apache-2.0, see `LICENSE-APACHE`),
with two early returns added. Cargo.lock, tests and `Cargo.toml.orig` are left out.

- `src/theme/system_font.rs`, `resolve_default_font`: returns on macOS.
- `src/theme/mono_font.rs`, `resolve_default_mono_font`: returns on macOS.

Why: both probes call `TextSystem::all_font_names`, which enumerates every font
through CoreText and sorts them (about 50 ms at every launch). On macOS the
probes never change anything: `.SystemUIFont` resolves to itself and Menlo, the
default monospace family, is always installed. Linux and Windows keep the
probes, because they prevent a crash when a default font is missing.

Wired by `[patch.crates-io]` in the root `Cargo.toml`. Drop the directory and
that patch entry once upstream skips the enumeration (or enumerates lazily).
