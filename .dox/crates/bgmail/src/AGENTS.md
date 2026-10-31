# crates/bgmail/src/ — Source API

## Purpose

Source for the `bgmail` binary: app entry, views, mail HTML, locale, config, and OS helpers.

## Ownership

- Owns: All modules under `crates/bgmail/src/`.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/bgmail/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with purpose
  and behavior. Update the matching entry when changing signatures or semantics.
- Prefer rustdoc on public items; DOX purpose/behavior should stay aligned with
  rustdoc.
- Visibility in entries (`pub` / `private`) reflects the source item.

## Work Guidance

- After adding/removing/renaming a function, update this catalog in the same
  change.
- Do not weaken parent DOX contracts from root/`crates/`/`bgmail/`.

## Verification

- `cargo test -p bgmail`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

_624 functions/methods documented._

### `src/actions.rs`

#### Types / constants

- **actions `Quit`, `ToggleCommandPalette`, `ComposeNew`, `OpenSettings`, `ToggleSidebar`, `MessageDelete`, `MessageDeletePermanent`, `MessageRestore`, `MessageArchive`, `MessageMarkJunk`, `MessageToggleFlag`, `ComposeSend`, `ComposeAttach`, `ComposeDiscard`, `ComposeClose`, `FindInMessage`, `MessagePrint`**: GPUI action types declared via the `actions!` macro under the `bgmail` namespace. Each is a zero-sized dispatch token wired to menus, key bindings, and `cx.on_action` handlers in `main.rs`.
- **struct `MoveMessageToFolder`**: Parameterized action carrying a folder storage path (`path: SharedString`). Dispatched when the user picks a move target from menus or the command palette.

#### Functions / methods

_(No functions — only action types.)_

### `src/app_menus.rs`

#### Types / constants

- **`MenuSurface`** (pub, L25)
  - Signature: `pub enum MenuSurface { Main, Compose }`
  - Purpose: Identifies which window's command set should drive the global menu bar.
  - Behavior: `Main` reflects the mail reader (File, Edit, View, Message menus). `Compose` replaces those with compose-specific File/Edit menus while a compose window is key.

- **`ActiveMenuSurface`** (pub, L32)
  - Signature: `pub struct ActiveMenuSurface(pub MenuSurface)`
  - Purpose: GPUI global that records the menu layout last installed by a sync function.
  - Behavior: Stored via `cx.set_global` when `sync_main_menus` or `sync_compose_menus` runs. Read by `active_menu_surface`, defaulting to `Main` when unset.

#### Functions / methods

##### Context: `module`

- **`app_menu`** (private, L36)
  - Signature: `fn app_menu() -> Menu`
  - Purpose: Builds the application menu (macOS app menu / first menu slot).
  - Behavior: Returns a menu named "BGMail" with About (no-op), Services OS submenu, separator, and Quit bound to `Quit`.

- **`edit_menu`** (private, L49)
  - Signature: `fn edit_menu() -> Menu`
  - Purpose: Builds the standard Edit menu shared by main and compose surfaces.
  - Behavior: Wires Undo, Redo, Cut, Copy, Paste, and Select All to GPUI OS actions (`OsAction::*`) with `NoAction` placeholders.

- **`build_menus`** (pub, L65)
  - Signature: `pub fn build_menus(ctx: &CommandContext, language: Language) -> Vec<Menu>`
  - Purpose: Builds the full menu tree for the main mail window.
  - Behavior: Assembles App, File (compose + settings), Edit, View (sidebar toggle + command palette), and Message menus. Message actions are added only when `commands::command_enabled` allows them; move-to-folder items come from `ctx.move_targets()` filtered the same way. Flag label switches between flag/unflag based on `ctx.message_starred()`. Move targets appear in a submenu when non-empty.

- **`build_compose_menus`** (pub, L158)
  - Signature: `pub fn build_compose_menus(language: Language) -> Vec<Menu>`
  - Purpose: Builds menus while a compose window is key.
  - Behavior: Returns App, a File menu (Send, Attach, Close, Discard), and Edit. Omits View and Message menus entirely.

- **`active_menu_surface`** (pub, L177)
  - Signature: `pub fn active_menu_surface(cx: &App) -> MenuSurface`
  - Purpose: Reads which menu surface is currently active.
  - Behavior: Returns the inner `MenuSurface` from the `ActiveMenuSurface` global, or `MenuSurface::Main` if the global is missing.

- **`push_if_enabled`** (private, L183)
  - Signature: `fn push_if_enabled<A: gpui::Action>(items: &mut Vec<MenuItem>, label: &'static str, action: A, id: CommandId, ctx: &CommandContext)`
  - Purpose: Conditionally appends a localized action item to a menu list.
  - Behavior: Calls `commands::command_enabled(&id, ctx)`; when true, pushes `MenuItem::action(label, action)`.

- **`sync_main_menus`** (pub, L196)
  - Signature: `pub fn sync_main_menus(cx: &mut App, ctx: &CommandContext, language: Language)`
  - Purpose: Refreshes the global menu bar for the main mail window.
  - Behavior: Sets `ActiveMenuSurface(Main)` and replaces GPUI menus with `build_menus(ctx, language)`.

- **`sync_compose_menus`** (pub, L202)
  - Signature: `pub fn sync_compose_menus(cx: &mut App, language: Language)`
  - Purpose: Refreshes the global menu bar for the compose window.
  - Behavior: Sets `ActiveMenuSurface(Compose)` and installs `build_compose_menus(language)`.

- **`sync_menus`** (pub, L208)
  - Signature: `pub fn sync_menus(cx: &mut App, ctx: &CommandContext, language: Language)`
  - Purpose: Convenience alias to refresh main-window menus from a command context.
  - Behavior: Delegates to `sync_main_menus`.

- **`menu_has_action_label`** (private, L214)
  - Signature: `fn menu_has_action_label(menu: &Menu, label: &str) -> bool` (`#[cfg(test)]`)
  - Purpose: Test helper that checks whether a menu tree contains an action with an exact label.
  - Behavior: Recursively walks `MenuItem::Action` and `MenuItem::Submenu` entries, comparing action names to `label`.

- **`ctx_with_inbox_message`** (private, L229)
  - Signature: `fn ctx_with_inbox_message() -> CommandContext` (tests)
  - Purpose: Builds a `CommandContext` with a selected inbox message and folder metadata.
  - Behavior: Constructs a synthetic `MessageDetail` in INBOX and a single-folder map so message-menu tests have enabled delete/move actions.

- **`message_menu_includes_delete_when_message_selected`** (private, L261)
  - Signature: `fn message_menu_includes_delete_when_message_selected()` (test)
  - Purpose: Asserts delete appears in the Message menu when a message is selected.
  - Behavior: Builds menus from `ctx_with_inbox_message()` and checks the localized delete label via `menu_has_action_label`.

- **`message_menu_omits_delete_without_selection`** (private, L274)
  - Signature: `fn message_menu_omits_delete_without_selection()` (test)
  - Purpose: Asserts delete is omitted when nothing is selected.
  - Behavior: Builds menus from `CommandContext::default()` and expects delete label absent.

- **`file_menu_always_includes_compose`** (private, L287)
  - Signature: `fn file_menu_always_includes_compose()` (test)
  - Purpose: Asserts compose is always available from File.
  - Behavior: Finds the File menu and checks for the localized compose window title action.

- **`compose_menus_include_send_attach_close_and_discard`** (private, L300)
  - Signature: `fn compose_menus_include_send_attach_close_and_discard()` (test)
  - Purpose: Validates compose surface File menu contents and absence of reader-only menus.
  - Behavior: Checks Send, Attach, Close, and Discard labels exist; asserts Message and View menus are not present.

### `src/command_palette.rs`

#### Types / constants

- **struct `CommandPaletteState`**: Mutable state for the in-window command palette overlay (open flag, query, selection index, search input entity).

#### Functions / methods

##### Context: `CommandPaletteState`

- **`filtered_entries`** (pub, L17)
  - Signature: `pub fn filtered_entries(&self, language: Language, ctx: &CommandContext) -> Vec<CommandEntry>`
  - Purpose: Returns palette commands filtered by the current query.
  - Behavior: Calls `palette_commands` then keeps entries whose label matches `self.query` via `command_matches_query`.

- **`clamp_selection`** (pub, L28)
  - Signature: `pub fn clamp_selection(&mut self, count: usize)`
  - Purpose: Keeps `selected_ix` within `[0, count-1]`.
  - Behavior: Resets to 0 when count is zero; clamps down when index exceeds last entry.

- **`on_query_change`** (pub, L36)
  - Signature: `pub fn on_query_change(&mut self, query: String)`
  - Purpose: Updates the filter query and resets selection.
  - Behavior: Stores query and sets `selected_ix` to 0 so filtering starts from the top.

- **`move_selection`** (pub, L41)
  - Signature: `pub fn move_selection(&mut self, delta: isize, entry_count: usize) -> bool`
  - Purpose: Moves highlight up/down by `delta`.
  - Behavior: Returns false when there are no entries or selection unchanged; otherwise clamps and returns true.

- **`selected_command`** (pub, L55)
  - Signature: `pub fn selected_command<'a>(&self, entries: &'a [CommandEntry]) -> Option<&'a CommandId>`
  - Purpose: Returns the command id at the current selection index.
  - Behavior: Indexes into `entries` at `selected_ix`; None when out of range.
##### Context: `module (tests)`

- **`selection_clamps_to_last_entry`** (private, L65)
  - Signature: `fn selection_clamps_to_last_entry()`
  - Purpose: Test: selection clamps when entry count shrinks.
  - Behavior: Sets index past end, clamps to 3 entries, expects index 2.

### `src/command_palette_overlay.rs`

#### Types / constants

- _(None at module top-level.)_

#### Functions / methods

- **`render_command_palette`** (pub(crate), L17)
  - Signature: `pub(crate) fn render_command_palette(cx: &mut Context<RootView>, entries: Vec<CommandEntry>, selected_ix: usize, input: Entity<TextInput>) -> gpui::AnyElement`
  - Purpose: Builds the dimmed in-window command palette overlay.
  - Behavior: Renders a semi-transparent backdrop (click/Escape dismisses), centered panel with search input and scrollable command list. List items highlight selection and execute on click. Key events delegate to `RootView::handle_command_palette_key`. Panel clicks stop propagation so backdrop dismiss does not fire.

- **`palette_module_exports_in_window_renderer`** (private, L99)
  - Signature: `fn palette_module_exports_in_window_renderer()`
  - Purpose: Test: overlay stays an in-window RootView helper.
  - Behavior: Source guard asserting module doc and `pub(crate) fn render_command_palette` exist.

### `src/commands.rs`

#### Types / constants

- **enum `CommandId`**: Identifies a command in menus and the palette (compose, settings, message actions incl. `MessagePrint` (enabled with a selected message, even in Trash), move-to-folder with path).
- **struct `CommandContext`**: Snapshot of UI state (selected message, detail, folders) used to decide command availability and labels.
- **struct `CommandEntry`**: One palette row: `id` plus localized `label`.

#### Functions / methods

##### Context: `CommandContext`

- **`message_in_trash`** (pub, L35)
  - Signature: `pub fn message_in_trash(&self) -> bool`
  - Purpose: Whether the selected message is in Trash.
  - Behavior: True when `message_detail.folders_csv` contains the system trash path.

- **`message_in_junk`** (pub, L41)
  - Signature: `pub fn message_in_junk(&self) -> bool`
  - Purpose: Whether the selected message is in Junk.
  - Behavior: True when folders CSV contains the system junk path.

- **`message_in_archive`** (pub, L47)
  - Signature: `pub fn message_in_archive(&self) -> bool`
  - Purpose: Whether the selected message is in Archive.
  - Behavior: True when folders CSV contains the system archive path.

- **`message_starred`** (pub, L53)
  - Signature: `pub fn message_starred(&self) -> bool`
  - Purpose: Whether the selected message is flagged/starred.
  - Behavior: Reads `starred` from `message_detail` when present.

- **`move_targets`** (pub, L59)
  - Signature: `pub fn move_targets(&self) -> Vec<(SharedString, SharedString)>`
  - Purpose: Eligible move destinations for the selected message.
  - Behavior: Returns `(display_name, path)` pairs for folders in the message's account, excluding forbidden manual-move destinations (sent, drafts, flagged, etc.).

##### Context: `module`

- **`message_has_folder`** (private, L85)
  - Signature: `fn message_has_folder(csv: &str, folder_path: &str) -> bool`
  - Purpose: Checks whether a comma-separated folder list contains a path.
  - Behavior: Splits on commas, ignores empty segments, compares paths exactly.
- **`folder_display_name`** (pub, L92)
  - Signature: `pub fn folder_display_name(folder: &Folder, language: Language) -> String`
  - Purpose: Localized display name for a storage folder.
  - Behavior: Uses `folder.display_name` when set; otherwise maps system paths to localized mailbox names via `MailboxKind`, falling back to the raw path.
- **`system_folder_label`** (private, L99)
  - Signature: `fn system_folder_label(path: &str, language: Language) -> String`
  - Purpose: Maps a system folder path to a localized label.
  - Behavior: Matches known system paths to `MailboxKind` display names; unknown paths pass through unchanged.
- **`label_for`** (private, L113)
  - Signature: `fn label_for(id: &CommandId, language: Language, ctx: &CommandContext) -> SharedString`
  - Purpose: Localized label for a command id.
  - Behavior: Uses locale keys for standard commands; flag toggles between Flag/Unflag based on starred state; move commands use folder display names.
- **`command_enabled`** (pub, L141)
  - Signature: `pub fn command_enabled(id: &CommandId, ctx: &CommandContext) -> bool`
  - Purpose: Whether a command can run in the current context.
  - Behavior: Always enables compose/settings/sidebar. Message commands require a selection and apply folder-state rules (e.g. delete disabled in trash, restore only in trash, archive disabled when already archived or in trash).
- **`palette_commands`** (pub, L163)
  - Signature: `pub fn palette_commands(language: Language, ctx: &CommandContext) -> Vec<CommandEntry>`
  - Purpose: Builds the filtered command palette list.
  - Behavior: Starts with global commands; adds message actions and move targets when a message is selected; filters through `command_enabled` and attaches localized labels.
- **`command_matches_query`** (pub, L194)
  - Signature: `pub fn command_matches_query(label: &str, query: &str) -> bool`
  - Purpose: Case-insensitive substring filter for the palette.
  - Behavior: Empty or whitespace-only queries match everything; otherwise compares ASCII-lowercased label against query.
##### Context: `module (tests)`

- **`ctx_with_message`** (private, L208)
  - Signature: `fn ctx_with_message()`
  - Purpose: Builds a test `CommandContext` with inbox or trash message.
  - Behavior: Constructs message detail and folder map for unit tests.

- **`delete_disabled_in_trash_restore_enabled`** (private, L257)
  - Signature: `fn delete_disabled_in_trash_restore_enabled()`
  - Purpose: Test: trash enables restore/permanent delete, disables normal delete.
  - Behavior: Uses trash context and asserts enabled/disabled flags.

- **`inbox_message_can_delete_not_restore`** (private, L265)
  - Signature: `fn inbox_message_can_delete_not_restore()`
  - Purpose: Test: inbox message allows delete, not restore.
  - Behavior: Asserts delete enabled and restore disabled for inbox message.

- **`palette_never_lists_command_palette_action`** (private, L272)
  - Signature: `fn palette_never_lists_command_palette_action()`
  - Purpose: Test: palette does not list itself.
  - Behavior: Ensures no palette entry mentions command palette in EN/PT.

- **`palette_without_message_omits_message_actions`** (private, L283)
  - Signature: `fn palette_without_message_omits_message_actions()`
  - Purpose: Test: no message commands without selection.
  - Behavior: Default context omits delete/archive/move entries but includes compose.

- **`palette_with_message_includes_move_targets`** (private, L296)
  - Signature: `fn palette_with_message_includes_move_targets()`
  - Purpose: Test: selection adds move-to-folder entries.
  - Behavior: Inbox message context includes at least one `MoveToFolder` command.

- **`flag_label_reflects_starred_state`** (private, L305)
  - Signature: `fn flag_label_reflects_starred_state()`
  - Purpose: Test: flag label becomes Unflag when starred.
  - Behavior: Starred message yields Unflag label for toggle-flag command.

- **`command_filter_is_case_insensitive`** (private, L312)
  - Signature: `fn command_filter_is_case_insensitive()`
  - Purpose: Test: query matching is case-insensitive.
  - Behavior: ARCH matches Archive Message; junk does not.

- **`move_targets_exclude_sent_drafts_and_flagged`** (private, L318)
  - Signature: `fn move_targets_exclude_sent_drafts_and_flagged()`
  - Purpose: Test: forbidden folders excluded from move list.
  - Behavior: Sent, drafts, and flagged paths absent; user folder present.

