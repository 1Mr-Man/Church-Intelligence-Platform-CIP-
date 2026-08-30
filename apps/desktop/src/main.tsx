import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import './index.css'
import App from './App.tsx'
import { PresentationDisplay } from './components/PresentationDisplay.tsx'
import { logCheckpoint } from './components/presentationDiagnostics'
import { isTauriRuntime } from './lib/runtime'
import type { PresentationScreen } from './domain'

/**
 * Four windows share this one frontend bundle: the operator's main window
 * (`App`) and the three presentation display screens (`PresentationDisplay`,
 * created at runtime by `presentation_display.rs` - Phase 3.10 generalizes
 * the original single display window into three fixed roles: `"display"`
 * (Stage, unchanged from before this phase), `"display-confidence"`
 * (Confidence Monitor), `"display-lobby"` (Lobby/Overflow). They're
 * distinguished purely by reading the current webview's own label - no
 * second Vite entry point, no second build. `getCurrentWebviewWindow()`
 * itself is safe to import unconditionally (it only reads a global,
 * matching `isTauriRuntime()`'s own discipline in `lib/runtime.ts`); this
 * module-scope check runs once, before React ever renders anything, so
 * outside the Tauri desktop shell (the web runtime) this always resolves
 * to the ordinary `App`, since there is no such window to be.
 */
const DISPLAY_WINDOW_ROLES: Record<string, PresentationScreen> = {
  display: 'stage',
  'display-confidence': 'confidence',
  'display-lobby': 'lobby',
}

const currentWindowLabel = isTauriRuntime() ? getCurrentWebviewWindow().label : null
const displayRole: PresentationScreen | null =
  currentWindowLabel !== null ? (DISPLAY_WINDOW_ROLES[currentWindowLabel] ?? null) : null
const isDisplayWindow = displayRole !== null

/**
 * Phase 3.8.4 TEMPORARY DIAGNOSTIC: real Windows testing showed the
 * display window appearing but staying completely white - upstream of
 * anything `PresentationDisplay.tsx`'s own effect-based checkpoints can
 * observe, since those require this branch to have already been reached
 * and rendered. Logging the branch selection itself (display window
 * only, to avoid noise on every ordinary main-window launch) and
 * catching any otherwise-invisible frontend exception in that window
 * closes that gap - see `docs/phase-3-8-4-audit.md` boundaries F-H. Runs
 * once at module scope (never during a component's render) so it never
 * mutates global state as a render side effect.
 */
if (isDisplayWindow) {
  logCheckpoint('root-branch-selected', 'display label detected, rendering PresentationDisplay')
  window.onerror = (message, source, lineno, colno, error) => {
    logCheckpoint('frontend-exception', `${String(message)} at ${source ?? '?'}:${lineno ?? '?'}:${colno ?? '?'} ${error?.stack ?? ''}`)
  }
  window.addEventListener('unhandledrejection', (event) => {
    logCheckpoint('frontend-exception', `unhandled promise rejection: ${String(event.reason)}`)
  })
}

function Root() {
  return displayRole !== null ? <PresentationDisplay role={displayRole} /> : <App />
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
