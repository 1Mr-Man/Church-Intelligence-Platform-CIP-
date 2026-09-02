import { useEffect, useState } from "react";
import "./LoginScreen.css";
import type { OperatorAccountSummary } from "../domain";
import { createOperatorAccount, listOperatorAccounts, login } from "../lib/commands";

interface LoginScreenProps {
  onLoggedIn: (operator: OperatorAccountSummary) => void;
}

/**
 * The Phase 10 top-level gate: mirrors `WebRuntimeNotice`'s pattern of
 * replacing the entire app with a single-purpose screen, wired into
 * `App.tsx` exactly the same way. Two states, decided by whether any
 * operator account exists yet:
 *
 * - Zero accounts (first launch, or a fresh database): a "create the
 *   first Admin account" form - no login required, since there is no
 *   other way for the very first account to ever come into existence.
 * - One or more accounts: an ordinary login form (pick an account, enter
 *   its PIN).
 *
 * See `docs/phase-10-audit.md` for the full design record, including
 * this screen's explicitly limited security role (a workflow control,
 * not the actual access-control boundary - that's the backend's
 * `ensure_admin` calls).
 */
export function LoginScreen({ onLoggedIn }: LoginScreenProps) {
  const [accounts, setAccounts] = useState<OperatorAccountSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Login form state
  const [selectedAccountId, setSelectedAccountId] = useState("");
  const [pin, setPin] = useState("");
  const [submitting, setSubmitting] = useState(false);

  // First-account creation form state
  const [newName, setNewName] = useState("");
  const [newPin, setNewPin] = useState("");

  useEffect(() => {
    let cancelled = false;
    listOperatorAccounts()
      .then((list) => {
        if (cancelled) return;
        setAccounts(list);
        if (list.length > 0) setSelectedAccountId(list[0].id);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const operator = await login(selectedAccountId, pin);
      onLoggedIn(operator);
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const handleCreateFirstAdmin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const created = await createOperatorAccount(newName, newPin, "admin");
      const operator = await login(created.id, newPin);
      onLoggedIn(operator);
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  if (accounts === null) {
    return (
      <div className="login-screen">
        <h1>Church Intelligence Platform</h1>
        <p>Loading&hellip;</p>
      </div>
    );
  }

  if (accounts.length === 0) {
    return (
      <div className="login-screen">
        <h1>Church Intelligence Platform</h1>
        <p className="login-screen__badge">First-time setup</p>
        <p>Create the first operator account. It will be an Admin account.</p>
        <form onSubmit={handleCreateFirstAdmin} className="login-screen__form">
          <label>
            Name
            <input
              type="text"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              required
              autoFocus
            />
          </label>
          <label>
            PIN (at least 4 characters)
            <input
              type="password"
              value={newPin}
              onChange={(e) => setNewPin(e.target.value)}
              minLength={4}
              required
            />
          </label>
          {error && (
            <p className="login-screen__error" role="alert">
              {error}
            </p>
          )}
          <button type="submit" disabled={submitting}>
            Create Admin Account
          </button>
        </form>
      </div>
    );
  }

  return (
    <div className="login-screen">
      <h1>Church Intelligence Platform</h1>
      <p className="login-screen__badge">Sign in</p>
      <form onSubmit={handleLogin} className="login-screen__form">
        <label>
          Operator
          <select
            value={selectedAccountId}
            onChange={(e) => setSelectedAccountId(e.target.value)}
          >
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.displayName} ({account.role})
              </option>
            ))}
          </select>
        </label>
        <label>
          PIN
          <input
            type="password"
            value={pin}
            onChange={(e) => setPin(e.target.value)}
            required
            autoFocus
          />
        </label>
        {error && (
          <p className="login-screen__error" role="alert">
            {error}
          </p>
        )}
        <button type="submit" disabled={submitting}>
          Log In
        </button>
      </form>
    </div>
  );
}
