import { useHashSegment } from '../../app/hashRoute'
import { Tabs } from '../../ui/Tabs'
import MetricsTab from './MetricsTab'
import ToolsTab from './ToolsTab'
import SettingsTab from './SettingsTab'

type Tab = 'metrics' | 'tools' | 'settings'

const SYSTEM_TABS = ['metrics', 'tools', 'settings'] as const satisfies readonly Tab[]

export default function SystemGroup({ onLogout }: { onLogout: () => void }) {
  const [tab, setTab] = useHashSegment<Tab>(1, SYSTEM_TABS, 'metrics')

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-bold text-ink">System</h1>
        <p className="mt-0.5 text-[13px] text-ink2">Health metrics, diagnostic tools and device controls</p>
      </div>

      <Tabs
        tabs={[
          { id: 'metrics', label: 'Metrics' },
          { id: 'tools', label: 'Tools' },
          { id: 'settings', label: 'Settings' },
        ]}
        active={tab}
        onChange={setTab}
      />

      {tab === 'metrics' && <MetricsTab />}
      {tab === 'tools' && <ToolsTab />}
      {tab === 'settings' && <SettingsTab onLogout={onLogout} />}
    </div>
  )
}
