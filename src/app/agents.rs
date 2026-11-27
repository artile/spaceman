//! The agent workspace at runtime (`[agents] workspace`): an agent window that raises itself
//! there never pulls the user along. Only explicit input switches to it: a click (the Dock, a
//! notification, the HUD) or a Command chord such as Command-Tab.
//!
//! Routing agent windows there is an ordinary `route` rule with `agent = true`; this module only
//! decides what focus does about them.
use super::commands::WorkspaceFocusOutcome;
use super::*;

/// How long before an activation a click explains it: a Dock or notification click activates
/// its app within a few hundred milliseconds, a slow app within a second.
const CLICK_WINDOW: f64 = 1.0;
/// A shortcut activates while Command is still down, within this long of its key.
const CHORD_WINDOW: f64 = 1.0;
/// Command-Tab (or an Option-Tab switcher) activates when the modifier goes up, however long the
/// user browsed the switcher before.
const RELEASE_WINDOW: f64 = 0.5;
const SWITCHER_BROWSE: f64 = 10.0;
/// Input a little after the activation notification still caused it (event delivery order).
const LATE: f64 = 0.25;

/// Whether `input` explains an activation as the user's own. Each input time counts back from
/// the activation, not from the cycle that reads it: a cycle can run a second later.
pub(super) fn explicit(input: &macos::RecentInput) -> bool {
    let activated = input.activated.unwrap_or(0.0);
    let within = |since: f64, window: f64| (-LATE..window).contains(&(since - activated));
    let click = within(input.mouse_down, CLICK_WINDOW);
    let chord = input.command_held && within(input.key_down, CHORD_WINDOW);
    // A switcher's last key (Tab) comes before the modifier goes up; in typing, a letter follows
    // the Shift release instead.
    let release = within(input.modifiers_changed, RELEASE_WINDOW)
        && input.key_down >= input.modifiers_changed - 0.01
        && within(input.key_down, SWITCHER_BROWSE);
    click || chord || release
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
            activated: None,
        }
    }

    fn at(input: macos::RecentInput, activated: f64) -> macos::RecentInput {
        macos::RecentInput {
            activated: Some(activated),
            ..input
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
        // Shift let go 0.3 s ago and a letter typed after it.
        assert!(!explicit(&input(0.05, 3.0, 0.3, false)));
    }

    #[test]
    fn command_tab_counts_from_the_activation_however_late_the_cycle_reads_it() {
        // Logged live: Tab 1.46 s, Command up 1.46 s before the cycle, activation 1.40 s before.
        assert!(explicit(&at(input(1.46, 12.46, 1.46, false), 1.40)));
        // The user browsed the switcher for 3 s, then let Command go.
        assert!(explicit(&at(input(3.2, 30.0, 0.15, false), 0.1)));
        // A Dock click 1.5 s before the cycle, 0.4 s before the activation.
        assert!(explicit(&at(input(40.0, 1.53, 70.0, false), 1.13)));
    }

    #[test]
    fn input_long_before_the_activation_is_not() {
        // An agent window raised itself 0.1 s ago; the last keys and clicks were seconds before.
        assert!(!explicit(&at(input(2.35, 10.35, 2.35, false), 0.1)));
        // Typing after an agent's activation never explains it.
        assert!(!explicit(&at(input(0.05, 9.0, 9.0, false), 2.0)));
    }
}
