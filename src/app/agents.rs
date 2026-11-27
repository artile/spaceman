//! The agent workspace at runtime (`[agents] workspace`): an agent window that raises itself
//! there never pulls the user along. Only explicit input switches to it: a click (the Dock, a
//! notification, the HUD) or a Command chord such as Command-Tab.
//!
//! Routing agent windows there is an ordinary `route` rule with `agent = true`; this module only
//! decides what focus does about them.
use super::commands::WorkspaceFocusOutcome;
use super::*;

/// How recent a click must be to explain an activation: a Dock or notification click
/// activates its app within a few hundred milliseconds.
const CLICK_WINDOW: f64 = 0.6;
/// How recent a key chord must be: Command-Tab activates when Command goes up.
const CHORD_WINDOW: f64 = 1.0;

/// Whether `input` explains an activation as the user's own.
pub(super) fn explicit(input: &macos::RecentInput) -> bool {
    input.mouse_down < CLICK_WINDOW
        || (input.key_down < CHORD_WINDOW
            && (input.command_held || input.modifiers_changed < CHORD_WINDOW))
}

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Gives focus back to the workspace the user is on, so their keys never land in a hidden
    /// agent window. The agent's focus is then the stale focus a switch leaves behind, and focus
    /// follow ignores it until Accessibility reports a different one.
    pub(super) fn return_focus_from_agents(
        &mut self,
        visible: &[ResolvedWorkspace],
        focused: Option<crate::PhysicalWindowId>,
        frontmost: Option<crate::AppInstanceId>,
    ) {
        let current = self.focus.history().current;
        let Some(workspace) = current
            .filter(|workspace| {
                visible
                    .iter()
                    .any(|resolved| resolved.workspace == *workspace)
            })
            .or_else(|| visible.first().map(|resolved| resolved.workspace))
        else {
            return;
        };
        let windows = self.workspaces.windows(workspace);
        if let WorkspaceFocusOutcome::Failed =
            self.focus_workspace_entry(workspace, &windows, focused, frontmost, false)
        {
            log_event!("agent hold: focus back to workspace {workspace} failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(key: f64, click: f64, modifiers: f64, command: bool) -> macos::RecentInput {
        macos::RecentInput {
            key_down: key,
            mouse_down: click,
            modifiers_changed: modifiers,
            command_held: command,
        }
    }

    #[test]
    fn clicks_and_command_chords_are_explicit() {
        assert!(explicit(&input(9.0, 0.2, 9.0, false)));
        assert!(explicit(&input(0.3, 9.0, 0.3, false)));
        assert!(explicit(&input(0.3, 9.0, 5.0, true)));
    }

    #[test]
    fn idle_input_and_plain_typing_are_not() {
        assert!(!explicit(&input(9.0, 9.0, 9.0, false)));
        assert!(!explicit(&input(0.2, 3.0, 4.0, false)));
        assert!(!explicit(&input(3.0, 1.2, 0.1, false)));
    }
}
