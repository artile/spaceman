//! `[agents]`: the workspace AI agents work on, out of the user's way.

use serde::Deserialize;

use crate::WorkspaceId;

/// The longest `label` the HUD and the menu draw.
const LABEL_MAX: usize = 8;

/// The `[agents]` table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentsConfig {
    /// Focus follow never switches to this workspace on its own: an agent window that raises
    /// itself there leaves the user where they are. Only explicit input switches to it.
    pub workspace: Option<WorkspaceId>,
    /// A short name the HUD and the menu bar menu show for that workspace instead of its id.
    pub label: Option<String>,
}

impl AgentsConfig {
    /// Whether `workspace` is the agent workspace.
    #[must_use]
    pub fn guards(&self, workspace: WorkspaceId) -> bool {
        self.workspace == Some(workspace)
    }

    /// The label to show for `workspace`, if it is the labelled agent workspace.
    #[must_use]
    pub fn label_for(&self, workspace: WorkspaceId) -> Option<&str> {
        self.label.as_deref().filter(|_| self.guards(workspace))
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawAgents {
    workspace: Option<WorkspaceId>,
    label: Option<String>,
}

impl RawAgents {
    pub(super) fn parse(self) -> Result<AgentsConfig, String> {
        let label = match self.label.map(|label| label.trim().to_owned()) {
            Some(label) if label.is_empty() => return Err("agents.label cannot be empty".into()),
            Some(label) if label.chars().count() > LABEL_MAX => {
                return Err(format!(
                    "agents.label is longer than {LABEL_MAX} characters"
                ));
            }
            Some(_) if self.workspace.is_none() => {
                return Err("agents.label requires agents.workspace".into());
            }
            label => label,
        };
        Ok(AgentsConfig {
            workspace: self.workspace,
            label,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::WorkspaceId;
    use crate::config::Config;

    #[test]
    fn parses_the_agents_table_and_an_agent_only_rule() {
        let config = Config::parse(
            "version = 1\n[agents]\nworkspace = 9\nlabel = 'AI'\n\
             [[rules]]\nagent = true\naction = 'route'\nworkspace = 9\nwindows = 'all'",
        )
        .unwrap();
        let nine = WorkspaceId::new(9).unwrap();
        assert!(config.agents().guards(nine));
        assert!(!config.agents().guards(WorkspaceId::ONE));
        assert_eq!(config.agents().label_for(nine), Some("AI"));
        assert_eq!(config.agents().label_for(WorkspaceId::ONE), None);
        assert_eq!(config.agent_route(), Some(nine));
    }

    #[test]
    fn an_agent_rule_with_a_bundle_is_no_agent_route() {
        let config = Config::parse(
            "version = 1\n[[rules]]\nagent = true\nbundle_id = 'com.google.Chrome'\n\
             action = 'route'\nworkspace = 9",
        )
        .unwrap();
        assert_eq!(config.agent_route(), None);
    }

    #[test]
    fn rejects_bad_labels() {
        let parse = |agents: &str| Config::parse(&format!("version = 1\n[agents]\n{agents}"));
        assert_eq!(
            parse("workspace = 9\nlabel = ' '").unwrap_err(),
            "agents.label cannot be empty"
        );
        assert_eq!(
            parse("workspace = 9\nlabel = 'Agents at work'").unwrap_err(),
            "agents.label is longer than 8 characters"
        );
        assert_eq!(
            parse("label = 'AI'").unwrap_err(),
            "agents.label requires agents.workspace"
        );
        assert!(parse("workspace = 9\nunknown = 1").is_err());
    }
}
