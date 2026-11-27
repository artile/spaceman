//! The agent workspace: `agent = true` rules, `ctl agent claim`, and the focus-follow hold.
use super::*;

const AGENTS: &str = "version = 1
[agents]
workspace = 9
label = 'AI'

[[rules]]
agent = true
action = 'route'
workspace = 9
windows = 'all'
";

fn nine() -> WorkspaceId {
    WorkspaceId::new(9).unwrap()
}

fn agent_window(pid: i32, id: u32) -> crate::PhysicalWindowId {
    crate::PhysicalWindowId {
        app: AppInstanceId {
            pid,
            launched_at: 1,
        },
        window_id: id,
    }
}

fn idle() -> macos::RecentInput {
    macos::RecentInput {
        key_down: 30.0,
        mouse_down: 30.0,
        modifiers_changed: 30.0,
        command_held: false,
    }
}

fn agent_runtime(
    pid: i32,
    recent_input: Option<macos::RecentInput>,
) -> (
    WorkspaceRuntime<FakeBackend>,
    crate::PhysicalWindowId,
    crate::PhysicalWindowId,
) {
    let user = window(10);
    let agent = agent_window(pid, 20);
    let mut backend = FakeBackend {
        focused: Some(agent.clone()),
        frontmost: Some(agent.app.clone()),
        recent_input,
        ..FakeBackend::default()
    };
    backend.frames.insert(user.clone(), frame(50));
    backend
        .frames
        .insert(agent.clone(), main_parked_frame(frame(300)));
    let mut observation =
        observation_for_windows("com.example.User", &user, &[(user.clone(), frame(50))]);
    observation.applications.extend(
        observation_for_windows(
            "com.example.Agent",
            &agent,
            &[(agent.clone(), main_parked_frame(frame(300)))],
        )
        .applications,
    );
    backend.observation = Some(observation);
    let mut runtime = runtime_with_parked(backend, [(agent.clone(), frame(300))]);
    runtime.replace_config(Config::parse(AGENTS).unwrap());
    bind(&mut runtime, WorkspaceId::ONE, &user);
    bind(&mut runtime, nine(), &agent);
    runtime.record_focused_workspace(WorkspaceId::ONE);
    (runtime, user, agent)
}

#[test]
fn a_claimed_process_window_routes_to_the_agent_workspace() {
    crate::agents::claim_for_test(7001);
    let created = agent_window(7001, 11);
    let mut backend = FakeBackend {
        observation: Some(observation_for("com.example.Agent", &created, frame(100))),
        ..FakeBackend::default()
    };
    backend.frames.insert(created.clone(), frame(100));
    let mut runtime = runtime_with_config(backend, Config::parse(AGENTS).unwrap());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&created), Some(nine()));
}

#[test]
fn an_unclaimed_process_window_ignores_the_agent_rule() {
    let created = agent_window(7002, 11);
    let mut backend = FakeBackend {
        observation: Some(observation_for("com.example.Agent", &created, frame(100))),
        ..FakeBackend::default()
    };
    backend.frames.insert(created.clone(), frame(100));
    let mut runtime = runtime_with_config(backend, Config::parse(AGENTS).unwrap());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_ne!(runtime.workspaces.workspace_of(&created), Some(nine()));
}

#[test]
fn an_agent_window_raising_itself_never_switches_and_focus_returns() {
    let (mut runtime, user, agent) = agent_runtime(7003, Some(idle()));

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();
    // Accessibility still reports the agent window on the next cycle: it stays held.
    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(!status.active.contains(&nine()));
    assert_eq!(
        runtime.workspaces.active_for(&DisplaySelector::Main),
        Some(WorkspaceId::ONE)
    );
    assert_eq!(runtime.workspaces.workspace_of(&agent), Some(nine()));
    assert!(runtime.parked.contains_key(&agent));
    assert!(
        runtime
            .backend
            .calls
            .iter()
            .any(|call| matches!(call, BackendCall::Focus(window) if *window == user))
    );
}

#[test]
fn unknown_input_holds_too() {
    let (mut runtime, _user, _agent) = agent_runtime(7004, None);

    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(!status.active.contains(&nine()));
}

#[test]
fn a_click_on_an_agent_window_still_follows() {
    let click = macos::RecentInput {
        mouse_down: 0.1,
        ..idle()
    };
    let (mut runtime, _user, _agent) = agent_runtime(7005, Some(click));

    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(status.active.contains(&nine()));
}

#[test]
fn without_an_agents_table_focus_follows_as_before() {
    let (mut runtime, _user, _agent) = agent_runtime(7006, Some(idle()));
    runtime.replace_config(Config::default());

    let status = runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert!(status.active.contains(&nine()));
}