### `src/compose.rs`

#### Types / constants

- **`COMPOSE_DEFAULT_WIDTH`** (pub, L23)
  - Signature: `pub const COMPOSE_DEFAULT_WIDTH: f32 = 790.0`
  - Purpose: Default compose window width in logical pixels on first open.
  - Behavior: Used by `Config::default` and clamped upward by `clamp_compose_size`.

- **`COMPOSE_DEFAULT_HEIGHT`** (pub, L25)
  - Signature: `pub const COMPOSE_DEFAULT_HEIGHT: f32 = 720.0`
  - Purpose: Default compose window height in logical pixels on first open.
  - Behavior: Persisted in config defaults and respected unless below minimum height.

- **`COMPOSE_MIN_WIDTH`** (pub, L27)
  - Signature: `pub const COMPOSE_MIN_WIDTH: f32 = 480.0`
  - Purpose: Minimum allowed compose window width.
  - Behavior: Enforced by `clamp_compose_size` when restoring saved sizes.

- **`COMPOSE_MIN_HEIGHT`** (pub, L29)
  - Signature: `pub const COMPOSE_MIN_HEIGHT: f32 = 400.0`
  - Purpose: Minimum allowed compose window height.
  - Behavior: Enforced by `clamp_compose_size` when restoring saved sizes.

- **`COMPOSE_POSITION_UNSET`** (pub, L33)
  - Signature: `pub const COMPOSE_POSITION_UNSET: f32 = -1.0`
  - Purpose: Sentinel coordinate stored in config when the compose window has never been placed.
  - Behavior: Negative x/y values cause `open_bounds` to center the window instead of using saved origin.

- **`FIELD_LABEL_WIDTH`** (private, L65)
  - Signature: `const FIELD_LABEL_WIDTH: f32 = 72.0`
  - Purpose: Fixed width of the From/To/Cc/Subject label column in pixels.
  - Behavior: Applied in `field_row` and the To row so header rows align visually.

- **`ComposeView`** (pub, L68)
  - Signature: `pub struct ComposeView { accounts, from_account, show_cc_bcc, root, last_reported_bounds, white_background }`
  - Purpose: GPUI entity rendering the standalone new-message window (visual mock).
  - Behavior: Holds mock field state, a weak link to `RootView` for bounds persistence and close notification, cached white-background preference, and defers bounds sync to avoid re-entrancy during render.

#### Functions / methods

##### Context: `module`

- **`open_bounds`** (pub, L38)
  - Signature: `pub fn open_bounds(origin: Point<gpui::Pixels>, window_size: Size<gpui::Pixels>, cx: &App) -> Bounds<gpui::Pixels>`
  - Purpose: Computes initial window bounds from persisted compose geometry.
  - Behavior: Clamps size via `clamp_compose_size`. When `compose_position_is_unset(origin)`, returns centered bounds; otherwise uses the saved origin with clamped size.

- **`clamp_compose_size`** (pub, L52)
  - Signature: `pub fn clamp_compose_size(window_size: Size<gpui::Pixels>) -> Size<gpui::Pixels>`
  - Purpose: Ensures stored compose dimensions meet minimum width/height.
  - Behavior: Takes the max of each dimension with `COMPOSE_MIN_WIDTH` / `COMPOSE_MIN_HEIGHT`.

- **`compose_position_is_unset`** (pub, L60)
  - Signature: `pub fn compose_position_is_unset(origin: Point<gpui::Pixels>) -> bool`
  - Purpose: Detects whether persisted coordinates mean "center on first open".
  - Behavior: Returns true when either x or y converts to a negative `f32`.

- **`compose_body_colors`** (private, L363)
  - Signature: `fn compose_body_colors(white_background: bool) -> (Option<Hsla>, Color)`
  - Purpose: Chooses compose body background and placeholder colors, mirroring the reader.
  - Behavior: When `white_background` is true, returns white fill plus a custom mid-gray placeholder; otherwise transparent background with `Color::Disabled` placeholder.

- **`window_title`** (pub, L425)
  - Signature: `pub fn window_title(language: Language) -> &'static str`
  - Purpose: Localized window title for compose windows.
  - Behavior: Resolves `Key::ComposeWindowTitle` for the given language.

- **`compose_body_uses_white_background_when_reader_pref_is_on`** (private, L436)
  - Signature: `fn compose_body_uses_white_background_when_reader_pref_is_on()` (test)
  - Purpose: Verifies white-background color mapping for compose body.
  - Behavior: Asserts `compose_body_colors(true)` yields a background and `compose_body_colors(false)` yields none with disabled placeholder color.

- **`clamp_compose_size_enforces_minimums`** (private, L445)
  - Signature: `fn clamp_compose_size_enforces_minimums()` (test)
  - Purpose: Ensures undersized stored dimensions are raised to minimums.
  - Behavior: Clamps 100×200 to `COMPOSE_MIN_WIDTH` × `COMPOSE_MIN_HEIGHT`.

- **`clamp_compose_size_leaves_large_sizes_unchanged`** (private, L452)
  - Signature: `fn clamp_compose_size_leaves_large_sizes_unchanged()` (test)
  - Purpose: Ensures sizes above minimums pass through unchanged.
  - Behavior: Clamps 900×800 and expects identical output.

- **`compose_position_unset_detects_negative_coordinates`** (private, L459)
  - Signature: `fn compose_position_unset_detects_negative_coordinates()` (test)
  - Purpose: Covers sentinel and normal coordinate detection.
  - Behavior: Negative x or y is unset; (0, 0) is considered set.

- **`window_title_is_localized`** (private, L469)
  - Signature: `fn window_title_is_localized()` (test)
  - Purpose: Checks English and Portuguese compose titles.
  - Behavior: Expects "New Message" and "Nova mensagem".

- **`new_view_defaults_to_first_account_and_hidden_cc_bcc`** (private, L475)
  - Signature: `fn new_view_defaults_to_first_account_and_hidden_cc_bcc()` (test)
  - Purpose: Validates initial `ComposeView` state with sample accounts.
  - Behavior: Expects `from_account == 0`, hidden Cc/Bcc, and first account email as From address.

- **`selected_from_address_is_empty_without_accounts`** (private, L484)
  - Signature: `fn selected_from_address_is_empty_without_accounts()` (test)
  - Purpose: Ensures empty account list yields empty From address.
  - Behavior: Constructs view with no accounts and expects `""`.

- **`toggle_cc_bcc_flips_visibility`** (private, L490)
  - Signature: `fn toggle_cc_bcc_flips_visibility()` (test)
  - Purpose: Documents expected toggle semantics (logic-only test).
  - Behavior: Flips a boolean twice to assert show/hide behavior pattern.

- **`cycle_from_account_wraps`** (private, L499)
  - Signature: `fn cycle_from_account_wraps()` (test)
  - Purpose: Documents wrap-around account cycling (logic-only test).
  - Behavior: Increments from last index modulo account count expecting index 0.

##### Context: `ComposeView`

- **`new`** (pub, L88)
  - Signature: `pub fn new(accounts: Vec<Account>, root: WeakEntity<RootView>, white_background: bool) -> Self`
  - Purpose: Creates a compose view seeded with accounts and preferences.
  - Behavior: Initializes first account selected, Cc/Bcc hidden, no reported bounds, and stores the weak root handle plus white-background flag.

- **`set_white_background`** (pub(crate), L100)
  - Signature: `pub(crate) fn set_white_background(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Applies white-compose preference pushed from the main view without reading `RootView` during render.
  - Behavior: Updates cached flag and calls `cx.notify()` only when the value changes.

- **`toggle_cc_bcc`** (private, L108)
  - Signature: `fn toggle_cc_bcc(&mut self, cx: &mut Context<Self>)`
  - Purpose: Shows or hides Cc and Bcc header rows.
  - Behavior: Flips `show_cc_bcc` and notifies for re-render.

- **`cycle_from_account`** (private, L114)
  - Signature: `fn cycle_from_account(&mut self, cx: &mut Context<Self>)`
  - Purpose: Mock From-account selector cycling through available accounts.
  - Behavior: No-op when `accounts` is empty; otherwise increments index modulo length and notifies.

- **`send_message`** (pub(crate), L123)
  - Signature: `pub(crate) fn send_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Placeholder send action until the domain layer exists.
  - Behavior: Intentionally empty (ignores `cx`).

- **`attach_file`** (pub(crate), L128)
  - Signature: `pub(crate) fn attach_file(&mut self, cx: &mut Context<Self>)`
  - Purpose: Placeholder attach action until a file picker exists.
  - Behavior: Intentionally empty (ignores `cx`).

- **`discard_draft`** (pub(crate), L133)
  - Signature: `pub(crate) fn discard_draft(&mut self, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Mock discard that closes the compose window.
  - Behavior: Delegates to `close_window`.

- **`close_window`** (pub(crate), L138)
  - Signature: `pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Closes the compose window and notifies the main view.
  - Behavior: Defers work: upgrades `root` to call `on_compose_window_closed`, then removes the window. Avoids synchronous root updates during close handling.

- **`sync_menus_if_active`** (private, L150)
  - Signature: `fn sync_menus_if_active(&self, window: &Window, language: Language, cx: &mut Context<Self>)`
  - Purpose: Keeps the global menu bar on the compose surface when this window is active.
  - Behavior: Calls `app_menus::sync_compose_menus` when `window.is_window_active()`.

- **`selected_from_address`** (private, L156)
  - Signature: `fn selected_from_address(&self) -> &str`
  - Purpose: Returns the email of the currently selected From account.
  - Behavior: Looks up `accounts[from_account]` email or returns empty string.

