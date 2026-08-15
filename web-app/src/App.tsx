import { lazy, useEffect, useState } from 'react'
import { normalizeHash, useHashSegment } from './app/hashRoute'
import { AUTH_EXPIRED_EVENT, clearToken, hasToken } from './data/client'
import { agentHost, isDesktop } from './data/host'
import ConnectRouter from './app/ConnectRouter'
import { HomeProvider } from './app/HomeContext'
import Login from './app/Login'
import Shell, { GROUPS, type Group } from './app/Shell'
import { useTheme } from './app/theme'
import { ConfirmHost, Toaster } from './ui/feedback'

const HomePage = lazy(() => import('./features/home/HomePage'))
const SignalGroup = lazy(() => import('./features/signal/SignalGroup'))
const NetworkGroup = lazy(() => import('./features/network/NetworkGroup'))
const ModemGroup = lazy(() => import('./features/modem/ModemGroup'))
const SystemGroup = lazy(() => import('./features/system/SystemGroup'))

export default function App() {
  const [authed, setAuthed] = useState(hasToken())
  const [connected, setConnected] = useState(!isDesktop || agentHost() !== null)
  const [group, setGroup] = useHashSegment<Group>(0, GROUPS, 'home')
  const { theme, toggle } = useTheme()

  useEffect(() => {
    normalizeHash('home')
  }, [])

  useEffect(() => {
    const onAuthExpired = () => setAuthed(false)
    window.addEventListener(AUTH_EXPIRED_EVENT, onAuthExpired)
    return () => window.removeEventListener(AUTH_EXPIRED_EVENT, onAuthExpired)
  }, [])

  // Desktop starts a step earlier: there is no address to inherit from the page.
  if (!connected) {
    return (
      <>
        <ConnectRouter onSet={() => setConnected(true)} />
        <Toaster />
      </>
    )
  }

  if (!authed) {
    return (
      <>
        <Login onAuthed={() => setAuthed(true)} />
        <Toaster />
      </>
    )
  }

  return (
    <>
      <HomeProvider fast={group === 'home' || group === 'signal'}>
        <Shell group={group} onNavigate={setGroup} theme={theme} onToggleTheme={toggle}>
          {group === 'home' && <HomePage />}
          {group === 'signal' && <SignalGroup />}
          {group === 'network' && <NetworkGroup />}
          {group === 'modem' && <ModemGroup />}
          {group === 'system' && (
            <SystemGroup
              onLogout={() => {
                clearToken()
                setAuthed(false)
              }}
            />
          )}
        </Shell>
      </HomeProvider>
      <Toaster />
      <ConfirmHost />
    </>
  )
}
