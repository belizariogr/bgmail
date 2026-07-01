//! Zed-style command palette: filterable list of available commands.

use ui::TextInput;

use crate::commands::{command_matches_query, palette_commands, CommandEntry, CommandId};

/// State for the command palette overlay.
#[derive(Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_ix: usize,
    pub input: Option<gpui::Entity<TextInput>>,
}

impl CommandPaletteState {
    pub fn filtered_entries(
        &self,
        language: crate::locale::Language,
        ctx: &crate::commands::CommandContext,
    ) -> Vec<CommandEntry> {
        palette_commands(language, ctx)
            .into_iter()
            .filter(|entry| command_matches_query(entry.label.as_ref(), &self.query))
            .collect()
    }

    pub fn clamp_selection(&mut self, count: usize) {
        if count == 0 {
            self.selected_ix = 0;
        } else if self.selected_ix >= count {
            self.selected_ix = count - 1;
        }
    }

    pub fn on_query_change(&mut self, query: String) {
        self.query = query;
        self.selected_ix = 0;
    }

    pub fn move_selection(&mut self, delta: isize, entry_count: usize) -> bool {
        if entry_count == 0 {
            return false;
        }
        let next = self.selected_ix as isize + delta;
        let clamped = next.clamp(0, entry_count as isize - 1) as usize;
        if clamped != self.selected_ix {
            self.selected_ix = clamped;
            true
        } else {
            false
        }
    }

    pub fn selected_command<'a>(&self, entries: &'a [CommandEntry]) -> Option<&'a CommandId> {
        entries.get(self.selected_ix).map(|entry| &entry.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_clamps_to_last_entry() {
        let mut state = CommandPaletteState::default();
        state.selected_ix = 5;
        state.clamp_selection(3);
        assert_eq!(state.selected_ix, 2);
    }
}
