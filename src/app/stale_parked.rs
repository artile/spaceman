//! Windows an application reopened at a Spaceman parking spot.
//!
//! Quitting an app while some of its windows are parked lets macOS save those parked frames, so
//! the relaunched app reopens them one pixel inside a display corner. Their centers are off
//! screen, so no observation lists them, and their physical identity is new, so no recovery
//! frame covers them. Revealing every such window (not only the focused one) lets observation,
//! recall and reconciliation adopt it like any other window, on the workspace it remembers.

use super::*;

/// Rescued windows on one display cascade by this much, so none hides another.
const RESCUE_CASCADE: i32 = 24;

impl<B: WorkspaceBackend> WorkspaceRuntime<B> {
    /// Reveal every window sitting exactly at a display's parking spot that Spaceman neither
    /// parked nor owns.
    pub(super) fn recover_stale_parked_windows(
        &mut self,
        outputs: &macos::DisplayOutputs,
        server_frames: &HashMap<u32, crate::Rect>,
    ) {
        let displays = outputs.frames().collect::<Vec<_>>();
        let screens = outputs.screens();
        let parked_ids: HashSet<u32> = self.parked.keys().map(|window| window.window_id).collect();
        // Almost every window is on screen or parked by Spaceman: both are skipped before any
        // geometry or window-server lookup.
        let mut stale: Vec<(u32, crate::Rect, crate::Rect)> = server_frames
            .iter()
            .filter(|(id, frame)| {
                !parked_ids.contains(id) && !displays.iter().any(|d| d.contains_center(**frame))
            })
            .filter_map(|(&id, &frame)| {
                outputs.displays().iter().find_map(|display| {
                    let parked = crate::geometry::parked_frame_clear_of(
                        display.frame,
                        &displays,
                        &screens,
                        frame,
                    );
                    crate::geometry::parked_at(frame, parked, 1).then_some((
                        id,
                        frame,
                        display.frame,
                    ))
                })
            })
            .collect();
        if stale.is_empty() {
            return;
        }
        // A window Spaceman owns but did not park (an app holding it off screen itself) is never
        // rescued; leave it out before asking the window server, so it costs nothing per cycle.
        let owned: HashSet<u32> = WorkspaceId::all()
            .flat_map(|workspace| self.workspaces.windows(workspace))
            .map(|window| window.window_id)
            .collect();
        stale.retain(|(id, ..)| !owned.contains(id));
        if stale.is_empty() {
            return;
        }
        // Rescue in creation order, so the cascade matches the order recall assigns.
        stale.sort_by_key(|(id, ..)| *id);
        let ids = stale.iter().map(|(id, ..)| *id).collect();
        let owners = self.backend.window_owners(&ids);
        let mut cascade: HashMap<(i32, i32), i32> = HashMap::new();
        for (id, frame, output) in stale {
            let Some(app) = owners.get(&id) else {
                continue;
            };
            let window = crate::PhysicalWindowId {
                app: app.clone(),
                window_id: id,
            };
            // A process that started this cycle has no observation behind it yet.
            if !self.memory.bundles.contains_key(app)
                && let Some(bundle_id) = self.backend.bundle_id_of(app)
            {
                self.memory.bundles.insert(app.clone(), bundle_id);
            }
            let disposition = self.bundle_disposition(&window, frame);
            if self.workspaces.workspace_of_any(&window).is_some()
                || self.workspaces.scratchpad().contains(&window)
                || disposition.is_some_and(crate::config::DispositionPatch::is_ignored)
            {
                continue;
            }
            // A window its rule routes to a hidden workspace would be parked there right after
            // the rescue; rescuing it first flashes it on the user's screen (an agent's app
            // reopening its window where Spaceman last parked it). It joins that workspace
            // where it is.
            if let Some(home) = disposition
                .and_then(|patch| patch.home)
                .filter(|home| !self.is_visible(*home, outputs))
            {
                self.adopt_parked_unseen(window, home, frame, outputs);
                continue;
            }
            let offset = cascade.entry((output.x, output.y)).or_insert(0);
            let target = crate::Rect {
                x: output.x + *offset,
                y: output.y + *offset,
                ..frame
            };
            *offset += RESCUE_CASCADE;
            log_event!(
                "recover stale parked window: {:?} from {:?} to {:?}",
                window,
                frame,
                target
            );
            let outcome = self.backend.set_position(&window, target);
            if !matches!(outcome, macos::ApplyOutcome::Applied) {
                log_event!(
                    "stale parked recovery incomplete for {:?}: {:?}",
                    window,
                    outcome
                );
            }
        }
    }

    /// Owns `window` on hidden `workspace` as a parked tile: recovery frame on that workspace's
    /// display, no move at all.
    fn adopt_parked_unseen(
        &mut self,
        window: crate::PhysicalWindowId,
        workspace: WorkspaceId,
        frame: crate::Rect,
        outputs: &macos::DisplayOutputs,
    ) {
        let output = super::runtime::output_for(&self.workspaces, workspace, outputs);
        let recovery = crate::Rect {
            x: output.x,
            y: output.y,
            ..frame
        };
        if !self.record_recovery(&window, recovery) {
            return;
        }
        self.parked.park_applied(&window, self.backend.now());
        let canvas = self.layout_settings(output).canvas();
        self.workspaces.adopt_tile(
            window.clone(),
            workspace,
            self.config.layout(),
            canvas,
            None,
        );
        log_event!("route parked {window:?} -> workspace {workspace} without showing it");
        self.note_placement(&window, crate::journal::PlacementSource::Rule, workspace);
    }
}
