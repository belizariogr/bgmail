# crates/theme/src/ — Source API

## Purpose

Source for the `theme` crate: appearance, color roles, built-in palettes, and active-theme globals for GPUI.

## Ownership

- Owns: `src/theme.rs` API surface.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/theme/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with purpose
  and behavior. Update the matching entry when changing signatures or semantics.
- Prefer rustdoc on public items; DOX purpose/behavior should stay aligned with
  rustdoc.
- Visibility in entries (`pub` / `private`) reflects the source item.

## Work Guidance

- After adding/removing/renaming a function, update this catalog in the same
  change.
- Do not weaken parent DOX contracts from root/`crates/`/`theme/`.
- Colors are declared only here; components consume them via `ui::Color` and
  `ActiveTheme`, never raw hex in UI code.

## Verification

- `cargo test -p theme`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

_19 functions/methods documented._

### `src/theme.rs`

#### Types / constants

- **enum `Appearance`**: Light or dark theme mode; used to pick a built-in palette and to toggle appearance at runtime.
- **struct `ThemeColors`**: Flat set of semantic `Hsla` color roles (background, borders, text, icons, accent, chrome, scrollbar, status). Field names mirror Zed conventions for component portability.
- **struct `Theme`**: Named built-in theme: static `name`, `appearance`, and embedded `ThemeColors`.
- **struct `GlobalTheme`**: GPUI global wrapper around `Arc<Theme>`; registered on `App` via `init` / `set_theme`.
- **trait `ActiveTheme`**: Read-only access to the active `Arc<Theme>` from any `App` context.

#### Functions / methods

##### Context: `module`

- **`hex`** (private, L24)
  - Signature: `fn hex(value: u32) -> Hsla`
  - Purpose: Converts a 24-bit RGB hex literal into GPUI `Hsla`.
  - Behavior: Calls `rgb(value).into()` with full opacity. Used throughout palette definitions so colors read like VSCode theme hex values. Does not panic.

- **`hexa`** (private, L31)
  - Signature: `fn hexa(value: u32, alpha: f32) -> Hsla`
  - Purpose: Like `hex`, but sets a custom alpha channel for translucent colors.
  - Behavior: Builds `Hsla { a: alpha, ..hex(value) }`. Used for scrollbar thumb colors. Does not panic.

- **`init`** (pub, L261)
  - Signature: `pub fn init(appearance: Appearance, cx: &mut App)`
  - Purpose: Bootstraps the theme global on a new `App` instance.
  - Behavior: Resolves `Theme::for_appearance(appearance)`, wraps it in `Arc`, and stores it as `GlobalTheme` via `cx.set_global`. Does not refresh windows (call before windows exist or call `set_theme` later for live updates).

- **`set_theme`** (pub, L266)
  - Signature: `pub fn set_theme(theme: Theme, cx: &mut App)`
  - Purpose: Replaces the active theme at runtime.
  - Behavior: Overwrites the `GlobalTheme` with a new `Arc<Theme>`, then calls `cx.refresh_windows()` so every open window redraws immediately (views read the global at render time but do not observe it). Does not panic.

- **`toggle_appearance`** (pub, L275)
  - Signature: `pub fn toggle_appearance(cx: &mut App) -> Appearance`
  - Purpose: Switches between light and dark built-in themes.
  - Behavior: Reads current appearance from `GlobalTheme`, calls `toggled()`, applies `Theme::for_appearance(next)` via `set_theme`, and returns the new `Appearance`. Side effect: all windows refresh.

##### Context: `Appearance`

- **`is_light`** (pub, L49)
  - Signature: `pub fn is_light(self) -> bool`
  - Purpose: Reports whether this appearance is light mode.
  - Behavior: Returns `true` only for `Appearance::Light`. Pure predicate; no side effects.

- **`toggled`** (pub, L54)
  - Signature: `pub fn toggled(self) -> Self`
  - Purpose: Returns the opposite appearance for theme-toggle UI.
  - Behavior: Maps `Light → Dark` and `Dark → Light`. Does not mutate global state.

