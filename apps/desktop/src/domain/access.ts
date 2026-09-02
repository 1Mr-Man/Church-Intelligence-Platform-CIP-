/**
 * Operator account / role domain contracts (Phase 10: Church/User Roles
 * & Permissions). Mirrors `cip_core_access`/`commands::OperatorAccountSummaryDto`
 * (Rust). See `docs/roles-permissions.md`.
 */

export type Role = "admin" | "operator";

/**
 * Never carries `pinHash`/`pinSalt` - the backend's DTO deliberately
 * omits them (see `commands::OperatorAccountSummaryDto`'s own docs).
 */
export interface OperatorAccountSummary {
  id: string;
  displayName: string;
  role: Role;
  createdAt: string; // ISO-8601
}