- **`render_toolbar`** (private, L163)
  - Signature: `fn render_toolbar(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Builds the bottom toolbar (discard, attach, send).
  - Behavior: Renders bordered row with trash icon (discard), flexible spacer, attachment icon, and filled Send button wired to mock handlers.

- **`render_header`** (private, L206)
  - Signature: `fn render_header(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Builds the compose header fields (From, To, optional Cc/Bcc, Subject).
  - Behavior: From row cycles accounts on click; To row shows placeholder and Cc/Bcc toggle; optional Cc/Bcc rows and Subject row use `field_row`. Uses theme border and muted label colors.

- **`cc_bcc_toggle`** (private, L288)
  - Signature: `fn cc_bcc_toggle(&self, language: Language, text_color: Hsla, hover_bg: Hsla, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Compact Cc/Bcc toggle aligned with 12px field rows.
  - Behavior: Renders localized label with hover background, stops mouse-down propagation, and toggles Cc/Bcc visibility on click.

- **`field_row`** (private, L312)
  - Signature: `fn field_row(&self, label: &'static str, placeholder: impl Into<SharedString>, value_slot: Option<impl IntoElement>, border: Hsla) -> impl IntoElement`
  - Purpose: One labeled header row with optional interactive value slot.
  - Behavior: Fixed-width muted label, bottom border, and either a custom value element or disabled placeholder label.

- **`render_body`** (private, L352)
  - Signature: `fn render_body(&self, language: Language, white_background: bool) -> impl IntoElement`
  - Purpose: Builds the message body placeholder area.
  - Behavior: Flex-grow region with optional white background and localized body placeholder text.

##### Context: `Render for ComposeView`

- **`render`** (private, L380)
  - Signature: `fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Renders the full compose window and wires actions/menus/bounds sync.
  - Behavior: When windowed bounds change, defers `root.sync_compose_window_bounds`. Syncs compose menus if active. Builds vertical layout with header, body, toolbar; registers `ComposeSend`, `ComposeAttach`, `ComposeDiscard`, and `ComposeClose` actions.


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


### `src/crash_guard.rs`

#### Types / constants

- **struct `CrashGuard`** (pub, Clone): Path of the reader's render marker (`reader-render.guard` beside the mail database).

#### Functions / methods

- **`CrashGuard::beside(db_path)`**: Guard for that database.
- **`CrashGuard::take_unfinished()`**: Message id left by a run that crashed while rendering it (marker cleared); `None` otherwise.
- **`CrashGuard::begin(id)`**: Writes the marker before rendering formatted HTML (best effort).
- **`CrashGuard::finish()`**: Removes the marker once a frame showing it was drawn.
- Tests: unfinished renders reported once; finished renders leave no marker.

### `src/print.rs`

#### Types / constants

- **const `STALE_AFTER`** (private): 10 minutes; older print pages are deleted on the next print.

#### Functions / methods

- **`print_dir()`** (pub): `<temp dir>/bgmail-print`.
- **`nonce()`** (pub): 16 hex digits from the process's random hasher keys, the clock, a counter and the pid (CSP nonce and file name).
- **`write_print_page(dir, html, nonce)`** (pub): Creates the private directory, removes stale pages, writes `message-<nonce>.html` as a new private file; returns its path.
- **`remove_stale_pages(dir, now, age)`** (pub): Deletes `message-*.html` older than `age`; returns how many.
- **`create_private_dir(dir)`** / **`create_private_file(path)`** (private): Unix modes 0700 / 0600 (create-new); elsewhere plain creation (Windows' temp dir is per user).
- Tests: nonces differ; pages are private, never overwritten; only stale pages are removed.

### `src/data.rs`

#### Types / constants

- **`EMBEDDED_IMAGE_BYTES`** (private, L17)
  - Signature: `const EMBEDDED_IMAGE_BYTES: &[u8]`
  - Purpose: Raw PNG bytes baked into the binary for the first sample message.
  - Behavior: Loaded via `include_bytes!` from `assets/tweezers.png`; encoded to a `data:` URI so the reader renders it without file access.

- **`EMBEDDED_IMAGE_WIDTH`** (private, L22)
  - Signature: `const EMBEDDED_IMAGE_WIDTH: u32 = 700`
  - Purpose: Display width (px) for the embedded sample image.
  - Behavior: Matches asset intrinsic width; referenced in generated HTML `width` attributes and layout tests.

- **`EMBEDDED_IMAGE_HEIGHT`** (private, L23)
  - Signature: `const EMBEDDED_IMAGE_HEIGHT: u32 = 200`
  - Purpose: Display height (px) for the embedded sample image.
  - Behavior: Matches asset intrinsic height; referenced in generated HTML `height` attributes.

- **`MailboxKind`** (pub, L27)
  - Signature: `pub enum MailboxKind { Inbox, Drafts, Sent, Junk, Trash, Archive, Custom }`
  - Purpose: Semantic kind of a mailbox for icons and localized naming.
  - Behavior: Standard kinds map to locale keys; `Custom` carries its label on `Mailbox::label` instead.

- **`GlobalMailbox`** (pub, L66)
  - Signature: `pub enum GlobalMailbox { Inbox, Flagged, Drafts, Sent }`
  - Purpose: Unified sidebar mailbox aggregating the same logical folder across accounts (mock-only).
  - Behavior: Display order defined by `ALL`; each variant has a dedicated locale key.

- **`GlobalMailbox::ALL`** (pub, L75)
  - Signature: `pub const ALL: [GlobalMailbox; 4]`
  - Purpose: Ordered list of global mailboxes for sidebar rendering.
  - Behavior: `[Inbox, Flagged, Drafts, Sent]`.

- **`Mailbox`** (pub, L101)
  - Signature: `pub struct Mailbox { kind, unread, label }`
  - Purpose: One mailbox row within an account in mock data.
  - Behavior: Standard mailboxes use kind-derived names; custom folders set `label` and `kind: Custom`.

- **`Account`** (pub, L140)
  - Signature: `pub struct Account { name, email, mailboxes }`
  - Purpose: Mock connected e-mail account with nested mailboxes.
  - Behavior: Used by sidebar, settings, and compose mock selectors.

- **`MessageBody`** (pub, L149)
  - Signature: `pub enum MessageBody { Html(SharedString), Text(SharedString) }`
  - Purpose: Reader content variant for HTML or plain-text bodies.
  - Behavior: Carries full body string; both are rendered by the reader (`mail_html` wraps text in `<pre>`).

- **`Message`** (pub, L156)
  - Signature: `pub struct Message { sender, sender_email, subject, preview, body, time, unread, starred, has_attachment }`
  - Purpose: Mock list/detail message model for the visual prototype.
  - Behavior: Populates message list and seeds storage via `db_seed`.

- **`BASE64_ALPHABET`** (private, L535)
  - Signature: `const BASE64_ALPHABET: &[u8; 64]`
  - Purpose: RFC 4648 base64 alphabet for the inline encoder.
  - Behavior: Used by `base64_encode` when building embedded image data URIs.

#### Functions / methods

##### Context: `MailboxKind`

- **`name_key`** (private, L42)
  - Signature: `fn name_key(self) -> Option<Key>`
  - Purpose: Locale key for standard mailbox names.
  - Behavior: Returns `Some(Key::…)` for built-in kinds and `None` for `Custom`.

- **`display_name`** (pub, L56)
  - Signature: `pub fn display_name(self, language: Language) -> &'static str`
  - Purpose: Localized name for a standard mailbox kind.
  - Behavior: Translates via `name_key`; returns empty string for `Custom` (see `Mailbox::display_name`).

##### Context: `GlobalMailbox`

- **`name_key`** (private, L83)
  - Signature: `fn name_key(self) -> Key`
  - Purpose: Locale key for a global mailbox label.
  - Behavior: Maps each variant to its corresponding `Key` (Inbox reuses mailbox inbox key).

- **`display_name`** (pub, L93)
  - Signature: `pub fn display_name(self, language: Language) -> &'static str`
  - Purpose: Localized label for a global mailbox.
  - Behavior: Resolves `name_key().tr(language)`.

##### Context: `Mailbox`

- **`new`** (private, L110)
  - Signature: `fn new(kind: MailboxKind, unread: usize) -> Self`
  - Purpose: Constructs a standard mailbox with no custom label.
  - Behavior: Sets `label: None` and stores kind plus unread count.

- **`custom`** (private, L119)
  - Signature: `fn custom(name: impl Into<SharedString>, unread: usize) -> Self`
  - Purpose: Constructs a user-created folder mailbox.
  - Behavior: Sets `kind: Custom` and stores explicit `label`.

- **`display_name`** (pub, L130)
  - Signature: `pub fn display_name(&self, language: Language) -> SharedString`
  - Purpose: Sidebar/settings display name for any mailbox.
  - Behavior: Returns custom label when present; otherwise localized kind name.

##### Context: `module`

- **`default_mailboxes`** (private, L169)
  - Signature: `fn default_mailboxes(inbox_unread: usize) -> Vec<Mailbox>`
  - Purpose: Standard six-mailbox set for mock accounts.
  - Behavior: Inbox (with given unread), Drafts, Sent, Junk (3 unread), Trash, Archive.

- **`sample_accounts`** (pub, L182)
  - Signature: `pub fn sample_accounts() -> Vec<Account>`
  - Purpose: Multi-account mock data exercising sidebar overflow and custom folders.
  - Behavior: Returns five accounts with varied unread counts; Work account adds six custom folders including a long-name truncation case.

- **`sample_messages`** (pub, L230)
  - Signature: `pub fn sample_messages() -> Vec<Message>`
  - Purpose: Mock inbox message list with diverse metadata and bodies.
  - Behavior: Builds 18 messages from static tuples; first message uses rich HTML with embedded `data:` image; others cycle through `sample_email_bodies()`.

- **`html_body`** (private, L450)
  - Signature: `fn html_body(subject: &str, preview: &str, sender: &str, image_src: &str) -> SharedString`
  - Purpose: Generates rich HTML exercising reader features.
  - Behavior: Includes headings, lists, blockquote, code block, inline/remote images, normal and ~2000-character links for status-bar overflow tests.

- **`sample_email_bodies`** (pub, L515)
  - Signature: `pub fn sample_email_bodies() -> Vec<MessageBody>`
  - Purpose: Catalog of reusable HTML/text fixtures from `assets/emails/`.
  - Behavior: Returns 14 `include_str!` bodies (mostly HTML, one plain text) for mocks and tests.

- **`base64_encode`** (private, L540)
  - Signature: `fn base64_encode(input: &[u8]) -> String`
  - Purpose: Minimal dependency-free base64 encoder with padding.
  - Behavior: Encodes 3-byte chunks using `BASE64_ALPHABET`; emits `=` padding as needed.

- **`embedded_image_data_uri`** (private, L564)
  - Signature: `fn embedded_image_data_uri() -> String`
  - Purpose: Self-contained PNG data URI for the first sample message.
  - Behavior: Prefixes `data:image/png;base64,` with base64 of `EMBEDDED_IMAGE_BYTES`.

- **`accounts_have_default_mailboxes`** (private, L576)
  - Signature: `fn accounts_have_default_mailboxes()` (test)
  - Purpose: Validates every sample account has six standard mailboxes in order.
  - Behavior: Asserts five accounts, each with at least six mailboxes matching expected kinds.

- **`work_account_has_custom_folders`** (private, L597)
  - Signature: `fn work_account_has_custom_folders()` (test)
  - Purpose: Ensures Work account custom folder seeding.
  - Behavior: Expects 12 mailboxes total and six `Custom` folders; first custom named "Clients".

- **`global_mailboxes_are_localized`** (private, L616)
  - Signature: `fn global_mailboxes_are_localized()` (test)
  - Purpose: Checks global mailbox translations and inbox label reuse.
  - Behavior: Compares English/Portuguese Flagged labels; Inbox matches `MailboxKind::Inbox` label.

- **`custom_folder_name_is_not_localized`** (private, L633)
  - Signature: `fn custom_folder_name_is_not_localized()` (test)
  - Purpose: Ensures custom folder labels are language-independent.
  - Behavior: Same display name in English and Portuguese for a custom folder.

- **`sample_messages_are_populated`** (private, L642)
  - Signature: `fn sample_messages_are_populated()` (test)
  - Purpose: Sanity-checks mock message list diversity.
  - Behavior: Non-empty list with at least one unread and one attachment flag.

- **`sample_email_bodies_has_at_least_ten_varied_contents`** (private, L650)
  - Signature: `fn sample_email_bodies_has_at_least_ten_varied_contents()` (test)
  - Purpose: Guards fixture catalog size.
  - Behavior: Asserts at least ten bodies in `sample_email_bodies()`.

- **`sample_email_bodies_are_all_non_empty`** (private, L660)
  - Signature: `fn sample_email_bodies_are_all_non_empty()` (test)
  - Purpose: Ensures no empty fixture files.
  - Behavior: Trims each HTML/text body and rejects empty strings.

- **`sample_email_bodies_mix_html_and_text`** (private, L670)
  - Signature: `fn sample_email_bodies_mix_html_and_text()` (test)
  - Purpose: Confirms fixture catalog includes both formats.
  - Behavior: Asserts at least one Html and one Text variant in the catalog.

- **`sample_messages_include_html_and_text_bodies`** (private, L677)
  - Signature: `fn sample_messages_include_html_and_text_bodies()` (test)
  - Purpose: Confirms assembled messages use both body kinds.
  - Behavior: Scans `sample_messages()` for Html and Text bodies.

- **`first_message_embeds_the_image_as_a_data_uri`** (private, L688)
  - Signature: `fn first_message_embeds_the_image_as_a_data_uri()` (test)
  - Purpose: Validates inline image embedding in the first HTML message.
  - Behavior: Expects `data:image/png;base64,` substring and explicit width/height attributes.

- **`base64_encodes_known_vectors`** (private, L705)
  - Signature: `fn base64_encodes_known_vectors()` (test)
  - Purpose: RFC 4648 vector coverage for the inline encoder.
  - Behavior: Checks empty input and classic padding cases (`f`, `fo`, `foo`, etc.).

- **`embedded_image_data_uri_is_well_formed`** (private, L716)
  - Signature: `fn embedded_image_data_uri_is_well_formed()` (test)
  - Purpose: Ensures data URI prefix and non-empty payload.
  - Behavior: Starts with PNG data-URI scheme and has content beyond the prefix.

- **`embedded_image_is_a_decodable_raster`** (private, L723)
  - Signature: `fn embedded_image_is_a_decodable_raster()` (test)
  - Purpose: Guards against non-raster bytes in the embedded asset.
  - Behavior: Asserts PNG or JPEG magic bytes at start of `EMBEDDED_IMAGE_BYTES`.

- **`embedded_image_is_wider_than_a_typical_reading_pane`** (private, L737)
  - Signature: `fn embedded_image_is_wider_than_a_typical_reading_pane()` (test)
  - Purpose: Ensures horizontal scrollbar exercise in the reader.
  - Behavior: Requires `EMBEDDED_IMAGE_WIDTH >= 640`.

- **`unread_count_matches_first_account`** (private, L744)
  - Signature: `fn unread_count_matches_first_account()` (test)
  - Purpose: Spot-checks Personal account inbox unread mock count.
  - Behavior: Expects 5000 unread on first account's inbox.

- **`mailbox_names_are_localized`** (private, L750)
  - Signature: `fn mailbox_names_are_localized()` (test)
  - Purpose: Verifies inbox localization in English and Portuguese.
  - Behavior: Expects "Inbox" and "Caixa de entrada" for `MailboxKind::Inbox`.


### `src/db_seed.rs`

#### Types / constants

_(None at module top level — conversion helpers only.)_

#### Functions / methods

- **`sample_dir_messages(dir)`** (pub): Development (`BGMAIL_SAMPLE_DIR`): every `*.html` in `dir`, sorted, as extra unread inbox messages of the default account after the samples; subject from `<title>` (`html_title`) else the file stem, sender = file stem.
- **`html_title(html)`** (private): Text of `<title>` with whitespace collapsed; `None` when missing/empty.

##### Context: `module`

- **`seed_accounts`** (pub, L12)
  - Signature: `pub fn seed_accounts() -> Vec<SeedAccount>`
  - Purpose: Builds account and folder seed records for initial SQLite population.
  - Behavior: Maps `data::sample_accounts()` into `SeedAccount` values, converting each mailbox via `seed_mailbox`.

- **`seed_mailbox`** (private, L23)
  - Signature: `fn seed_mailbox(mailbox: &Mailbox) -> SeedMailbox`
  - Purpose: Converts one mock mailbox into a storage seed record.
  - Behavior: Standard kinds map to `system::*` paths via `seed_system`; custom folders set `custom_name` from `mailbox.label` and leave `system_path` unset.

- **`seed_system`** (private, L39)
  - Signature: `fn seed_system(path: &'static str, unread: usize) -> SeedMailbox`
  - Purpose: Builds a seed record for a known system folder path.
  - Behavior: Sets `system_path: Some(path)`, no custom name, and copies unread count.

- **`seed_messages`** (pub, L48)
  - Signature: `pub fn seed_messages() -> Vec<SeedMessage>`
  - Purpose: Builds message seed records from mock sample messages.
  - Behavior: Assigns all messages to the first sample account email (fallback `you@gmail.com`), preserving sample order as `sort_order`.

- **`seed_message`** (private, L61)
  - Signature: `fn seed_message(account_email: &str, sort_order: i64, message: &Message) -> SeedMessage`
  - Purpose: Converts one mock `Message` into a storage seed row.
  - Behavior: Copies HTML/text raw content and format; derives `plain_text` via `plain_text_from_raw`; uses mock preview or `preview_from_plain` when preview empty; copies metadata flags and leaves `extra_folders` empty.

- **`global_folder_path`** (pub, L92)
  - Signature: `pub fn global_folder_path(global: GlobalMailbox) -> &'static str`
  - Purpose: Maps unified sidebar mailboxes to storage system folder paths.
  - Behavior: Returns `system::INBOX`, `FLAGGED`, `DRAFTS`, or `SENT` for the corresponding `GlobalMailbox` variant.

- **`folder_kind_from_path`** (pub, L102)
  - Signature: `pub fn folder_kind_from_path(path: &str) -> MailboxKind`
  - Purpose: Maps stored folder paths back to UI mailbox kinds for icons and localization.
  - Behavior: Matches known system paths; any other path becomes `MailboxKind::Custom`.

- **`folder_display_name`** (pub, L115)
  - Signature: `pub fn folder_display_name(path: &str, display_name: &str, language: crate::locale::Language) -> gpui::SharedString`
  - Purpose: Resolves sidebar folder row labels from storage metadata.
  - Behavior: Returns non-empty `display_name` when provided; otherwise localized name from `folder_kind_from_path(path)`.

- **`message_body_from_detail`** (pub, L127)
  - Signature: `pub fn message_body_from_detail(detail: &storage::MessageDetail) -> MessageBody`
  - Purpose: Converts stored message detail into the UI reader body enum.
  - Behavior: Wraps `raw_content` as `MessageBody::Html` when `raw_format == "html"`, otherwise `MessageBody::Text`.

- **`seed_messages_match_sample_count`** (private, L140)
  - Signature: `fn seed_messages_match_sample_count()` (test)
  - Purpose: Ensures seed message count tracks mock catalog size.
  - Behavior: Asserts `seed_messages().len()` equals `sample_messages().len()`.

- **`seed_accounts_include_custom_folders`** (private, L145)
  - Signature: `fn seed_accounts_include_custom_folders()` (test)
  - Purpose: Verifies custom folders survive account seeding.
  - Behavior: Finds Work account and asserts a mailbox with custom name "Clients" exists.

- **`plain_text_is_derived_from_html_bodies`** (private, L155)
  - Signature: `fn plain_text_is_derived_from_html_bodies()` (test)
  - Purpose: Sanity-checks HTML-to-plain derivation used during seeding.
  - Behavior: Strips tags from sample HTML and retains visible text content.


### `src/locale.rs`

#### Types / constants

- **`Language`** (pub, L15)
  - Signature: `pub enum Language { English, Portuguese }`
  - Purpose: Supported UI languages (English default).
  - Behavior: Stored as a GPUI global; resolved at render time via `ActiveLanguage`.

- **`Language::ALL`** (pub, L25)
  - Signature: `pub const ALL: [Language; 2]`
  - Purpose: Ordered list for language selectors and exhaustive translation tests.
  - Behavior: `[English, Portuguese]`.

- **`Key`** (pub, L38)
  - Signature: `pub enum Key { … }` (66 UI string keys)
  - Purpose: Typed catalog of translatable UI chrome strings.
  - Behavior: Each variant maps to per-language text via `tr`; covers mailboxes, settings (incl. the Privacy note on blocked remote images), toolbar, commands, compose, the reader context menu (`CtxCopy`, `CtxSelectAll`, `CtxOpenLink`, `CtxCopyLink`), the find bar (`FindPlaceholder`, `FindMatches` "{} of {}", `FindNoMatches`), plain-text fallback notices (`NoticeTooLarge`, `NoticeTooDeep`, `NoticeCrashed`, `NoticeShowFormatted`) and status strings.

- **`GlobalLanguage`** (private, L332)
  - Signature: `struct GlobalLanguage(Language)`
  - Purpose: GPUI global holding the active UI language.
  - Behavior: Wrapped by `init` / `set_language`; read through `ActiveLanguage for App`.

- **`ActiveLanguage`** (pub, L350)
  - Signature: `pub trait ActiveLanguage { fn language(&self) -> Language; }`
  - Purpose: Trait for reading the active language from GPUI contexts.
  - Behavior: Implemented for `App` to return the inner `Language` from `GlobalLanguage`.

#### Functions / methods

##### Context: `Language`

- **`label`** (pub, L28)
  - Signature: `pub fn label(self) -> &'static str`
  - Purpose: Endonym shown in language picker UI.
  - Behavior: Returns "English" or "Português (Brasil)" for each variant.

##### Context: `Key`

- **`tr`** (pub, L126)
  - Signature: `pub fn tr(self, language: Language) -> &'static str`
  - Purpose: Resolves a UI string key to localized text.
  - Behavior: Large `match` on `(self, language)` returning static English or Brazilian Portuguese strings; includes format placeholders like `{}` for `BlockedElements`.

##### Context: `module`

- **`message_count`** (pub, L295)
  - Signature: `pub fn message_count(language: Language, count: usize) -> String`
  - Purpose: Localized message-list header count.
  - Behavior: English `"{count} messages"`; Portuguese `"{count} mensagens"`.

- **`status_counts`** (pub, L303)
  - Signature: `pub fn status_counts(language: Language, accounts: usize, messages: usize) -> String`
  - Purpose: Status bar left segment with account and message totals.
  - Behavior: English `"{accounts} accounts · {messages} messages"`; Portuguese uses "contas" and "mensagens".

- **`status_search_counts`** (pub, L311)
  - Signature: `pub fn status_search_counts(language: Language, accounts: usize, showing: usize, total: usize) -> String`
  - Purpose: Status bar left segment while search filtering is active.
  - Behavior: English `"{accounts} accounts · {showing} of {total} messages"`; Portuguese uses "de" phrasing.

- **`status_unread`** (pub, L324)
  - Signature: `pub fn status_unread(language: Language, unread: usize) -> String`
  - Purpose: Status bar right segment with unread count and sync hint.
  - Behavior: English `"{unread} unread · Updated just now"`; Portuguese uses "não lidas · Atualizado agora".

- **`status_zoom`** (pub)
  - Signature: `pub fn status_zoom(language: Language, zoom: f32) -> String`
  - Purpose: Status bar note for the reader's page zoom: `"Zoom {percent}%"` (both languages).

- **`init`** (pub, L337)
  - Signature: `pub fn init(language: Language, cx: &mut App)`
  - Purpose: Initializes localization at app startup.
  - Behavior: Sets `GlobalLanguage(language)` on the GPUI app.

- **`set_language`** (pub, L342)
  - Signature: `pub fn set_language(language: Language, cx: &mut App)`
  - Purpose: Switches active language live across all windows.
  - Behavior: Updates global and calls `cx.refresh_windows()` so every open window re-renders with new strings.

- **`every_key_has_text_in_every_language`** (private, L433)
  - Signature: `fn every_key_has_text_in_every_language()` (test)
  - Purpose: Ensures translation completeness for all 64 keys.
  - Behavior: Iterates `ALL_KEYS` × `Language::ALL` asserting non-empty `tr` results.

- **`translations_differ_between_languages`** (private, L445)
  - Signature: `fn translations_differ_between_languages()` (test)
  - Purpose: Spot-checks that locales actually differ where expected.
  - Behavior: Compares inbox and settings title strings across English and Portuguese.

- **`default_language_is_english`** (private, L458)
  - Signature: `fn default_language_is_english()` (test)
  - Purpose: Confirms `Language::default()` is English.
  - Behavior: Equality assertion on default enum value.

- **`formatted_strings_include_arguments`** (private, L463)
  - Signature: `fn formatted_strings_include_arguments()` (test)
  - Purpose: Verifies dynamic status strings embed numeric arguments.
  - Behavior: Checks English status counts contain digits and "accounts"; Portuguese unread string contains count and "não lidas".

##### Context: `ActiveLanguage for App`

- **`language`** (private, L356)
  - Signature: `fn language(&self) -> Language`
  - Purpose: Reads active language from the GPUI app global.
  - Behavior: Returns `self.global::<GlobalLanguage>().0`.


### `src/mail_html.rs`

#### Types / constants

- **struct `RenderedEmail`** (pub): Full themed HTML document for `html_view` plus `blocked_images` (remote `<img>` withheld) for the privacy shield.
- **struct `DocumentColors`** (pub): Page `background`, `text`, `accent`; `is_dark()` (background lightness < 0.5) selects the dark scheme.
- **struct `PrintHeader`** (pub): Subject, sender and date (with localized labels) printed above a message.
- **const `PRINT_COLORS`** (private): Light page colors used for printing whatever the theme.
- **const `DISALLOWED_ELEMENTS`** (private): Selector of elements stripped with their content (scripts, frames, media, form controls, `base`/`meta`/`link`, scripting surfaces, scriptable SVG parts).
- **const `URL_ATTRIBUTES`** (private): Attributes vetted for dangerous URL schemes.
- **const `RESOURCE_ATTRIBUTES`** (private): Attributes that fetch on render; blocked for remoteness when loading is off.

#### Functions / methods

- **`DocumentColors::is_dark`** (pub): `background.l < 0.5`.
- **`plain_text`** (pub): Readable text of an HTML body for the plain-text fallback: one linear scan (no tree), drops tags/comments, skips `script`/`style`/`head`/`title` content, blank lines around paragraph blocks and line breaks at line blocks, tabs between cells, decoded entities, collapsed whitespace except in `<pre>`.
- **`decode_entities`** (private): Named basics, `&nbsp;`, decimal/hex references; unknown entities kept.
- **`is_openable_link`** (pub): Whether a clicked link may be opened outside the app: `http(s)://` and `mailto:` only (case-insensitive, trimmed).
- **`email_document`** (pub)
  - Signature: `pub fn email_document(colors: DocumentColors, body: &MessageBody) -> RenderedEmail`
  - Purpose: Builds the reader document for a body.
  - Behavior: HTML bodies are sanitized with remote loading off (Stage 3 adds loading); plain text is escaped into `<pre class="plain">`. Wraps the result in a document with `document_css` that starts with the body's own doctype (`leading_doctype`, so legacy mail keeps its quirks mode), else `<!DOCTYPE html>`.
- **`leading_doctype`** (private): The doctype an HTML body starts with, after a BOM, whitespace, `<?…?>` declarations and comments; `None` without one.
- **`print_document`** (pub)
  - Signature: `pub fn print_document(header: &PrintHeader, body: &MessageBody, nonce: &str) -> String`
  - Purpose: Standalone page for printing a message in the system browser.
  - Behavior: `email_document` with `PRINT_COLORS`, plus in `<head>` (first) a Content-Security-Policy (`default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'nonce-…'`), the subject as title, header styles and a nonce'd script calling `print()` on load; the escaped header opens `<body>`. Remote content stays blocked by the sanitizer and, as defense in depth, by the CSP.
- **`document_css`** (private): Theme stylesheet (color scheme, page colors/padding/font, links in accent, blocked-image placeholder, code/pre/blockquote/hr styles).
- **`css_color`** / **`css_color_alpha`** / **`channel`** (private): Theme colors → CSS `rgb()`/`rgba()`; channel clamps and rounds to 0–255.
- **`sanitize_html`** (private, test-only): `sanitize_html_inner` without the remote-image report.
- **`sanitize_html_inner`** (private)
  - Signature: `fn sanitize_html_inner(html: &str, load_remote: bool, shown: &HashSet<String>) -> (String, bool, usize)`
  - Purpose: Streaming `lol_html` sanitizer; returns (html, has remote `<img>`, blocked count).
  - Behavior: Drops `DISALLOWED_ELEMENTS` with content, neutralizes attributes on every element, and (remote loading off) blanks CSS `url(...)`. Rewrite failures drop the body.
- **`strip_css_urls`** / **`find_url_open`** (private): Empty every CSS `url(...)` (case-insensitive `url(` detection).
- **`neutralize_attributes`** (private): Removes `on*`, `contenteditable`, dangerous-scheme URLs, scripted `style`, and (unless allowed) remote resource attributes / `srcset`; a blocked remote `<img src>` is kept as `data-rm-blocked-src`; returns the image's blocked state.
- **`is_dangerous_url`** / **`is_remote_url`** / **`style_has_script_vector`** (private): URL scheme and CSS vector checks.
- **`escape_html`** (private): Escapes `& < > " '`.
- Tests: HTML escaping, plain text (structure, entities, broken markup), openable links, colors, document wrapping (HTML verbatim, plain text escaped, color scheme), disallowed elements, SVG scriptable parts, event handlers, dangerous schemes, legacy style vectors, safe markup kept, remote detection and blocking (attributes, `href` kept, CSS urls), remote image policy (`load_remote`, `shown`), documents always blocking remote images.

### `src/main.rs`

#### Types / constants

- **struct `MainWindow`**: GPUI global holding the `WindowHandle<RootView>` for routing app-wide shortcuts and deferred menu actions.
- **struct `CommandPaletteShortcut`**: GPUI global keeping the keystroke interceptor subscription alive for the app lifetime.

#### Functions / methods

##### Context: `module`

- **`is_command_palette_keystroke`** (private, L56)
  - Signature: `fn is_command_palette_keystroke(keystroke: &Keystroke) -> bool`
  - Purpose: Detects Ctrl+P / Cmd+P command-palette shortcut.
  - Behavior: Matches key P (case-insensitive) without Shift when Control or platform (Cmd) modifier is held.

- **`toggle_command_palette`** (private, L62)
  - Signature: `fn toggle_command_palette(cx: &mut App)`
  - Purpose: Defers palette toggle to the main RootView.
  - Behavior: Reads `MainWindow` global and schedules `RootView::toggle_command_palette` on the next frame.

- **`schedule_menu_command`** (private, L71)
  - Signature: `fn schedule_menu_command(id: CommandId, cx: &mut App)`
  - Purpose: Defers a menu command to RootView if enabled.
  - Behavior: Builds command context, checks `command_enabled`, then calls `execute_command` when allowed.

- **`schedule_compose_action`** (private, L83)
  - Signature: `fn schedule_compose_action(cx: &mut App, action: impl FnOnce(...) + 'static)`
  - Purpose: Defers an action on the compose window.
  - Behavior: Routes closure through `RootView::with_compose_window` when compose is open.

- **`schedule_compose_close`** (private, L95)
  - Signature: `fn schedule_compose_close(cx: &mut App)`
  - Purpose: Defers closing the focused auxiliary window.
  - Behavior: Calls `RootView::close_focused_auxiliary_window` (compose or settings).

- **`schedule_find_in_message`** (private)
  - Signature: `fn schedule_find_in_message(cx: &mut App)`
  - Purpose: Opens the reader's find bar in the main window.
  - Behavior: Defers `RootView::open_find_bar`. Registered globally so Cmd/Ctrl+F works even when nothing in the main window is focused.

- **`register_global_menu_actions`** (private, L105)
  - Signature: `fn register_global_menu_actions(cx: &mut App)`
  - Purpose: Registers app-wide action handlers for macOS menu bar.
  - Behavior: Maps each GPUI action to deferred menu/compose handlers so items stay enabled when focus is in a compose/settings window.

- **`app_menus`** (private, L161)
  - Signature: `fn app_menus() -> Vec<Menu>`
  - Purpose: Builds initial macOS application menus at startup.
  - Behavior: Uses default command context and language; full menus refresh from RootView as state changes.

- **`register_command_palette_shortcuts`** (private, L168)
  - Signature: `fn register_command_palette_shortcuts(cx: &mut App)`
  - Purpose: Wires command palette toggle action and keystroke interceptor.
  - Behavior: Registers `ToggleCommandPalette` handler and intercepts Ctrl/Cmd+P globally, stopping propagation.

- **`main`** (private, L181)
  - Signature: `fn main()`
  - Purpose: Application entry point.
  - Behavior: Startup milestones, theme/locale init, window open with persisted bounds, Windows cloaked maximize sequence, persistence hooks and globals.

- **`command_palette_shortcut_matches_ctrl_p_and_cmd_p`** (private, L391)
  - Signature: `fn command_palette_shortcut_matches_ctrl_p_and_cmd_p()`
  - Purpose: Test: palette shortcut detection.
  - Behavior: Asserts Ctrl+P, Cmd+P, and uppercase P match; plain P does not.

### `src/root.rs`

#### Types / constants

- **const `SIDEBAR_MIN_WIDTH`**: Minimum sidebar column width (150px).
- **const `LIST_MIN_WIDTH`**: Minimum message-list column width (350px).
- **const `READER_MIN_WIDTH`**: Minimum reading-pane width (550px).
- **const `WINDOW_MIN_WIDTH` / `WINDOW_MIN_HEIGHT`**: Minimum main window size (950×480px); exported for startup clamping.
- **const `NARROW_BREAKPOINT`**: Window width (1200px) below which sidebar auto-collapses and floats when reopened.
- **const `RESIZE_HANDLE_WIDTH`**: Hit width of column resize dividers (6px).
- **const `FOLD_ANIM_MS`**: Fixed sidebar account fold animation duration (90ms).
- **const `SIDEBAR_ROW_HEIGHT`**: Fixed sidebar row height for fold math (32px).
- **const `CHEVRON_BOX` / `CHEVRON_ICON`**: Chevron container and glyph sizes for account disclosure.
- **const `ITEM_INDENT` / `ITEM_PADDING`**: Sidebar tree indentation and inner row padding.
- **const `DRAG_THRESHOLD`**: Pixels before toolbar mouse-down becomes window drag (2px).
- **const `SEARCH_FIELD_WIDTH` / `SEARCH_ICON_WIDTH`**: Expanded and collapsed search control widths.
- **const `SEARCH_DEBOUNCE_MS`**: Delay before search query reloads the message list (150ms).
- **const `READER_LIGHT_TEXT`**: Near-black body text when reader uses forced white background.
- **enum `ResizeHandle`**: Which column divider is being dragged (`Sidebar` or `List`).
- **struct `ResizeDrag`**: GPUI drag payload wrapping `ResizeHandle`.
- **enum `Selection`**: Sidebar selection — unified `Global` mailbox or account `Mailbox` with folder path.
- **struct `FoldAnim`**: In-flight account fold animation token and direction.
- **enum `SettingsSection`**: Settings window section (General, Accounts, Appearance, Notifications, Privacy).
- **struct `ReaderSignature`**: Memo key for the reader document (message id and page colors, which include the white-background preference).
- **struct `ReaderMenu`**: Open reader context menu: window position, `HitInfo`, whether text was selected.
- **struct `FindBar`**: Open find bar: query `TextInput`, match count, current match, input observation.
- **const `MAX_FORMATTED_HTML_BYTES`** (1 MB): HTML bodies above it are shown as plain text unless the user asks for the formatted version.
- **enum `ReaderNotice`**: Why a message is shown as plain text: `TooLarge`, `Unrenderable` (`RenderError`), `Crashed` (crash guard).
- **`plain_text_reason`** (private): `Crashed` if the message crashed the renderer, else `TooLarge` above the limit unless shown formatted anyway, else `None`.
- **struct `RootView`**: Main application view state (layout, selection, reader `HtmlView`, search, palette, auxiliary windows, persistence).
- **struct `SettingsView`**: Preferences window content with section navigation.

#### Functions / methods

- **`mail_list_query`** (private, L179)
  - Signature: `fn mail_list_query(selection: &Selection, search: &str) -> MailListQuery`
  - Purpose: Builds SQLite list query for current sidebar selection or active search.
  - Behavior: Non-empty search returns `Search`; otherwise maps global or account folder selection to the appropriate query variant.
- **`mailbox_icon`** (private, L248)
  - Signature: `fn mailbox_icon(kind: MailboxKind) -> IconName`
  - Purpose: Icon for a mailbox kind in the sidebar.
  - Behavior: Maps each MailboxKind to a semantic IconName.
- **`global_mailbox_icon`** (private, L260)
  - Signature: `fn global_mailbox_icon(kind: GlobalMailbox) -> IconName`
  - Purpose: Icon for unified global mailboxes.
  - Behavior: Maps inbox/flagged/drafts/sent global mailboxes to icons.
- **`chevron_svg`** (private, L272)
  - Signature: `fn chevron_svg(color: Hsla, angle: f32) -> Svg`
  - Purpose: Rotatable chevron SVG for account disclosure.
  - Behavior: Renders right-pointing chevron rotated by `angle` radians (0= collapsed, π/2= expanded).
- **`ui_accounts`** (private, L461)
  - Signature: `fn ui_accounts(accounts: &[storage::Account]) -> Vec<data::Account>`
  - Purpose: Adapts storage accounts for legacy compose/settings UI types.
  - Behavior: Maps name/email fields; leaves mailboxes empty.
- **`settings_row`** (private, L3684)
  - Signature: `fn settings_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> impl IntoElement`
  - Purpose: Settings UI row with muted label and value.
  - Behavior: Horizontal flex row with spaced labels.
- **`can_open_command_palette`** (pub(crate), L3911)
  - Signature: `pub(crate) fn can_open_command_palette(main_window_active: bool) -> bool`
  - Purpose: Gate for opening the command palette.
  - Behavior: Returns `main_window_active` — palette only opens when main window is key.
##### Context: `SettingsSection`

- **`title_key`** (private, L227)
  - Signature: `fn title_key(self) -> Key`
  - Purpose: Locale key for section title.
  - Behavior: Maps each section to its Key variant.

- **`icon`** (private, L237)
  - Signature: `fn icon(self) -> IconName`
  - Purpose: Icon for section nav.
  - Behavior: Maps each section to Settings/Account/Palette/Flag/Shield icons.
##### Context: `RootView`

- **`new`** (pub, L473)
  - Signature: `pub fn new(settings: Config) -> Self`
  - Purpose: Constructs RootView with default database path.
  - Behavior: Delegates to `new_with_database` using `storage::database_path()`.

- **`new_with_database`** (private, L477)
  - Signature: `fn new_with_database(settings: Config, db_path: impl AsRef<Path>) -> Self`
  - Purpose: Constructs RootView with explicit DB path (tests).
  - Behavior: Opens SQLite, seeds if empty, loads folders/messages, restores layout from settings, initializes selection to global inbox.

- **`reload_message_list`** (private, L586)
  - Signature: `fn reload_message_list(&mut self)`
  - Purpose: Reloads list column from database.
  - Behavior: Queries messages for current selection/search and updates `list_messages`.

- **`clear_message_selection`** (private, L605)
  - Signature: `fn clear_message_selection(&mut self)`
  - Purpose: Clears selected message and reader state.
  - Behavior: Resets selected id, detail, and reader signature.

- **`reload_selected_detail`** (private, L611)
  - Signature: `fn reload_selected_detail(&mut self)`
  - Purpose: Reloads full body for selected message.
  - Behavior: Fetches MessageDetail from DB when an id is selected.

- **`on_message_selection_changed`** (private, L617)
  - Signature: `fn on_message_selection_changed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Handles message selection side effects.
  - Behavior: Reloads detail, syncs menus, notifies GPUI.

- **`command_context`** (pub(crate), L624)
  - Signature: `pub(crate) fn command_context(&self) -> CommandContext`
  - Purpose: Snapshot for menus and palette.
  - Behavior: Packages selected message id, detail clone, and folders map.

- **`sync_app_menus`** (pub(crate), L633)
  - Signature: `pub(crate) fn sync_app_menus(&self, cx: &mut Context<Self>)`
  - Purpose: Refreshes global menu bar from current state.
  - Behavior: Skips when compose window is active; otherwise syncs main menus with command context.

- **`on_compose_window_closed`** (pub(crate), L643)
  - Signature: `pub(crate) fn on_compose_window_closed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Cleanup when compose window closes.
  - Behavior: Clears compose handle and restores main menu surface.

- **`on_settings_window_closed`** (pub(crate), L650)
  - Signature: `pub(crate) fn on_settings_window_closed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Cleanup when settings window closes.
  - Behavior: Clears settings handle and notifies.

- **`close_focused_auxiliary_window`** (pub(crate), L657)
  - Signature: `pub(crate) fn close_focused_auxiliary_window(&mut self, cx: &mut Context<Self>)`
  - Purpose: Closes focused compose or settings window.
  - Behavior: Removes and closes whichever auxiliary window has focus.

- **`close_settings_window`** (pub(crate), L672)
  - Signature: `pub(crate) fn close_settings_window(&mut self, cx: &mut Context<Self>)`
  - Purpose: Closes settings window if open.
  - Behavior: Used by global shortcut fallback.

- **`compose_window_open_and_active`** (private, L680)
  - Signature: `fn compose_window_open_and_active(&self, cx: &mut Context<Self>) -> bool`
  - Purpose: Whether compose window exists and is active.
  - Behavior: Checks compose handle active state.

- **`with_compose_window`** (pub(crate), L686)
  - Signature: `pub(crate) fn with_compose_window<R>(&self, cx, f) -> Option<R>`
  - Purpose: Runs closure on compose view if open.
  - Behavior: Updates compose entity and returns closure result or None.

- **`refresh_after_message_action`** (private, L698)
  - Signature: `fn refresh_after_message_action(&mut self, cx: &mut Context<Self>)`
  - Purpose: Post-action UI refresh.
  - Behavior: Reloads list, syncs menus, invalidates the reader signature, notifies.

- **`ensure_command_palette`** (private, L705)
  - Signature: `fn ensure_command_palette(&mut self, cx: &mut Context<Self>)`
  - Purpose: Lazily creates command palette state.
  - Behavior: Creates TextInput, wires query observer to sync filter, stores CommandPaletteState.

- **`sync_command_palette_query`** (private, L729)
  - Signature: `fn sync_command_palette_query(&mut self, cx: &mut Context<Self>)`
  - Purpose: Syncs palette filter from input entity.
  - Behavior: Reads input content, calls on_query_change when changed, notifies.

- **`toggle_command_palette`** (pub(crate), L744)
  - Signature: `pub(crate) fn toggle_command_palette(&mut self, window: &Window, cx: &mut Context<Self>)`
  - Purpose: Public palette toggle entry.
  - Behavior: Delegates to internal toggle.

- **`toggle_command_palette_internal`** (private, L753)
  - Signature: `fn toggle_command_palette_internal(&mut self, window: &Window, cx: &mut Context<Self>)`
  - Purpose: Opens or closes the palette.
  - Behavior: Closes if open; otherwise requires active main window, clears query/selection, marks focus pending.

- **`dismiss_command_palette`** (pub(crate), L775)
  - Signature: `pub(crate) fn dismiss_command_palette(&mut self, cx: &mut Context<Self>)`
  - Purpose: Closes palette overlay.
  - Behavior: Sets open=false, clears focus pending, notifies.

- **`close_command_palette`** (pub(crate), L783)
  - Signature: `pub(crate) fn close_command_palette(&mut self, cx: &mut Context<Self>)`
  - Purpose: Alias for dismiss_command_palette.
  - Behavior: Calls dismiss_command_palette.

- **`command_palette_render_args`** (private, L788)
  - Signature: `fn command_palette_render_args(&self, cx: &App) -> Option<(...)>`
  - Purpose: Snapshot for palette overlay render.
  - Behavior: When open, returns filtered entries, clamped selection index, and input entity.

- **`execute_command`** (pub(crate), L805)
  - Signature: `pub(crate) fn execute_command(&mut self, id: &CommandId, window, cx)`
  - Purpose: Runs a command by id.
  - Behavior: Dispatches to compose/settings/sidebar/message handlers, then closes palette.

- **`print_message`** (pub(crate))
  - Signature: `pub(crate) fn print_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Prints the selected message through the system browser.
  - Behavior: Builds the `PrintHeader` (sender with address, `received_at`), the print page (`mail_html::print_document` with a fresh `print::nonce`), writes it with `print::write_print_page` and opens it with `cx.open_with_system` (errors logged).

- **`delete_message_to_trash`** (pub(crate), L830)
  - Signature: `pub(crate) fn delete_message_to_trash(&mut self, cx: &mut Context<Self>)`
  - Purpose: Moves selected message to trash.
  - Behavior: No-op without selection; DB move then refresh_after_message_action.

- **`delete_message_permanently`** (pub(crate), L838)
  - Signature: `pub(crate) fn delete_message_permanently(&mut self, cx: &mut Context<Self>)`
  - Purpose: Permanently deletes selected message.
  - Behavior: Clears selection on success, then refreshes.

- **`restore_message`** (pub(crate), L848)
  - Signature: `pub(crate) fn restore_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Restores message from trash.
  - Behavior: DB restore then refresh.

- **`archive_message`** (pub(crate), L856)
  - Signature: `pub(crate) fn archive_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Archives selected message.
  - Behavior: DB archive then refresh.

- **`mark_message_junk`** (pub(crate), L864)
  - Signature: `pub(crate) fn mark_message_junk(&mut self, cx: &mut Context<Self>)`
  - Purpose: Marks selected message as junk.
  - Behavior: DB junk move then refresh.

- **`toggle_message_flag`** (pub(crate), L872)
  - Signature: `pub(crate) fn toggle_message_flag(&mut self, cx: &mut Context<Self>)`
  - Purpose: Toggles starred/flagged state.
  - Behavior: DB toggle then refresh.

- **`move_message_to_folder`** (pub(crate), L880)
  - Signature: `pub(crate) fn move_message_to_folder(&mut self, folder_path: &str, cx: &mut Context<Self>)`
  - Purpose: Moves message to folder path.
  - Behavior: DB move then refresh.

- **`handle_command_palette_key`** (pub(crate), L888)
  - Signature: `pub(crate) fn handle_command_palette_key(&mut self, event, window, cx) -> bool`
  - Purpose: Keyboard handling while palette open.
  - Behavior: Handles Escape, Up/Down (move selection), Enter (execute); returns whether event was consumed.

- **`persist_now`** (pub(crate), L953)
  - Signature: `pub(crate) fn persist_now(&self)`
  - Purpose: Immediate synchronous settings save.
  - Behavior: No-op until persist_ready; writes current_config via config::save.

- **`enable_persistence`** (pub(crate), L960)
  - Signature: `pub(crate) fn enable_persistence(&mut self)`
  - Purpose: Arms debounced persistence after startup.
  - Behavior: Sets persist_ready true (~700ms after window open).

- **`mark_content_ready`** (pub(crate), L968)
  - Signature: `pub(crate) fn mark_content_ready(&mut self, cx: &mut Context<Self>)`
  - Purpose: Reveals UI after Windows layout settles.
  - Behavior: Sets content_ready and notifies (Windows cloaked open path).

- **`current_config`** (private, L976)
  - Signature: `fn current_config(&self) -> Config`
  - Purpose: Builds Config from live layout state.
  - Behavior: Snapshots window/column bounds, maximize state, compose window geometry, and preferences.

- **`sync_compose_window_bounds`** (pub(crate), L1006)
  - Signature: `pub(crate) fn sync_compose_window_bounds(&mut self, origin, window_size, cx)`
  - Purpose: Tracks compose window move/resize.
  - Behavior: Updates stored compose origin/size and schedules debounced save.

- **`request_save`** (private, L1026)
  - Signature: `fn request_save(&mut self, cx: &mut Context<Self>)`
  - Purpose: Debounced settings persistence.
  - Behavior: Increments save token, spawns timer; latest token wins, writes on background thread.

- **`ensure_scrollbar_states`** (private, L1047)
  - Signature: `fn ensure_scrollbar_states(&mut self, cx: &mut Context<Self>)`
  - Purpose: Lazily creates scrollbar state entities.
  - Behavior: Creates list and sidebar ScrollbarState entities on first render.

- **`ensure_search_input`** (private, L1054)
  - Signature: `fn ensure_search_input(&mut self, language, window, cx)`
  - Purpose: Lazily creates toolbar search TextInput.
  - Behavior: Creates input with placeholder, blur/change handlers for debounced search sync.

- **`sync_search_if_needed`** (private, L1090)
  - Signature: `fn sync_search_if_needed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Applies search query changes from render.
  - Behavior: When query differs from last applied, reloads list, adjusts mailbox selection, syncs selection scroll.

- **`next_search_debounce_token`** (private, L1112)
  - Signature: `fn next_search_debounce_token(&mut self) -> u64`
  - Purpose: Allocates next search debounce token.
  - Behavior: Increments search_debounce_seq.

- **`search_debounce_token_current`** (private, L1117)
  - Signature: `fn search_debounce_token_current(&self, token: u64) -> bool`
  - Purpose: Checks debounce token still current.
  - Behavior: Compares token to latest search_debounce_seq.

- **`schedule_search_sync`** (private, L1121)
  - Signature: `fn schedule_search_sync(&mut self, cx: &mut Context<Self>)`
  - Purpose: Schedules debounced search list reload.
  - Behavior: Spawns timer; stale tokens ignored; notifies on fire.

- **`search_query`** (private, L1139)
  - Signature: `fn search_query(&self, cx: &App) -> String`
  - Purpose: Current search field text.
  - Behavior: Reads search input entity or empty string.

- **`search_is_active`** (private, L1147)
  - Signature: `fn search_is_active(&self, _cx: &App) -> bool`
  - Purpose: Whether search filter is applied.
  - Behavior: True when last applied query is non-empty.

- **`show_search_expanded`** (private, L1153)
  - Signature: `fn show_search_expanded(&self) -> bool`
  - Purpose: Whether toolbar shows expanded search.
  - Behavior: True when not compact or force-expanded flag set.

- **`prepare_search_focus`** (private, L1158)
  - Signature: `fn prepare_search_focus(&mut self)`
  - Purpose: Expands compact search before focus.
  - Behavior: Sets search_force_expanded when compact.

- **`show_search_clear_in_field`** (private, L1164)
  - Signature: `fn show_search_clear_in_field(&self, cx: &App) -> bool`
  - Purpose: Whether clear button shows in search field.
  - Behavior: Delegates to query-based helper with live query.

- **`show_search_clear_in_field_for_query`** (private, L1168)
  - Signature: `fn show_search_clear_in_field_for_query(&self, query: &str) -> bool`
  - Purpose: Clear button visibility rules.
  - Behavior: Always when force-expanded compact; otherwise only when query non-empty on wide toolbar.

- **`clear_search`** (private, L1177)
  - Signature: `fn clear_search(&mut self, cx: &mut Context<Self>)`
  - Purpose: Clears search and restores pre-search mailbox.
  - Behavior: Clears input, resets force-expanded, restores mailbox if searching, reloads list.

- **`sync_search_selection`** (private, L1205)
  - Signature: `fn sync_search_selection(&mut self)`
  - Purpose: Keeps selection valid after search/filter.
  - Behavior: Selects first message if current missing; scrolls list to top on query change.

- **`focus_search_input`** (private, L1222)
  - Signature: `fn focus_search_input(&mut self, window, cx)`
  - Purpose: Focuses toolbar search field.
  - Behavior: Ensures input exists, prepares expand, focuses handle.

- **`search_input_focused`** (private, L1235)
  - Signature: `fn search_input_focused(&self, window, cx) -> bool`
  - Purpose: Whether search input has focus.
  - Behavior: Checks focus handle on search entity.

- **`blur_search_if_focused`** (private, L1242)
  - Signature: `fn blur_search_if_focused(&mut self, window, cx)`
  - Purpose: Blurs search when focused.
  - Behavior: Clears focus on search input entity.

- **`should_collapse_search_after_blur`** (private, L1249)
  - Signature: `fn should_collapse_search_after_blur(&self, query: &str) -> bool`
  - Purpose: Whether compact search should collapse on blur.
  - Behavior: True when compact, force-expanded, and query empty.

- **`on_search_blur`** (private, L1254)
  - Signature: `fn on_search_blur(&mut self, cx: &mut Context<Self>)`
  - Purpose: Handles search field blur.
  - Behavior: Collapses force-expanded compact search when appropriate.

- **`on_main_window_activated`** (pub(crate), L1264)
  - Signature: `pub(crate) fn on_main_window_activated(&mut self, cx: &mut Context<Self>)`
  - Purpose: Reacts to the main window becoming active.
  - Behavior: Refreshes the custom caption layout and notifies.

- **`step_match`** (private)
  - Signature: `fn step_match(current: usize, count: usize, step: isize) -> usize`
  - Purpose: Next/previous find match index, wrapping; 0 without matches.

- **`reader_selection_color`** (private)
  - Signature: `fn reader_selection_color(accent: Hsla) -> [u8; 4]`
  - Purpose: Reader selection highlight: the accent at 35% opacity as RGBA (readable over dark and light text).

- **`on_reader_event`** (private)
  - Signature: `fn on_reader_event(&mut self, view: Entity<HtmlView>, event: &HtmlViewEvent, cx)`
  - Purpose: Handles reader events.
  - Behavior: `LinkHovered` → status bar `hovered_link`; `LinkActivated` → `cx.open_url` when `is_openable_link`; `ZoomChanged` → `reader_zoom` (status bar); `ContextMenu` → opens `ReaderMenu` (records whether text is selected).

- **`open_find_bar`** (pub(crate))
  - Signature: `pub(crate) fn open_find_bar(&mut self, window, cx)`
  - Purpose: Opens (or focuses) the find bar while a message is shown.
  - Behavior: Creates the query input on first open (its changes re-run the search, deferred) and focuses it.

- **`close_find_bar`** (private): Drops the bar, clears the reader's find highlight (`find("")`) and focuses the reader.
- **`run_find`** (private): Searches the reader for the query, resets to the first match and selects it.
- **`step_find`** (private): Moves to the next/previous match (`step_match`) and selects it (the reader scrolls it into view).

- **`sync_reader`** (private)
  - Signature: `fn sync_reader(&mut self, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Keeps the reader document in sync with the selected message and theme.
  - Behavior: Without a selection clears the signature. Otherwise computes page colors (forced white page when `reader_white_background`), skips work when the `ReaderSignature` is unchanged, picks plain text per `plain_text_reason` (`mail_html::plain_text`), builds the sanitized themed document (`mail_html::email_document`), parses it with `HtmlDocument::try_parse` (falling back to plain text, `Unrenderable`, when too deep; dark appearance for dark pages, `reader_selection_color`), records the blocked image count (0 for plain text) and the notice, marks formatted HTML renders in the `CrashGuard` until the next frame (`window.on_next_frame`), closes the context menu, sets it on the reader `HtmlView` (created and subscribed with `on_reader_event` on first use) and re-runs an open find.

- **`set_reader_white_background`** (private, L1658)
  - Signature: `fn set_reader_white_background(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Toggles reader white background pref.
  - Behavior: Persists and refreshes windows (the reader signature includes the page colors).

- **`set_compose_white_background`** (private, L1668)
  - Signature: `fn set_compose_white_background(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Toggles compose white background pref.
  - Behavior: Persists and updates open compose view if any.

- **`open_settings`** (pub(crate), L1683)
  - Signature: `pub(crate) fn open_settings(&mut self, _window, cx: &mut Context<Self>)`
  - Purpose: Opens or focuses settings window.
  - Behavior: Creates SettingsView window with persisted bounds or activates existing.

- **`open_compose`** (pub(crate), L1729)
  - Signature: `pub(crate) fn open_compose(&mut self, _window, cx: &mut Context<Self>)`
  - Purpose: Opens or focuses compose window.
  - Behavior: Creates ComposeView window or activates existing; syncs compose menus.

- **`select_mailbox`** (private, L1780)
  - Signature: `fn select_mailbox(&mut self, selection: Selection, cx: &mut Context<Self>)`
  - Purpose: Selects sidebar mailbox with UI update.
  - Behavior: Applies selection, reloads list, clears message selection, syncs menus, notifies.

- **`apply_mailbox_selection`** (private, L1798)
  - Signature: `fn apply_mailbox_selection(&mut self, selection: Selection)`
  - Purpose: Core mailbox selection without notify.
  - Behavior: Sets selected_mailbox; dismisses floating sidebar in narrow layout.

- **`note_scroll`** (private, L1807)
  - Signature: `fn note_scroll(states, cx: &mut Context<Self>)`
  - Purpose: Marks scrollbars active after scroll.
  - Behavior: Calls note_scroll on states and schedules fade-out re-render.

- **`toggle_sidebar`** (pub(crate), L1829)
  - Signature: `pub(crate) fn toggle_sidebar(&mut self)`
  - Purpose: Shows/hides accounts sidebar.
  - Behavior: Flips show_sidebar flag.

- **`is_account_collapsed`** (private, L1834)
  - Signature: `fn is_account_collapsed(&self, account_idx: usize) -> bool`
  - Purpose: Whether account mailbox list is collapsed.
  - Behavior: Checks collapsed_accounts set.

- **`toggle_account`** (private, L1841)
  - Signature: `fn toggle_account(&mut self, account_idx: usize) -> FoldAnim`
  - Purpose: Toggles account expand/collapse with animation.
  - Behavior: Updates collapsed set, assigns fold token, returns FoldAnim for timer.

- **`clear_fold`** (private, L1860)
  - Signature: `fn clear_fold(&mut self, account_idx: usize, token: u64) -> bool`
  - Purpose: Finalizes fold animation.
  - Behavior: Removes fold_anim if token matches; returns whether state changed.

- **`account_list_visible`** (private, L1875)
  - Signature: `fn account_list_visible(&self, account_idx: usize) -> bool`
  - Purpose: Whether account rows should render.
  - Behavior: Visible when expanded or mid-collapse animation.

- **`sidebar_docked`** (private, L1886)
  - Signature: `fn sidebar_docked(&self) -> bool`
  - Purpose: Whether sidebar occupies a layout column.
  - Behavior: True when sidebar shown and not narrow floating mode.

- **`reader_segment_width`** (private, L1892)
  - Signature: `fn reader_segment_width(&self) -> Pixels`
  - Purpose: Toolbar width for reader segment.
  - Behavior: Window width minus sidebar and list columns (and handles).

- **`search_is_compact`** (private, L1913)
  - Signature: `fn search_is_compact(&self) -> bool`
  - Purpose: Whether search collapses to icon.
  - Behavior: True when reader segment narrower than SEARCH_COLLAPSE_WIDTH.

- **`visible_action_groups`** (private, L1920)
  - Signature: `fn visible_action_groups(&self) -> usize`
  - Purpose: Count of toolbar action groups that fit.
  - Behavior: 0–3 based on reader segment width thresholds.

- **`render_list_controls`** (private, L1941)
  - Signature: `fn render_list_controls(&self, show_count: bool, cx) -> impl IntoElement`
  - Purpose: Message list header controls.
  - Behavior: Mailbox title, optional count, filter/more buttons; used in toolbar or list header.

- **`sync_layout`** (private, L2007)
  - Signature: `fn sync_layout(&mut self, total: Pixels)`
  - Purpose: Reconciles column widths to window size.
  - Behavior: Tracks narrow breakpoint, auto sidebar collapse/restore, clamps columns to minimums preserving reader floor.

- **`resize`** (private, L2046)
  - Signature: `fn resize(&mut self, handle: ResizeHandle, x: Pixels, total: Pixels)`
  - Purpose: Applies column divider drag.
  - Behavior: Sets sidebar or list width from cursor x, clamped so reader keeps READER_MIN_WIDTH.

- **`render_toolbar`** (private, L2067)
  - Signature: `fn render_toolbar(&mut self, window, cx) -> impl IntoElement`
  - Purpose: Renders unified top toolbar.
  - Behavior: Window drag region, list controls when docked, compose/actions/search, theme and sidebar toggles.

- **`render_search_control`** (private, L2321)
  - Signature: `fn render_search_control(&self, compact_search, search_bg, language, cx) -> AnyElement`
  - Purpose: Renders compact or expanded search UI.
  - Behavior: Icon button or TextInput with clear affordance based on layout state.

- **`render_search_clear_button`** (private, L2377)
  - Signature: `fn render_search_clear_button(language, cx) -> impl IntoElement`
  - Purpose: Clear button inside search field.
  - Behavior: Small icon button calling clear_search.

- **`icon_button_with_tooltip`** (private, L2401)
  - Signature: `fn icon_button_with_tooltip(id, label, shortcut, button) -> impl IntoElement`
  - Purpose: IconButton wrapped with tooltip div.
  - Behavior: Adds localized label and optional shortcut display.

- **`render_toolbar_separator`** (private, L2419)
  - Signature: `fn render_toolbar_separator(color: Hsla) -> impl IntoElement`
  - Purpose: Vertical toolbar divider.
  - Behavior: 1px × 20px colored bar.

- **`render_sidebar`** (private, L2425)
  - Signature: `fn render_sidebar(&self, floating: bool, cx) -> impl IntoElement`
  - Purpose: Renders accounts/folders sidebar.
  - Behavior: Global section, account groups with fold animations, scrollbars; floating adds shadow/overlay styling.

- **`render_resize_handle`** (private, L2468)
  - Signature: `fn render_resize_handle(&self, kind: ResizeHandle, _cx) -> impl IntoElement`
  - Purpose: Draggable column resize strip.
  - Behavior: Hit target on column edge; drag handled at row level.

- **`global_unread`** (private, L2492)
  - Signature: `fn global_unread(&self, global: GlobalMailbox) -> usize`
  - Purpose: Aggregated unread for global mailbox.
  - Behavior: Sums inbox unreads or counts flagged/starred depending on mailbox.

- **`render_global_section`** (private, L2500)
  - Signature: `fn render_global_section(&self, cx) -> impl IntoElement`
  - Purpose: Unified mailboxes at sidebar top.
  - Behavior: Inbox/Flagged/Drafts/Sent rows with icons, selection, unread badges.

- **`render_account`** (private, L2541)
  - Signature: `fn render_account(&self, account_idx, account, cx) -> impl IntoElement`
  - Purpose: Renders one account group in sidebar.
  - Behavior: Header with chevron toggle, animated mailbox rows, folder selection.

- **`render_account_chevron`** (private, L2698)
  - Signature: `fn render_account_chevron(&self, account_idx, color) -> AnyElement`
  - Purpose: Animated disclosure chevron.
  - Behavior: Rotates between collapsed/expanded based on fold animation state.

- **`render_count_badge`** (private, L2731)
  - Signature: `fn render_count_badge(&self, count, selected, cx) -> impl IntoElement`
  - Purpose: Unread/count pill on sidebar rows.
  - Behavior: Shows count when non-zero with selected/unselected styling.

- **`render_message_list`** (private, L2766)
  - Signature: `fn render_message_list(&mut self, cx) -> impl IntoElement`
  - Purpose: Renders scrollable message list column.
  - Behavior: Optional list header when sidebar undocked, message rows, scrollbar.

- **`render_message_row`** (private, L2847)
  - Signature: `fn render_message_row(&self, message, cx) -> impl IntoElement`
  - Purpose: Single message list row.
  - Behavior: Sender, subject preview, date, unread/flag/attachment indicators; click selects message.

- **`render_privacy_shield`** (private, L2968)
  - Signature: `fn render_privacy_shield(&self, cx) -> impl IntoElement`
  - Purpose: Privacy shield on reader subject line.
  - Behavior: Red shield with a "Blocked images: N" tooltip while the message has blocked remote images; an empty slot of the same size otherwise (layout stability). No unblock action until Stage 3.

- **`render_reader_notice`** (private)
  - Signature: `fn render_reader_notice(&self, cx) -> impl IntoElement`
  - Purpose: Notice above a message shown as plain text (reason text); `TooLarge` adds "Show formatted" (adds the message to `formatted_anyway`).

- **`render_find_bar`** (private)
  - Signature: `fn render_find_bar(&self, cx) -> impl IntoElement`
  - Purpose: Find bar under the reader header.
  - Behavior: Search-styled query field, "n of m"/"No matches" status, previous/next (`ChevronUp`/`ChevronDown`) and close buttons; Enter = next, Shift+Enter = previous, Escape closes.

- **`render_reader_menu`** (private)
  - Signature: `fn render_reader_menu(&self, cx) -> impl IntoElement`
  - Purpose: Reader context menu (`ui::ContextMenu`) anchored at the click.
  - Behavior: Openable links → Open link / Copy link; a selection → Copy (with shortcut hint); always Select all. A full-window occluding layer closes it on any click.

- **`render_reader`** (private, L3100)
  - Signature: `fn render_reader(&mut self, cx) -> impl IntoElement`
  - Purpose: Renders reading pane.
  - Behavior: Empty state, fixed header with subject/sender/date and privacy shield, and the body area hosting the reader `HtmlView` (white background when forced).

- **`render_status_bar`** (private, L3300)
  - Signature: `fn render_status_bar(&self, cx) -> impl IntoElement`
  - Purpose: Bottom status bar.
  - Behavior: Counts on the left; on the right the reader zoom (`status_zoom`, only when not 100%) and the unread count; the hovered link URL centered when present.
##### Context: `SettingsView`

- **`new`** (private, L3379)
  - Signature: `fn new(accounts: Vec<data::Account>, root: WeakEntity<RootView>) -> Self`
  - Purpose: Constructs settings view.
  - Behavior: Stores accounts, default General section, weak root link.

- **`close_window`** (private, L3387)
  - Signature: `fn close_window(&mut self, window, cx: &mut Context<Self>)`
  - Purpose: Closes settings window.
  - Behavior: Notifies root and removes window.

- **`render_nav`** (private, L3399)
  - Signature: `fn render_nav(&self, cx) -> impl IntoElement`
  - Purpose: Settings section sidebar nav.
  - Behavior: Lists sections with icons; click switches section.

- **`render_content`** (private, L3443)
  - Signature: `fn render_content(&self, cx) -> impl IntoElement`
  - Purpose: Active settings section body.
  - Behavior: Renders General/Accounts/Appearance/Notifications/Privacy content.

- **`render_language_picker`** (private, L3609)
  - Signature: `fn render_language_picker(&self, language, cx) -> impl IntoElement`
  - Purpose: Language selector control.
  - Behavior: Buttons for EN/PT; updates locale global and refreshes app.

##### Context: `Render for SettingsView`

- **`render`** (private, L3660)
  - Signature: `fn render(&mut self, _window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Settings window layout.
  - Behavior: Split nav + content with titlebar close handling.
##### Context: `Render for RootView`

- **`render`** (private, L3697)
  - Signature: `fn render(&mut self, window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Main RootView render.
  - Behavior: Full three-column layout, toolbar, deferred palette/search/reader sync, content_ready gate, window frame wrap.
##### Context: `module (tests)`

- **`test_view`** (private, L3921)
  - Signature: `fn test_view()`
  - Purpose: Creates temp DB and RootView for unit tests.
  - Behavior: TempDir + new_with_database with default config.

- **`sidebar_starts_visible`** (private, L3929)
  - Signature: `fn sidebar_starts_visible()`
  - Purpose: Test: sidebar visible by default.
  - Behavior: Asserts show_sidebar true on new view.

- **`command_palette_only_opens_when_main_window_is_focused`** (private, L3947)
  - Signature: `fn command_palette_only_opens_when_main_window_is_focused()`
  - Purpose: Test: can_open_command_palette gate.
  - Behavior: True when active, false when not.

- **`command_palette_starts_closed`** (private, L3976)
  - Signature: `fn command_palette_starts_closed()`
  - Purpose: Test: palette closed initially.
  - Behavior: command_palette none or open=false.

- **`toggle_sidebar_flips_visibility`** (private, L3985)
  - Signature: `fn toggle_sidebar_flips_visibility()`
  - Purpose: Test: toggle_sidebar inverts flag.
  - Behavior: Toggle twice restores visibility.

- **`selecting_in_narrow_layout_dismisses_floating_sidebar`** (private, L3994)
  - Signature: `fn selecting_in_narrow_layout_dismisses_floating_sidebar()`
  - Purpose: Test: narrow select hides sidebar.
  - Behavior: Narrow layout + select closes floating sidebar.

- **`selecting_in_wide_layout_keeps_sidebar_open`** (private, L4010)
  - Signature: `fn selecting_in_wide_layout_keeps_sidebar_open()`
  - Purpose: Test: wide select keeps sidebar.
  - Behavior: Wide layout selection leaves sidebar visible.

- **`mailbox_change_clears_reader_selection`** (private, L4031)
  - Signature: `fn mailbox_change_clears_reader_selection()`
  - Purpose: Test: mailbox change clears message.
  - Behavior: Selecting different mailbox clears selected message.

- **`default_selection_is_global_inbox`** (private, L4042)
  - Signature: `fn default_selection_is_global_inbox()`
  - Purpose: Test: initial selection is global inbox.
  - Behavior: selected_mailbox is Global(Inbox).

- **`global_inbox_has_unread`** (private, L4051)
  - Signature: `fn global_inbox_has_unread()`
  - Purpose: Test: seeded inbox has unread.
  - Behavior: global_unread(Inbox) > 0.

- **`global_flagged_counts_flagged_folder`** (private, L4057)
  - Signature: `fn global_flagged_counts_flagged_folder()`
  - Purpose: Test: flagged global counts starred.
  - Behavior: Uses DB count for flagged folder.

- **`global_drafts_and_sent_have_no_unread`** (private, L4065)
  - Signature: `fn global_drafts_and_sent_have_no_unread()`
  - Purpose: Test: drafts/sent unread zero.
  - Behavior: Both global_unread return 0.

- **`accounts_start_expanded`** (private, L4072)
  - Signature: `fn accounts_start_expanded()`
  - Purpose: Test: all accounts start expanded.
  - Behavior: No account in collapsed set initially.

- **`toggle_account_collapses_and_expands`** (private, L4080)
  - Signature: `fn toggle_account_collapses_and_expands()`
  - Purpose: Test: account toggle changes collapsed state.
  - Behavior: Toggle adds/removes from collapsed_accounts.

- **`toggle_account_hands_out_unique_tokens`** (private, L4093)
  - Signature: `fn toggle_account_hands_out_unique_tokens()`
  - Purpose: Test: fold tokens unique per toggle.
  - Behavior: Two toggles yield different FoldAnim tokens.

- **`collapsing_account_stays_visible_until_finalized`** (private, L4101)
  - Signature: `fn collapsing_account_stays_visible_until_finalized()`
  - Purpose: Test: collapse animation keeps rows visible.
  - Behavior: account_list_visible true during collapse anim.

- **`clear_fold_ignores_stale_token`** (private, L4113)
  - Signature: `fn clear_fold_ignores_stale_token()`
  - Purpose: Test: stale fold timer ignored.
  - Behavior: clear_fold with old token returns false.

- **`expanded_account_is_visible_without_animation`** (private, L4124)
  - Signature: `fn expanded_account_is_visible_without_animation()`
  - Purpose: Test: expanded account list visible.
  - Behavior: account_list_visible(0) true initially.

- **`narrow_window_auto_collapses_sidebar`** (private, L4130)
  - Signature: `fn narrow_window_auto_collapses_sidebar()`
  - Purpose: Test: narrow sync_layout hides sidebar.
  - Behavior: 800px width sets narrow and hides sidebar.

- **`resizing_while_narrow_recollapses_floating_sidebar`** (private, L4138)
  - Signature: `fn resizing_while_narrow_recollapses_floating_sidebar()`
  - Purpose: Test: resize in narrow re-collapses sidebar.
  - Behavior: Opening sidebar then sync_layout collapses again.

- **`floating_sidebar_stays_open_without_resize`** (private, L4150)
  - Signature: `fn floating_sidebar_stays_open_without_resize()`
  - Purpose: Test: sidebar stays open if no layout sync.
  - Behavior: Manual toggle without sync keeps open.

- **`widening_window_restores_sidebar`** (private, L4160)
  - Signature: `fn widening_window_restores_sidebar()`
  - Purpose: Test: leaving narrow restores sidebar.
  - Behavior: Wide sync_layout sets show_sidebar true.

- **`resize_respects_sidebar_minimum`** (private, L4170)
  - Signature: `fn resize_respects_sidebar_minimum()`
  - Purpose: Test: sidebar drag respects SIDEBAR_MIN_WIDTH.
  - Behavior: Drag below min clamps to minimum.

- **`resize_respects_list_minimum`** (private, L4178)
  - Signature: `fn resize_respects_list_minimum()`
  - Purpose: Test: list drag respects LIST_MIN_WIDTH.
  - Behavior: List divider drag clamped to minimum.

- **`resize_keeps_reader_minimum`** (private, L4191)
  - Signature: `fn resize_keeps_reader_minimum()`
  - Purpose: Test: reader width floor enforced.
  - Behavior: Extreme drags leave reader at READER_MIN_WIDTH.

- **`resize_locks_panels_once_reader_minimum_is_reached`** (private, L4206)
  - Signature: `fn resize_locks_panels_once_reader_minimum_is_reached()`
  - Purpose: Test: divider locks at reader minimum.
  - Behavior: Further drag does not shrink reader below floor.

- **`search_collapses_when_reader_segment_is_narrow`** (private, L4226)
  - Signature: `fn search_collapses_when_reader_segment_is_narrow()`
  - Purpose: Test: narrow reader segment compacts search.
  - Behavior: search_is_compact true on narrow layout.

- **`search_expands_on_wide_reader_segment`** (private, L4237)
  - Signature: `fn search_expands_on_wide_reader_segment()`
  - Purpose: Test: wide layout expands search.
  - Behavior: 1600px layout search_is_compact false.

- **`all_action_groups_show_on_wide_window`** (private, L4245)
  - Signature: `fn all_action_groups_show_on_wide_window()`
  - Purpose: Test: wide window shows 3 action groups.
  - Behavior: visible_action_groups equals 3.

- **`action_groups_drop_as_reader_segment_shrinks`** (private, L4252)
  - Signature: `fn action_groups_drop_as_reader_segment_shrinks()`
  - Purpose: Test: action groups drop with width.
  - Behavior: Shrinking reader segment reduces visible groups.

- **`action_groups_vanish_but_search_survives_when_tiny`** (private, L4269)
  - Signature: `fn action_groups_vanish_but_search_survives_when_tiny()`
  - Purpose: Test: tiny segment hides actions, keeps search.
  - Behavior: Minimum layout still allows search affordance.

- **`sync_layout_shrinks_columns_to_fit`** (private, L4281)
  - Signature: `fn sync_layout_shrinks_columns_to_fit()`
  - Purpose: Test: sync_layout reduces column widths.
  - Behavior: Small window clamps sidebar/list widths.

- **`search_force_expanded_overrides_compact_layout`** (private, L4293)
  - Signature: `fn search_force_expanded_overrides_compact_layout()`
  - Purpose: Test: force-expanded overrides compact.
  - Behavior: show_search_expanded true when flag set.

- **`search_debounce_token_invalidates_previous_timer`** (private, L4303)
  - Signature: `fn search_debounce_token_invalidates_previous_timer()`
  - Purpose: Test: debounce token invalidates stale timers.
  - Behavior: New token makes old token not current.

- **`search_clear_shows_in_expanded_field_only_with_text_on_wide_toolbar`** (private, L4314)
  - Signature: `fn search_clear_shows_in_expanded_field_only_with_text_on_wide_toolbar()`
  - Purpose: Test: clear button rules on wide toolbar.
  - Behavior: Clear visible only with non-empty query unless force-expanded.

- **`search_clear_always_shows_in_force_expanded_compact_field`** (private, L4322)
  - Signature: `fn search_clear_always_shows_in_force_expanded_compact_field()`
  - Purpose: Test: force-expanded shows clear even empty.
  - Behavior: Empty query still shows clear when force-expanded.

- **`search_collapses_after_blur_only_when_compact_expanded_and_empty`** (private, L4330)
  - Signature: `fn search_collapses_after_blur_only_when_compact_expanded_and_empty()`
  - Purpose: Test: blur collapse conditions.
  - Behavior: should_collapse_search_after_blur matches compact+empty rules.

- **`move_to_trash_updates_folder_membership`** (private, L4343)
  - Signature: `fn move_to_trash_updates_folder_membership()`
  - Purpose: Test: delete moves message to trash in DB.
  - Behavior: After delete_message_to_trash, detail shows trash folder.

- **`command_context_reflects_trash_state`** (private, L4358)
  - Signature: `fn command_context_reflects_trash_state()`
  - Purpose: Test: command_context trash detection.
  - Behavior: After trash move, message_in_trash true in context.

### `src/shortcuts.rs`

#### Types / constants

- **const `COMPOSE_MAC` / `COMPOSE_OTHER`**: GPUI binding strings for New Message (Cmd+N / Ctrl+N).
- **const `SETTINGS_MAC` / `SETTINGS_OTHER`**: Binding strings for Settings (Cmd+, / Ctrl+,).
- **const `TOGGLE_SIDEBAR_MAC` / `TOGGLE_SIDEBAR_OTHER`**: Binding strings for sidebar toggle.
- **const `COMMAND_PALETTE_MAC` / `COMMAND_PALETTE_OTHER`**: Binding strings for command palette.
- **const `DELETE_MESSAGE_MAC` / `DELETE_MESSAGE_OTHER`**: Binding strings for move-to-trash.
- **const `ARCHIVE_MAC` / `ARCHIVE_OTHER`**: Binding strings for archive.
- **const `MARK_JUNK_MAC` / `MARK_JUNK_OTHER`**: Binding strings for mark junk.
- **const `TOGGLE_FLAG_MAC` / `TOGGLE_FLAG_OTHER`**: Binding strings for flag/unflag.
- **const `COMPOSE_CLOSE_MAC` / `COMPOSE_CLOSE_OTHER`**: Binding strings for closing compose window.
- **const `FIND_MAC` / `FIND_OTHER`**: `cmd-f` / `ctrl-f`, open the reader's find bar.
- **const `PRINT_MAC` / `PRINT_OTHER`**: `cmd-p` / `ctrl-p`, print the selected message.

#### Functions / methods

- **`primary_binding`** (pub, L43)
  - Signature: `pub fn primary_binding<'a>(mac: &'a str, other: &'a str) -> &'a str`
  - Purpose: Returns the platform-appropriate binding string for tooltips.
  - Behavior: Returns `mac` on macOS and `other` elsewhere.
- **`format_binding`** (pub, L52)
  - Signature: `pub fn format_binding(source: &str) -> String`
  - Purpose: Formats a GPUI keystroke string for display.
  - Behavior: Parses `source` as a `Keystroke` and renders platform glyphs (e.g. `cmd-n` → `⌘N`). Falls back to the raw string on parse failure.
- **`bind_app_shortcuts`** (pub, L60)
  - Signature: `pub fn bind_app_shortcuts(cx: &mut App)`
  - Purpose: Registers global key bindings for menu-driven actions.
  - Behavior: Binds both macOS and Windows/Linux chords for each action. Command palette bindings also register under the `TextInput` context so they work while typing in search fields.

- **`format_binding_parses_compose_shortcut`** (private, L95)
  - Signature: `fn format_binding_parses_compose_shortcut()`
  - Purpose: Test: compose shortcut formats to a non-empty display string.
  - Behavior: Formats the platform compose binding and asserts it is non-empty (and contains N on macOS).

- **`settings_comma_binding_parses`** (private, L103)
  - Signature: `fn settings_comma_binding_parses()`
  - Purpose: Test: settings comma bindings parse as valid keystrokes.
  - Behavior: Asserts both macOS and non-macOS settings bindings parse successfully.

- **`primary_binding_follows_platform`** (private, L109)
  - Signature: `fn primary_binding_follows_platform()`
  - Purpose: Test: `primary_binding` selects the correct platform string.
  - Behavior: Asserts macOS returns the Mac variant and other platforms return the other variant.

### `src/startup.rs`

#### Types / constants

- _(None at module top-level.)_

#### Functions / methods

- **`mark_start`** (pub, L13)
  - Signature: `pub fn mark_start()`
  - Purpose: Records the process start instant for startup timing.
  - Behavior: Stores `Instant::now()` in a `OnceLock`. Subsequent calls are ignored.
- **`elapsed`** (pub, L18)
  - Signature: `pub fn elapsed() -> Duration`
  - Purpose: Returns elapsed time since `mark_start`.
  - Behavior: Returns zero if `mark_start` was never called.
- **`format_elapsed`** (pub, L23)
  - Signature: `pub fn format_elapsed(duration: Duration) -> String`
  - Purpose: Formats a duration for human-readable log output.
  - Behavior: Renders milliseconds with one decimal place (e.g. `"42.0ms"`).
- **`log_milestone`** (pub, L28)
  - Signature: `pub fn log_milestone(label: &str)`
  - Purpose: Logs a named startup milestone in debug builds.
  - Behavior: Prints `[BGMail startup] {label}: {elapsed}` to stderr when `debug_assertions` is enabled.

- **`format_elapsed_shows_milliseconds`** (private, L40)
  - Signature: `fn format_elapsed_shows_milliseconds()`
  - Purpose: Test: `format_elapsed` uses millisecond formatting.
  - Behavior: Asserts `format_elapsed(42ms)` equals `"42.0ms"`.

- **`elapsed_grows_after_mark_start`** (private, L45)
  - Signature: `fn elapsed_grows_after_mark_start()`
  - Purpose: Test: elapsed time increases after `mark_start`.
  - Behavior: Calls `mark_start`, sleeps briefly, and asserts `elapsed()` does not decrease.

### `src/window_drag.rs`

#### Types / constants

- **`NSPoint`** (private, L200)
  - Signature: `struct NSPoint { x: f64, y: f64 }` (`#[cfg(target_os = "macos")]`)
  - Purpose: Local Foundation point struct for macOS window drag math.
  - Behavior: Used in `start_window_drag` frame calculations without pulling in `cocoa`.

- **`NSSize`** (private, L208)
  - Signature: `struct NSSize { width: f64, height: f64 }` (macOS)
  - Purpose: Local Foundation size struct for macOS restore geometry.
  - Behavior: Paired with `NSPoint` inside `NSRect`.

- **`NSRect`** (private, L216)
  - Signature: `struct NSRect { origin: NSPoint, size: NSSize }` (macOS)
  - Purpose: Local Foundation rect for explicit maximized-to-restored frame placement.
  - Behavior: Passed to `-[NSWindow setFrame:display:]`.

#### Functions / methods

##### Context: `module`

- **`window_layout_settled`** (pub, L20)
  - Signature: `pub fn window_layout_settled(expect_maximized: bool, is_maximized: bool, viewport: Size<Pixels>, actual: Size<Pixels>) -> bool`
  - Purpose: Decides when deferred UI (`RootView::content_ready`) may reveal the window.
  - Behavior: Requires GPUI viewport size to match platform window size; when a maximized open is expected, also waits until `is_maximized` is true.

- **`nudge_window_resize`** (pub, L37)
  - Signature: `pub fn nudge_window_resize(window: &Window)` (Windows)
  - Purpose: Re-syncs GPUI viewport after async maximize-on-open.
  - Behavior: Reads Win32 client rect and zoom state, then re-posts `WM_SIZE` with current dimensions so GPUI picks up the maximized client size without changing window state.

- **`nudge_window_resize`** (pub, L77)
  - Signature: `pub fn nudge_window_resize(_window: &Window)` (non-Windows)
  - Purpose: Portable no-op stub for the resize nudge API.
  - Behavior: Empty body on platforms that do not need WM_SIZE re-posting.

- **`set_window_cloaked`** (pub, L86)
  - Signature: `pub fn set_window_cloaked(window: &Window, cloaked: bool)` (Windows)
  - Purpose: Hides window visually during first paint without altering layout state.
  - Behavior: Sets DWM cloak attribute via `DwmSetWindowAttribute(DWMWA_CLOAK)` so compositor skips the window while it stays maximized/rendering off-screen.

- **`set_window_cloaked`** (pub, L120)
  - Signature: `pub fn set_window_cloaked(window: &Window, cloaked: bool)` (macOS)
  - Purpose: Hides window visually during first paint on macOS.
  - Behavior: Sets `NSWindow` alpha to 0 when cloaked and 1 when revealed; window keeps rendering while transparent.

- **`set_window_cloaked`** (pub, L150)
  - Signature: `pub fn set_window_cloaked(_window: &Window, _cloaked: bool)` (other platforms)
  - Purpose: Portable no-op for cloak API on Linux etc.
  - Behavior: Empty body.

- **`initial_window_bounds`** (pub, L161)
  - Signature: `pub fn initial_window_bounds(restored: Bounds<Pixels>, maximized: bool, maxed: Bounds<Pixels>) -> WindowBounds` (macOS)
  - Purpose: Chooses open bounds avoiding restore-then-maximize flicker on macOS.
  - Behavior: When maximized, opens windowed directly at saved maximized frame (`maxed`); otherwise uses `restored`.

- **`initial_window_bounds`** (pub, L174)
  - Signature: `pub fn initial_window_bounds(restored: Bounds<Pixels>, maximized: bool, _maxed: Bounds<Pixels>) -> WindowBounds` (non-macOS)
  - Purpose: Chooses open bounds for Windows/Linux maximize behavior.
  - Behavior: Returns `WindowBounds::Maximized(restored)` when maximized, else `WindowBounds::Windowed(restored)`.

- **`start_window_drag`** (pub, L191)
  - Signature: `pub fn start_window_drag(window: &Window, _maximized: bool, _restore_size: Size<Pixels>)` (non-macOS)
  - Purpose: Begins interactive window move from a custom drag region.
  - Behavior: Delegates to GPUI `window.start_window_move()`; OS restores maximized windows automatically when dragging.

- **`start_window_drag`** (pub, L230)
  - Signature: `pub fn start_window_drag(window: &Window, maximized: bool, restore_size: Size<Pixels>)` (macOS)
  - Purpose: AppKit-backed window drag when GPUI move is unavailable.
  - Behavior: When maximized with valid restore size, computes new frame keeping cursor relative position on title bar, applies `setFrame`, then calls `performWindowDragWithEvent:` on the current event.

- **`settled_requires_viewport_to_match_window`** (private, L305)
  - Signature: `fn settled_requires_viewport_to_match_window()` (test)
  - Purpose: Tests viewport/actual size matching requirement.
  - Behavior: Smaller viewport than actual returns false; equal sizes return true when maximize not expected.

- **`settled_waits_for_maximize_when_expected`** (private, L314)
  - Signature: `fn settled_waits_for_maximize_when_expected()` (test)
  - Purpose: Tests maximize wait logic in `window_layout_settled`.
  - Behavior: When `expect_maximized`, requires `is_maximized` even if sizes already match.


### `src/window_frame.rs`

#### Types / constants

- **`MAC_TRAFFIC_LIGHT_CLEARANCE`** (private, L19)
  - Signature: `const MAC_TRAFFIC_LIGHT_CLEARANCE: f32 = 90.0`
  - Purpose: Left toolbar inset reserving space for macOS traffic lights.
  - Behavior: Used by `toolbar_left_padding` on macOS only.

- **`DEFAULT_LEFT_PADDING`** (private, L20)
  - Signature: `const DEFAULT_LEFT_PADDING: f32 = 12.0`
  - Purpose: Baseline left toolbar padding on non-macOS platforms.
  - Behavior: Minimum left padding when no GNOME left-side caption buttons are cached.

- **`WIN_CAPTION_BUTTON_WIDTH`** (private, L22)
  - Signature: `const WIN_CAPTION_BUTTON_WIDTH: f32 = 46.0`
  - Purpose: Hit width of each Windows Fluent caption button.
  - Behavior: Three buttons reserved in `right_controls_reserved_width`.

- **`GTK_CAPTION_SLOT`** (private, L24)
  - Signature: `const GTK_CAPTION_SLOT: f32 = 36.0`
  - Purpose: Adwaita/GTK circular caption button slot width.
  - Behavior: Used in Linux CSD button layout and width calculations.

- **`GTK_CAPTION_GAP`** (private, L25)
  - Signature: `const GTK_CAPTION_GAP: f32 = 4.0`
  - Purpose: Spacing between Linux CSD caption buttons.
  - Behavior: Applied in `caption_side_width` and GTK-style control row gap.

- **`CLIENT_SIDE_SHADOW`** (private, L26)
  - Signature: `const CLIENT_SIDE_SHADOW: f32 = 10.0`
  - Purpose: CSD outer shadow/inset and resize hit margin in pixels.
  - Behavior: Drives client inset, resize edge detection, and shadow padding in `wrap_client_decorations`.

- **`CLIENT_SIDE_ROUNDING`** (private, L27)
  - Signature: `const CLIENT_SIDE_ROUNDING: f32 = 10.0`
  - Purpose: Corner radius for client-side decorated windows.
  - Behavior: Applied to outer/backdrop and inner content unless edge is tiled flush.

- **`CLIENT_SIDE_BORDER`** (private, L28)
  - Signature: `const CLIENT_SIDE_BORDER: f32 = 1.0`
  - Purpose: Border thickness for CSD inner frame.
  - Behavior: Drawn per non-tiled edge inside `wrap_client_decorations`.

- **`CaptionLayout`** (private, L32)
  - Signature: `struct CaptionLayout { left: Vec<CaptionKind>, right: Vec<CaptionKind> }`
  - Purpose: Which caption buttons appear on each titlebar side.
  - Behavior: Parsed from GNOME `button-layout`; cached in `CAPTION_LAYOUT`.

- **`CaptionKind`** (private, L38)
  - Signature: `enum CaptionKind { Minimize, Maximize, Close }`
  - Purpose: Logical caption button kinds for layout and rendering.
  - Behavior: Mapped from gsettings tokens and to UI buttons/actions.

- **`CAPTION_LAYOUT`** (private, L45)
  - Signature: `static CAPTION_LAYOUT: RwLock<Option<CaptionLayout>>`
  - Purpose: Process-wide cache of desktop button layout.
  - Behavior: Populated lazily by `cached_caption_layout` and refreshed via `refresh_caption_layout`.

- **`CaptionWindowControls`** (private, L413)
  - Signature: `struct CaptionWindowControls { buttons, maximized, gtk_style }`
  - Purpose: Renders a row of caption buttons for Windows or Linux CSD.
  - Behavior: Built by `windows()` or `linux()` factories; rendered via `RenderOnce`.

- **`CaptionButton`** (private, L468)
  - Signature: `struct CaptionButton { kind, gtk_style, index }`
  - Purpose: Single minimize/maximize/restore/close caption control.
  - Behavior: Maps to icons, GPUI `WindowControlArea`, and window actions.

- **`CaptionButtonKind`** (private, L475)
  - Signature: `enum CaptionButtonKind { Minimize, Restore, Maximize, Close }`
  - Purpose: Concrete button variant including restore state when maximized.
  - Behavior: Selected in `CaptionWindowControls::render` based on kind and `maximized`.

- **`CaptionAction`** (private, L574)
  - Signature: `enum CaptionAction { Minimize, Zoom, Close }`
  - Purpose: Window action triggered by a caption button click.
  - Behavior: Minimize/zoom/close map to GPUI window APIs and `cx.quit()` for close.

#### Functions / methods

##### Context: `module`

- **`main_titlebar_options`** (pub, L47)
  - Signature: `pub fn main_titlebar_options() -> TitlebarOptions`
  - Purpose: GPUI titlebar configuration for the transparent main toolbar strip.
  - Behavior: Sets title "BGMail", transparent appearance, and macOS traffic-light position when applicable.

- **`main_window_decorations`** (pub, L57)
  - Signature: `pub fn main_window_decorations() -> Option<WindowDecorations>`
  - Purpose: Requests client-side decorations on Linux/FreeBSD.
  - Behavior: Returns `Some(WindowDecorations::Client)` on Linux/FreeBSD; `None` elsewhere (native SSD/macOS traffic lights).

- **`traffic_light_position`** (private, L66)
  - Signature: `fn traffic_light_position() -> Option<gpui::Point<Pixels>>` (macOS)
  - Purpose: Positions native traffic lights for transparent titlebar.
  - Behavior: Returns point (12, 16) in pixels.

- **`traffic_light_position`** (private, L71)
  - Signature: `fn traffic_light_position() -> Option<gpui::Point<Pixels>>` (non-macOS)
  - Purpose: Stub when native traffic lights are absent.
  - Behavior: Returns `None`.

- **`toolbar_left_padding`** (pub, L75)
  - Signature: `pub fn toolbar_left_padding() -> Pixels`
  - Purpose: Left inset for toolbar content clearing window controls.
  - Behavior: macOS uses traffic-light clearance; other platforms use default padding (Linux may add left caption width separately via `left_controls_reserved_width`).

- **`right_controls_reserved_width`** (pub, L84)
  - Signature: `pub fn right_controls_reserved_width() -> Pixels`
  - Purpose: Toolbar right inset reserving custom caption buttons.
  - Behavior: Windows reserves three rectangular buttons; Linux/FreeBSD uses cached right-side GTK layout width; zero elsewhere.

- **`left_controls_reserved_width`** (pub, L95)
  - Signature: `pub fn left_controls_reserved_width() -> Pixels`
  - Purpose: Toolbar left inset for GNOME-style left-side caption buttons.
  - Behavior: Non-zero on Linux/FreeBSD from cached left layout; zero on other platforms.

- **`caption_side_width`** (private, L103)
  - Signature: `fn caption_side_width(buttons: &[CaptionKind]) -> Pixels`
  - Purpose: Computes horizontal space for a caption button group.
  - Behavior: Zero when empty; otherwise `n * GTK_CAPTION_SLOT + (n-1) * GTK_CAPTION_GAP`.

- **`uses_custom_caption_buttons`** (private, L111)
  - Signature: `fn uses_custom_caption_buttons(window: &Window) -> bool`
  - Purpose: Whether this window draws custom caption controls.
  - Behavior: True on Windows always; on Linux/FreeBSD when decorations are client-side.

- **`refresh_caption_layout`** (pub, L123)
  - Signature: `pub fn refresh_caption_layout()`
  - Purpose: Re-reads GNOME button layout when the main window activates.
  - Behavior: No-op off Linux/FreeBSD; otherwise shells `gsettings` and updates `CAPTION_LAYOUT` cache.

- **`cached_caption_layout`** (private, L133)
  - Signature: `fn cached_caption_layout() -> CaptionLayout`
  - Purpose: Returns cached desktop caption layout, populating on first use.
  - Behavior: Reads RwLock cache or computes via `linux_caption_layout_from_desktop(None)` and stores result.

- **`linux_caption_layout_from_desktop`** (private, L148)
  - Signature: `fn linux_caption_layout_from_desktop(controls: Option<WindowControls>) -> CaptionLayout`
  - Purpose: Builds Linux CSD caption layout from gsettings and desktop heuristics.
  - Behavior: Parses `button-layout` when available; GNOME-like fallback close-only; KDE-style full trio otherwise; optional compositor capability filtering.

- **`filter_layout_by_controls`** (private, L174)
  - Signature: `fn filter_layout_by_controls(layout: &mut CaptionLayout, controls: WindowControls)`
  - Purpose: Removes caption buttons unsupported by the compositor.
  - Behavior: Retains minimize/maximize only when reported available; close always kept.

- **`parse_button_layout`** (private, L185)
  - Signature: `fn parse_button_layout(raw: &str) -> CaptionLayout`
  - Purpose: Parses GNOME `button-layout` string (`left:right`).
  - Behavior: Trims quotes; splits on `:`; parses comma-separated sides via `parse_button_side`.

- **`parse_button_side`** (private, L197)
  - Signature: `fn parse_button_side(side: &str) -> Vec<CaptionKind>`
  - Purpose: Parses one side of a button-layout string.
  - Behavior: Maps `minimize`, `maximize`, `close` tokens; ignores unknown tokens like `appmenu`.

- **`is_gnome_like_desktop`** (private, L208)
  - Signature: `fn is_gnome_like_desktop() -> bool`
  - Purpose: Detects GNOME/Unity/COSMIC desktops for layout defaults.
  - Behavior: Checks `XDG_CURRENT_DESKTOP` colon-separated parts case-insensitively.

- **`read_gnome_button_layout`** (private, L220)
  - Signature: `fn read_gnome_button_layout() -> Option<String>`
  - Purpose: Reads `org.gnome.desktop.wm.preferences button-layout` via gsettings.
  - Behavior: Returns trimmed stdout on success; `None` on failure or empty output.

- **`render_right_window_controls`** (pub, L237)
  - Signature: `pub fn render_right_window_controls(window: &mut Window) -> Option<AnyElement>`
  - Purpose: Builds right-side caption controls when needed.
  - Behavior: Returns `None` when custom captions unused; Windows full trio; Linux right-side layout from `linux_layout_for_window`.

- **`render_left_window_controls`** (pub, L252)
  - Signature: `pub fn render_left_window_controls(window: &mut Window) -> Option<AnyElement>`
  - Purpose: Builds left-side caption controls for GNOME layouts like `close:minimize,maximize`.
  - Behavior: Linux/FreeBSD client-decorated windows only; `None` when left layout empty.

- **`linux_layout_for_window`** (private, L266)
  - Signature: `fn linux_layout_for_window(window: &Window) -> CaptionLayout`
  - Purpose: Combines cached gsettings layout with live compositor restrictions.
  - Behavior: Starts from cache; when Wayland reports min/max unavailable, filters via `filter_layout_by_controls`.

- **`wrap_client_decorations`** (pub, L280)
  - Signature: `pub fn wrap_client_decorations(content: AnyElement, window: &mut Window, border_color: Hsla) -> AnyElement`
  - Purpose: Wraps main UI in Zed-style CSD chrome (shadow, border, resize handles).
  - Behavior: No-op for server-side decorations. For client decorations, sets client inset, draws resize hit canvas with cursor changes, handles edge resize on mouse down, and nests bordered/shadowed rounded content respecting tiling flags.

- **`resize_edge`** (private, L380)
  - Signature: `fn resize_edge(pos: Point<Pixels>, shadow_size: Pixels, size: Size<Pixels>) -> Option<ResizeEdge>`
  - Purpose: Hit-tests pointer position against CSD resize margins.
  - Behavior: Checks corners first within `shadow_size` band, then edges; returns `None` in interior.

- **`cursor_for_resize_edge`** (private, L403)
  - Signature: `fn cursor_for_resize_edge(edge: ResizeEdge) -> CursorStyle`
  - Purpose: Maps resize edge to GPUI cursor style.
  - Behavior: Vertical, horizontal, or diagonal resize cursors per edge enum.

- **`main_titlebar_keeps_macos_traffic_lights_only_on_macos`** (private, L638)
  - Signature: `fn main_titlebar_keeps_macos_traffic_lights_only_on_macos()` (test)
  - Purpose: Validates titlebar options per platform.
  - Behavior: Asserts transparent title and BGMail title; traffic-light position only on macOS.

- **`toolbar_padding_reserves_space_for_macos_traffic_lights_only`** (private, L652)
  - Signature: `fn toolbar_padding_reserves_space_for_macos_traffic_lights_only()` (test)
  - Purpose: Checks left padding platform behavior.
  - Behavior: macOS equals clearance constant; elsewhere at least default padding.

- **`main_window_requests_client_decorations_on_linux`** (private, L664)
  - Signature: `fn main_window_requests_client_decorations_on_linux()` (test)
  - Purpose: Ensures Linux/FreeBSD request client decorations.
  - Behavior: Compares `main_window_decorations()` to `Some(Client)` on those targets.

- **`right_controls_reserve_width_on_windows`** (private, L674)
  - Signature: `fn right_controls_reserve_width_on_windows()` (test)
  - Purpose: Validates reserved caption width on Windows/macOS.
  - Behavior: Windows equals three button widths; macOS zero.

- **`parse_button_layout_gnome_close_only`** (private, L685)
  - Signature: `fn parse_button_layout_gnome_close_only()` (test)
  - Purpose: Parses GNOME default close-only layout.
  - Behavior: `:close` yields empty left and single Close on right.

- **`parse_button_layout_full_right`** (private, L692)
  - Signature: `fn parse_button_layout_full_right()` (test)
  - Purpose: Parses standard right-side trio layout.
  - Behavior: Expects minimize, maximize, close on right with empty left.

- **`parse_button_layout_close_on_left`** (private, L706)
  - Signature: `fn parse_button_layout_close_on_left()` (test)
  - Purpose: Parses split left/right caption layout.
  - Behavior: Close on left; minimize and maximize on right.

- **`parse_button_layout_ignores_menu_and_appmenu`** (private, L716)
  - Signature: `fn parse_button_layout_ignores_menu_and_appmenu()` (test)
  - Purpose: Ensures unknown layout tokens are ignored.
  - Behavior: `appmenu:minimize,maximize,close` yields three right buttons and empty left.

- **`filter_layout_hides_unavailable_min_max`** (private, L723)
  - Signature: `fn filter_layout_hides_unavailable_min_max()` (test)
  - Purpose: Verifies compositor capability filtering.
  - Behavior: When min/max unavailable, only Close remains.

- **`caption_side_width_scales_with_button_count`** (private, L738)
  - Signature: `fn caption_side_width_scales_with_button_count()` (test)
  - Purpose: Ensures width grows with button count.
  - Behavior: Empty side zero; two buttons wider than one.

- **`caption_buttons_map_to_explicit_actions`** (private, L746)
  - Signature: `fn caption_buttons_map_to_explicit_actions()` (test)
  - Purpose: Validates caption button action mapping.
  - Behavior: Minimize→Minimize, maximize/restore→Zoom, close→Close.

- **`caption_buttons_map_to_native_control_areas`** (private, L754)
  - Signature: `fn caption_buttons_map_to_native_control_areas()` (test)
  - Purpose: Validates GPUI native control area mapping.
  - Behavior: Min→Min, max/restore→Max, close→Close.

- **`resize_edge_detects_corners_and_sides`** (private, L774)
  - Signature: `fn resize_edge_detects_corners_and_sides()` (test)
  - Purpose: Spot-checks resize hit testing.
  - Behavior: Corner maps to TopLeft; interior None; top edge detected mid-width.

##### Context: `CaptionWindowControls`

- **`windows`** (private, L421)
  - Signature: `fn windows(maximized: bool) -> Self`
  - Purpose: Factory for Windows Fluent caption strip.
  - Behavior: Fixed minimize/maximize/close order, rectangular style (`gtk_style: false`).

- **`linux`** (private, L433)
  - Signature: `fn linux(buttons: Vec<CaptionKind>, maximized: bool) -> Self`
  - Purpose: Factory for Linux Adwaita-style caption row.
  - Behavior: Uses provided button list and circular GTK styling.

##### Context: `RenderOnce for CaptionWindowControls`

- **`render`** (private, L443)
  - Signature: `fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement`
  - Purpose: Renders horizontal caption button row.
  - Behavior: Builds flex row; adds GTK gap/padding when `gtk_style`; chooses restore vs maximize icon when maximized; assigns stable ids per index.

##### Context: `CaptionButton`

- **`minimize`** (private, L483)
  - Signature: `fn minimize() -> Self`
  - Purpose: Builder for minimize caption button.
  - Behavior: Sets `CaptionButtonKind::Minimize`.

- **`restore`** (private, L491)
  - Signature: `fn restore() -> Self`
  - Purpose: Builder for restore (un-maximize) caption button.
  - Behavior: Sets `CaptionButtonKind::Restore`.

- **`maximize`** (private, L499)
  - Signature: `fn maximize() -> Self`
  - Purpose: Builder for maximize caption button.
  - Behavior: Sets `CaptionButtonKind::Maximize`.

- **`close`** (private, L507)
  - Signature: `fn close() -> Self`
  - Purpose: Builder for close caption button.
  - Behavior: Sets `CaptionButtonKind::Close`.

- **`with_style`** (private, L515)
  - Signature: `fn with_style(mut self, gtk_style: bool) -> Self`
  - Purpose: Selects GTK circular vs Windows rectangular styling.
  - Behavior: Sets `gtk_style` flag on builder.

- **`with_index`** (private, L520)
  - Signature: `fn with_index(mut self, index: usize) -> Self`
  - Purpose: Assigns stable index for element id disambiguation.
  - Behavior: Stores index used by `id()`.

- **`id`** (private, L525)
  - Signature: `fn id(self) -> SharedString`
  - Purpose: Stable GPUI element id for a caption button.
  - Behavior: Formats `"{kind}-{index}"` (minimize, restore, maximize, close).

- **`icon`** (private, L539)
  - Signature: `fn icon(self) -> IconName`
  - Purpose: Icon for the caption button kind.
  - Behavior: WindowMinimize/Restore/Maximize icons; Close uses Clear icon.

- **`action`** (private, L548)
  - Signature: `fn action(self) -> CaptionAction`
  - Purpose: Window action associated with the button.
  - Behavior: Minimize→Minimize; Restore/Maximize→Zoom; Close→Close.

- **`control_area`** (private, L556)
  - Signature: `fn control_area(self) -> WindowControlArea`
  - Purpose: Native window control hit region for the platform shell.
  - Behavior: Maps actions to Min/Max/Close `WindowControlArea` values.

- **`activate`** (private, L564)
  - Signature: `fn activate(self, window: &mut Window, cx: &mut App)`
  - Purpose: Executes caption button click behavior.
  - Behavior: Minimize calls `minimize_window`; Zoom calls `zoom_window`; Close calls `cx.quit()`.

##### Context: `RenderOnce for CaptionButton`

- **`render`** (private, L581)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Draws one caption button with hover and native control area.
  - Behavior: Close button hovers error color; others use element hover. GTK style uses circular compact target; Windows style uses full-height rectangular strip. Registers `window_control_area` and click handler calling `activate`.