##### Context: `Theme`

- **`colors`** (pub, L146)
  - Signature: `pub fn colors(&self) -> &ThemeColors`
  - Purpose: Borrows the theme's color palette.
  - Behavior: Returns `&self.colors`. No allocation or side effects.

- **`appearance`** (pub, L152)
  - Signature: `pub fn appearance(&self) -> Appearance`
  - Purpose: Returns the theme's light/dark mode.
  - Behavior: Copies `self.appearance`. No side effects.

- **`dark`** (pub, L157)
  - Signature: `pub fn dark() -> Self`
  - Purpose: Built-in dark palette based on VSCode Dark Modern.
  - Behavior: Constructs a static `Theme` named `"BGMail Dark"` with `Appearance::Dark` and a full `ThemeColors` block (dark grays, blue accent `0x0078d4`, translucent scrollbar thumbs, semantic success/warning/error). No I/O; deterministic each call.

- **`light`** (pub, L202)
  - Signature: `pub fn light() -> Self`
  - Purpose: Built-in light palette based on VSCode Light Modern.
  - Behavior: Constructs a static `Theme` named `"BGMail Light"` with `Appearance::Light` and a full `ThemeColors` block (white/light-gray surfaces, blue accent `0x005fb8`, lighter scrollbar alphas, semantic status colors). No I/O; deterministic each call.

- **`for_appearance`** (pub, L247)
  - Signature: `pub fn for_appearance(appearance: Appearance) -> Self`
  - Purpose: Selects the built-in theme for a given appearance.
  - Behavior: `Light → Theme::light()`, `Dark → Theme::dark()`. No side effects.

##### Context: `ActiveTheme`

- **`theme`** (private, L284)
  - Signature: `fn theme(&self) -> &Arc<Theme>`
  - Purpose: Trait hook for reading the active theme from a GPUI context.
  - Behavior: Implementors return a shared reference to the current `Arc<Theme>`.

##### Context: `ActiveTheme for App`

- **`theme`** (private, L288)
  - Signature: `fn theme(&self) -> &Arc<Theme>`
  - Purpose: GPUI `App` implementation of active-theme access.
  - Behavior: Returns `&self.global::<GlobalTheme>().0`. Panics if `init` was never called (missing global). Enables `cx.theme().colors().background` in components.

##### Context: `tests` (`#[cfg(test)]`)

- **`appearance_toggles`** (private, L298)
  - Signature: `fn appearance_toggles()`
  - Purpose: Regression test for `Appearance` helpers.
  - Behavior: Asserts `Light.toggled() == Dark`, `Dark.toggled() == Light`, `Light.is_light()`, and `!Dark.is_light()`. Panics on failure.

- **`builtin_themes_have_matching_appearance`** (private, L306)
  - Signature: `fn builtin_themes_have_matching_appearance()`
  - Purpose: Ensures built-in themes report correct appearance and names.
  - Behavior: Asserts `Theme::dark()` has `Appearance::Dark` and name `"BGMail Dark"`; `Theme::light()` has `Appearance::Light` and name `"BGMail Light"`. Panics on failure.

- **`for_appearance_returns_correct_theme`** (private, L314)
  - Signature: `fn for_appearance_returns_correct_theme()`
  - Purpose: Ensures `for_appearance` delegates to the correct built-in theme.
  - Behavior: Asserts equality with `Theme::dark()` and `Theme::light()` for each appearance. Panics on failure.

- **`dark_and_light_differ`** (private, L320)
  - Signature: `fn dark_and_light_differ()`
  - Purpose: Guards against accidental palette collapse between modes.
  - Behavior: Asserts dark and light `background` colors differ. Panics on failure.

- **`hex_conversion_is_stable`** (private, L328)
  - Signature: `fn hex_conversion_is_stable()`
  - Purpose: Sanity-checks hex-to-HSLA lightness mapping.
  - Behavior: Asserts `hex(0xffffff).l > 0.99` and `hex(0x000000).l < 0.01`. Panics on failure.
