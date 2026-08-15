import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { initDesktopTransport } from './data/host'
import './index.css'

// Awaited before the first render because it decides both where requests go and
// what sends them. Rendering first would let the app issue a request through
// the webview's own fetch, which the agent's CORS check refuses. In a browser
// this resolves immediately without loading anything.
initDesktopTransport().finally(() => {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
})
