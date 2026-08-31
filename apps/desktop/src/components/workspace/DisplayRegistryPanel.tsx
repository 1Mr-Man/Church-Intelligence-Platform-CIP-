/**
 * Phase 3.10.2: the operator configuration UI for assigning presentation
 * roles to physical monitors - see `docs/phase-3-10-2-display-registry.md`.
 * Lists whatever `list_displays` currently reports (real monitor geometry
 * merged with any persisted role assignment) and lets the operator change
 * a monitor's role, which persists immediately. This panel does not open
 * or move any presentation window itself - it only edits the assignment
 * that `display_presentation`/`open_presentation_display` read the next
 * time they place a window (see `resolve_screen_placement` in
 * `commands.rs`).
 */
import { useCallback, useEffect, useState } from "react";
import { assignDisplayRole, listDisplays } from "../../lib/commands";
import type { Display, DisplayRole } from "../../domain";

const ROLE_LABELS: Record<DisplayRole, string> = {
  unassigned: "Unassigned",
  operator: "Operator",
  projector: "Projector",
  stage: "Stage",
  confidence: "Confidence Monitor",
  lobby: "Lobby",
};

const ROLE_OPTIONS: DisplayRole[] = ["unassigned", "operator", "projector", "stage", "confidence", "lobby"];

function displayLabel(display: Display): string {
  const name = display.name ?? `Unnamed display (${display.width}x${display.height})`;
  const parts = [name];
  if (display.isPrimary) parts.push("primary");
  if (!display.connected) parts.push("disconnected");
  return `${parts[0]}${parts.length > 1 ? ` — ${parts.slice(1).join(", ")}` : ""}`;
}

export function DisplayRegistryPanel() {
  const [displays, setDisplays] = useState<Display[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [assigning, setAssigning] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setLoading(true);
    setError(null);
    listDisplays()
      .then(setDisplays)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const handleRoleChange = useCallback(
    (monitorId: string, role: DisplayRole) => {
      setAssigning(monitorId);
      setError(null);
      assignDisplayRole(monitorId, role)
        .then(refresh)
        .catch((e) => setError(String(e)))
        .finally(() => setAssigning(null));
    },
    [refresh],
  );

  return (
    <details className="live-brain__panel workspace-diagnostics">
      <summary>
        <h2 style={{ display: "inline" }}>Display Setup</h2>
      </summary>
      {error && (
        <p className="live-brain__error" role="alert">
          {error}
        </p>
      )}
      {displays.length === 0 && !loading && !error && <p>No monitors detected.</p>}
      {displays.length > 0 && (
        <table className="workspace-header__grid">
          <thead>
            <tr>
              <th>Monitor</th>
              <th>Resolution</th>
              <th>Position</th>
              <th>Role</th>
            </tr>
          </thead>
          <tbody>
            {displays.map((display) => (
              <tr key={display.monitorId}>
                <td>{displayLabel(display)}</td>
                <td>
                  {display.width}x{display.height} @ {display.scaleFactor}x
                </td>
                <td>
                  {display.x}, {display.y}
                </td>
                <td>
                  <select
                    value={display.assignedRole}
                    disabled={assigning === display.monitorId}
                    onChange={(e) => handleRoleChange(display.monitorId, e.target.value as DisplayRole)}
                  >
                    {ROLE_OPTIONS.map((role) => (
                      <option key={role} value={role}>
                        {ROLE_LABELS[role]}
                      </option>
                    ))}
                  </select>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <button type="button" onClick={refresh} disabled={loading}>
        {loading ? "Refreshing…" : "Refresh displays"}
      </button>
    </details>
  );
}
