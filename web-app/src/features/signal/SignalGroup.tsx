import { useHashSegment } from '../../app/hashRoute'
import { Tabs } from '../../ui/Tabs'
import Overview from './Overview'
import Locking from './Locking'
import OperatorTab from './OperatorTab'

type Tab = 'overview' | 'locking' | 'operator'

const SIGNAL_TABS = ['overview', 'locking', 'operator'] as const satisfies readonly Tab[]

export default function SignalGroup() {
  const [tab, setTab] = useHashSegment<Tab>(1, SIGNAL_TABS, 'overview')

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-bold text-ink">Signal</h1>
        <p className="mt-0.5 text-[13px] text-ink2">Live radio metrics, band and cell locking, operator selection</p>
      </div>

      <Tabs
        tabs={[
          { id: 'overview', label: 'Overview' },
          { id: 'locking', label: 'Mode & Locking' },
          { id: 'operator', label: 'Operator' },
        ]}
        active={tab}
        onChange={setTab}
      />

      {tab === 'overview' && <Overview />}
      {tab === 'locking' && <Locking />}
      {tab === 'operator' && <OperatorTab />}
    </div>
  )
}
