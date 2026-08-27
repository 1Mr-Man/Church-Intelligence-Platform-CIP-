import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import './index.css'
import App from './App.tsx'
import { PresentationDisplay } from './components/PresentationDisplay.tsx'
import { isTauriRuntime } from './lib/runtime'

/**
 * Two windows share this one frontend bundle: the operator's main window
 * (`App`) and the presentation display window (`PresentationDisplay`,
 * created at runtime by `presentation_display.rs` with the Tauri window
 * label `"display"`). They're distinguished purely by reading the current
 * webview's own label - no second Vite entry point, no second build.
 * `getCurrentWebviewWindow()` itself is safe to import unconditionally
 * (it only reads a global, matching `isTauriRuntime()`'s own discipline in
 * `lib/runtime.ts`); calling it is guarded by `isTauriRuntime()` below, so
 * outside the Tauri desktop shell (the web runtime) this always resolves
 * to the ordinary `App`, since there is no such window to be.
 */
function Root() {
  if (isTauriRuntime() && getCurrentWebviewWindow().label === 'display') {
    return <PresentationDisplay />
  }
  return <App />
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
