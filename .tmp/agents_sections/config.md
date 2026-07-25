### `src/config.rs`

#### Types / constants

- **`Config`** (pub, L22)
  - Signature: `pub struct Config { window_x, window_y, window_width, window_height, maximized, max_x, max_y, max_width, max_height, sidebar_width, list_width, load_remote_images, reader_white_background, compose_white_background, collapsed_accounts, compose_x, compose_y, compose_width, compose_height }`
  - Purpose: Serializable persisted layout and privacy settings (logical pixels).
  - Behavior: Loaded from `~/.config/BGMail/config.json`. `#[serde(default)]` fills missing fields from `Default`. Tracks main-window geometry (restored and maximized frames), column widths, remote-image opt-in, reader/compose white-background prefs, collapsed sidebar account indices, and compose-window bounds.

#### Functions / methods

##### Context: `Default for Config`

- **`default`** (private, L71)
  - Signature: `fn default() -> Self`
  - Purpose: Supplies built-in defaults matching the app's initial layout.
  - Behavior: Sets main window 1100×720 at origin, sidebar 200px, list 360px, privacy flags off, empty collapsed list, and compose bounds centered-unset with default compose size from `compose` constants.

##### Context: `module`

- **`home_dir`** (private, L100)
  - Signature: `fn home_dir() -> Option<PathBuf>`
  - Purpose: Resolves the user's home directory without extra dependencies.
  - Behavior: Reads `HOME` (Linux/macOS) or falls back to `USERPROFILE` (Windows).

- **`config_path`** (pub, L108)
  - Signature: `pub fn config_path() -> Option<PathBuf>`
  - Purpose: Returns the fixed cross-platform config file path.
  - Behavior: Joins home with `.config/BGMail/config.json`; returns `None` when home cannot be determined.

- **`load`** (pub, L113)
  - Signature: `pub fn load() -> Config`
  - Purpose: Loads settings from disk with safe fallback.
  - Behavior: Reads via `config_path` and `load_from`; returns `Config::default()` when path or parse fails.

- **`save`** (pub, L118)
  - Signature: `pub fn save(config: &Config)`
  - Purpose: Persists settings best-effort.
  - Behavior: Writes through `save_to` when `config_path` exists; silently ignores errors.

- **`load_from`** (private, L125)
  - Signature: `fn load_from(path: PathBuf) -> Config`
  - Purpose: Parses one config file path.
  - Behavior: Reads UTF-8 text, deserializes JSON to `Config`, or returns defaults on any failure.

- **`save_to`** (private, L133)
  - Signature: `fn save_to(path: &Path, config: &Config) -> std::io::Result<()>`
  - Purpose: Writes pretty JSON, creating parent directories.
  - Behavior: `create_dir_all` on parent, serializes with `serde_json::to_string_pretty`, then writes the file.

- **`config_path_uses_fixed_dot_config_location`** (private, L147)
  - Signature: `fn config_path_uses_fixed_dot_config_location()` (test)
  - Purpose: Asserts config lives under `.config/BGMail/config.json`.
  - Behavior: When `config_path()` is `Some`, checks suffix and `.config` segment.

- **`json_round_trips`** (private, L155)
  - Signature: `fn json_round_trips()` (test)
  - Purpose: Verifies full struct serde round-trip.
  - Behavior: Serializes a populated `Config` and deserializes back with equality.

- **`remote_images_are_blocked_by_default`** (private, L183)
  - Signature: `fn remote_images_are_blocked_by_default()` (test)
  - Purpose: Ensures privacy default for remote images.
  - Behavior: Default config and partial JSON without the field keep `load_remote_images == false`.

- **`collapsed_accounts_default_empty_and_round_trip`** (private, L192)
  - Signature: `fn collapsed_accounts_default_empty_and_round_trip()` (test)
  - Purpose: Validates collapsed-account persistence field.
  - Behavior: Default is empty; partial JSON preserves stored indices.

- **`compose_white_background_defaults_off`** (private, L199)
  - Signature: `fn compose_white_background_defaults_off()` (test)
  - Purpose: Ensures compose white background defaults off.
  - Behavior: Default and partial JSON keep the flag false.

- **`reader_white_background_defaults_off`** (private, L206)
  - Signature: `fn reader_white_background_defaults_off()` (test)
  - Purpose: Ensures reader white background defaults off.
  - Behavior: Default and partial JSON keep the flag false.

- **`missing_fields_fall_back_to_defaults`** (private, L213)
  - Signature: `fn missing_fields_fall_back_to_defaults()` (test)
  - Purpose: Confirms `#[serde(default)]` partial load behavior.
  - Behavior: JSON with only `sidebar_width` keeps that value and fills other fields from defaults.

- **`compose_bounds_default_to_centered_open_size`** (private, L221)
  - Signature: `fn compose_bounds_default_to_centered_open_size()` (test)
  - Purpose: Validates default compose geometry sentinels.
  - Behavior: Default has negative compose x/y and default width/height constants.

- **`missing_compose_fields_fall_back_to_defaults`** (private, L230)
  - Signature: `fn missing_compose_fields_fall_back_to_defaults()` (test)
  - Purpose: Ensures compose fields default when absent from JSON.
  - Behavior: Partial JSON yields default compose width and x.

- **`invalid_json_loads_defaults`** (private, L237)
  - Signature: `fn invalid_json_loads_defaults()` (test)
  - Purpose: Ensures corrupt files do not crash loading.
  - Behavior: Writes invalid text to a temp file and expects `Config::default()` from `load_from`.

- **`save_then_load_round_trips_on_disk`** (private, L245)
  - Signature: `fn save_then_load_round_trips_on_disk()` (test)
  - Purpose: End-to-end disk persistence test.
  - Behavior: Saves a non-default config to a temp path and reloads with equality.

- **`unique_temp_path`** (private, L273)
  - Signature: `fn unique_temp_path() -> PathBuf` (test helper)
  - Purpose: Generates a unique temp JSON path per test run.
  - Behavior: Combines system temp dir, process id, and nanosecond timestamp.
