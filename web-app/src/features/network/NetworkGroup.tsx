import { useHashSegment } from '../../app/hashRoute'
import { Tabs } from '../../ui/Tabs'
import ClientsTab from './ClientsTab'
import WifiTab from './WifiTab'
import RouterTab from './RouterTab'
import WireguardTab from './WireguardTab'

type Tab = 'clients' | 'wifi' | 'router' | 'wireguard'

const NETWORK_TABS = ['clients', 'wifi', 'router', 'wireguard'] as const satisfies readonly Tab[]

export default function NetworkGroup() {
  const [tab, setTab] = useHashSegment<Tab>(1, NETWORK_TABS, 'clients')

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-bold text-ink">Network</h1>
        <p className="mt-0.5 text-[13px] text-ink2">Connected clients, Wi-Fi, router settings and the WireGuard tunnel</p>
      </div>

      <Tabs
        tabs={[
          { id: 'clients', label: 'Clients' },
          { id: 'wifi', label: 'Wi-Fi' },
          { id: 'router', label: 'Router' },
          { id: 'wireguard', label: 'WireGuard' },
        ]}
        active={tab}
        onChange={setTab}
      />

      {tab === 'wireguard' && <WireguardTab />}
      {tab === 'clients' && <ClientsTab />}
      {tab === 'wifi' && <WifiTab />}
      {tab === 'router' && <RouterTab />}
    </div>
  )
}
