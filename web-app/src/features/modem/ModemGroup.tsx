import { useCapability } from '../../data/capabilities'
import { useHashSegment } from '../../app/hashRoute'
import { Tabs } from '../../ui/Tabs'
import ApnTab from './ApnTab'
import DataTab from './DataTab'
import EsimTab from './EsimTab'
import TtlTab from './TtlTab'
import SmsTab from './SmsTab'

type Tab = 'apn' | 'data' | 'ttl' | 'sms' | 'esim'

const MODEM_TABS = ['apn', 'data', 'ttl', 'sms', 'esim'] as const satisfies readonly Tab[]

export default function ModemGroup() {
  const [tab, setTab] = useHashSegment<Tab>(1, MODEM_TABS, 'apn')
  // Hidden on agents that do not serve the eUICC routes, rather than shown and
  // failing with a 404 when opened.
  const hasEsim = useCapability('euicc_read')

  // Derived rather than corrected in an effect: capability discovery resolves
  // one render after mount, and falling back is a rendering decision, not a
  // state change to write back.
  const active: Tab = !hasEsim && tab === 'esim' ? 'apn' : tab

  const tabs: { id: Tab; label: string }[] = [
    { id: 'apn', label: 'APN' },
    { id: 'data', label: 'Data' },
    { id: 'ttl', label: 'TTL' },
    { id: 'sms', label: 'SMS' },
    ...(hasEsim ? [{ id: 'esim' as const, label: 'eSIM' }] : []),
  ]

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-bold text-ink">Modem</h1>
        <p className="mt-0.5 text-[13px] text-ink2">
          APN profiles, data usage, TTL{hasEsim ? ', SMS and eSIM' : ' and SMS'}
        </p>
      </div>

      <Tabs tabs={tabs} active={active} onChange={setTab} />

      {active === 'apn' && <ApnTab />}
      {active === 'data' && <DataTab />}
      {active === 'ttl' && <TtlTab />}
      {active === 'sms' && <SmsTab />}
      {active === 'esim' && <EsimTab />}
    </div>
  )
}