#[test]
fn claim_moves_the_open_windows_of_the_process_to_the_agent_route() {
    crate::agents::claim_for_test(7007);
    let user = window(10);
    let agent = agent_window(7007, 30);
    let mut backend = FakeBackend {
        focused: Some(user.clone()),
        frontmost: Some(user.app.clone()),
        ..FakeBackend::default()
    };
    backend.frames.insert(user.clone(), frame(50));
    backend.frames.insert(agent.clone(), frame(400));
    let mut runtime = runtime_with_config(backend, Config::parse(AGENTS).unwrap());
    bind(&mut runtime, WorkspaceId::ONE, &user);
    bind(&mut runtime, WorkspaceId::ONE, &agent);

    runtime
        .route_agent(7007, outputs(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&agent), Some(nine()));
    assert_eq!(
        runtime.workspaces.workspace_of(&user),
        Some(WorkspaceId::ONE)
    );
}

#[test]
fn the_menu_shows_the_agent_label() {
    let (runtime, _user, _agent) = agent_runtime(7008, Some(idle()));

    let menu = runtime.status(0, &outputs()).menu;

    let labels = menu
        .iter()
        .flat_map(|display| &display.workspaces)
        .map(|entry| (entry.workspace, entry.label.clone()))
        .collect::<Vec<_>>();
    assert!(labels.contains(&(nine(), Some("AI".into()))));
    assert!(labels.contains(&(WorkspaceId::ONE, None)));
}

#[test]
fn an_expected_launch_routes_its_first_window_straight_to_the_agent_workspace() {
    crate::agents::expect("com.example.Launched").unwrap();
    let now = crate::config::now().as_millis() as u64;
    let created = crate::PhysicalWindowId {
        app: AppInstanceId {
            pid: 7011,
            launched_at: now,
        },
        window_id: 12,
    };
    let mut backend = FakeBackend {
        observation: Some(observation_for(
            "com.example.Launched",
            &created,
            frame(100),
        )),
        ..FakeBackend::default()
    };
    backend.frames.insert(created.clone(), frame(100));
    let mut runtime = runtime_with_config(backend, Config::parse(AGENTS).unwrap());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&created), Some(nine()));
    assert!(crate::agents::is_agent(7011));
    assert!(crate::agents::release(7011));
}

#[test]
fn an_agent_window_reopened_at_a_parking_spot_goes_to_the_agent_workspace_unseen() {
    crate::agents::claim_for_test(7012);
    let parked = main_parked_frame(outputs().main().frame);
    let reopened = agent_window(7012, 13);
    let mut backend = FakeBackend {
        observation: Some(observation_for("com.example.Agent", &reopened, parked)),
        ..FakeBackend::default()
    };
    backend.frames.insert(reopened.clone(), parked);
    let mut runtime = runtime_with_config(backend, Config::parse(AGENTS).unwrap());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&reopened), Some(nine()));
    let displays = outputs().frames().collect::<Vec<_>>();
    assert!(
        !runtime.backend.calls.iter().any(|call| match call {
            BackendCall::SetPosition(window, frame) | BackendCall::Apply(window, frame) => {
                *window == reopened && displays.iter().any(|d| d.contains_center(*frame))
            }
            _ => false,
        }),
        "{:?}",
        runtime.backend.calls
    );
}

#[test]
fn a_helper_window_accessibility_cannot_reach_is_never_adopted() {
    // An app's 30-point helper strip after a display change: listed by the window server, gone
    // to Accessibility. Owning it would leave a parked window no restore can bring back.
    crate::agents::claim_for_test(7013);
    let parked = main_parked_frame(outputs().main().frame);
    let helper = agent_window(7013, 14);
    let mut backend = FakeBackend {
        observation: Some(observation_for("com.example.Agent", &helper, parked)),
        frame_outcome: Some(macos::WindowAccessError::Gone),
        ..FakeBackend::default()
    };
    backend.frames.insert(helper.clone(), parked);
    let mut runtime = runtime_with_config(backend, Config::parse(AGENTS).unwrap());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    assert_eq!(runtime.workspaces.workspace_of(&helper), None);
    assert!(!runtime.parked.contains_key(&helper));
}

#[test]
fn a_user_window_at_a_parking_spot_is_rescued_even_when_its_rule_routes_it_away() {
    let routed = "version = 1
[agents]
workspace = 9

[[rules]]
bundle_id = 'com.example.User'
action = 'route'
workspace = 9
windows = 'all'
";
    let parked = main_parked_frame(outputs().main().frame);
    let reopened = window(15);
    let mut backend = FakeBackend {
        observation: Some(observation_for("com.example.User", &reopened, parked)),
        ..FakeBackend::default()
    };
    backend.frames.insert(reopened.clone(), parked);
    let mut runtime = runtime_with_config(backend, Config::parse(routed).unwrap());

    runtime.cycle(outputs(), &AtomicBool::new(false)).unwrap();

    let displays = outputs().frames().collect::<Vec<_>>();
    assert!(
        runtime.backend.calls.iter().any(|call| matches!(
            call,
            BackendCall::SetPosition(window, frame)
                if *window == reopened && displays.iter().any(|d| d.contains_center(*frame))
        )),
        "{:?}",
        runtime.backend.calls
    );
}
