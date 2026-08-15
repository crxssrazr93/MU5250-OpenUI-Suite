import { useEffect, useState } from 'react'
import { api } from '../../data/api'
import type { DnsConfig, LanConfig } from '../../types'
import { Button, Field, Input } from '../../ui/controls'
import { toast, toastError } from '../../ui/feedback'
import { Card } from '../../ui/primitives'

const DNS_PRESETS: { label: string; v: DnsConfig }[] = [
  {
    label: 'Cloudflare',
    v: { primary: '1.1.1.1', secondary: '1.0.0.1', ipv6_primary: '2606:4700:4700::1111', ipv6_secondary: '2606:4700:4700::1001' },
  },
  {
    label: 'Google',
    v: { primary: '8.8.8.8', secondary: '8.8.4.4', ipv6_primary: '2001:4860:4860::8888', ipv6_secondary: '2001:4860:4860::8844' },
  },
  {
    label: 'Quad9',
    v: { primary: '9.9.9.9', secondary: '149.112.112.112', ipv6_primary: '2620:fe::fe', ipv6_secondary: '2620:fe::9' },
  },
]

function DnsSection() {
  const [dns, setDns] = useState<DnsConfig>({ primary: '', secondary: '' })
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.dnsGet().then(setDns).catch(() => {})
  }, [])

  async function save() {
    setBusy(true)
    try {
      await api.dnsSet({
        dns_mode: 'manual',
        prefer_dns_manual: dns.primary,
        standby_dns_manual: dns.secondary,
        ...(dns.ipv6_primary ? { ipv6_wan_prefer_dns_manual: dns.ipv6_primary } : {}),
        ...(dns.ipv6_secondary ? { ipv6_wan_standby_dns_manual: dns.ipv6_secondary } : {}),
      })
      toast('DNS settings saved')
    } catch (e) {
      toastError(e, 'Failed to save DNS')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title="DNS servers">
      <div className="grid grid-cols-1 gap-2.5 lg:grid-cols-2">
        <Field label="Primary DNS (IPv4)">
          <Input value={dns.primary} onChange={(e) => setDns((d) => ({ ...d, primary: e.target.value }))} placeholder="1.1.1.1" inputMode="numeric" />
        </Field>
        <Field label="Secondary DNS (IPv4)">
          <Input value={dns.secondary} onChange={(e) => setDns((d) => ({ ...d, secondary: e.target.value }))} placeholder="1.0.0.1" inputMode="numeric" />
        </Field>
        <Field label="Primary DNS (IPv6)">
          <Input value={dns.ipv6_primary ?? ''} onChange={(e) => setDns((d) => ({ ...d, ipv6_primary: e.target.value }))} placeholder="2606:4700:4700::1111" />
        </Field>
        <Field label="Secondary DNS (IPv6)">
          <Input value={dns.ipv6_secondary ?? ''} onChange={(e) => setDns((d) => ({ ...d, ipv6_secondary: e.target.value }))} placeholder="2001:4860:4860::8888" />
        </Field>
      </div>
      <div className="mt-3.5 flex flex-wrap items-center gap-2">
        <Button variant="primary" onClick={save} loading={busy}>
          Apply
        </Button>
        <div className="flex gap-1.5">
          {DNS_PRESETS.map((p) => (
            <Button key={p.label} variant="ghost" size="sm" onClick={() => setDns(p.v)}>
              {p.label}
            </Button>
          ))}
        </div>
      </div>
    </Card>
  )
}

function LanSection() {
  const [lan, setLan] = useState<LanConfig>({ ip: '', netmask: '', dhcp_start: '', dhcp_end: '', dhcp_lease: '' })
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.lanGet().then(setLan).catch(() => {})
  }, [])

  async function save() {
    setBusy(true)
    try {
      await api.lanSet({
        lan_ipaddr: lan.ip,
        lan_netmask: lan.netmask,
        dhcp_start: lan.dhcp_start,
        dhcp_end: lan.dhcp_end,
        dhcp_lease_time: lan.dhcp_lease,
      })
      toast('LAN settings saved')
    } catch (e) {
      toastError(e, 'Failed to save LAN settings')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title="LAN / DHCP">
      <div className="grid grid-cols-1 gap-2.5 lg:grid-cols-2">
        <Field label="LAN IP">
          <Input value={lan.ip} onChange={(e) => setLan((l) => ({ ...l, ip: e.target.value }))} inputMode="numeric" />
        </Field>
        <Field label="Netmask">
          <Input value={lan.netmask} onChange={(e) => setLan((l) => ({ ...l, netmask: e.target.value }))} inputMode="numeric" />
        </Field>
        <Field label="DHCP start">
          <Input value={lan.dhcp_start} onChange={(e) => setLan((l) => ({ ...l, dhcp_start: e.target.value }))} inputMode="numeric" />
        </Field>
        <Field label="DHCP end">
          <Input value={lan.dhcp_end} onChange={(e) => setLan((l) => ({ ...l, dhcp_end: e.target.value }))} inputMode="numeric" />
        </Field>
        <Field label="Lease time (seconds)">
          <Input value={lan.dhcp_lease} onChange={(e) => setLan((l) => ({ ...l, dhcp_lease: e.target.value }))} inputMode="numeric" />
        </Field>
      </div>
      <div className="mt-3.5">
        <Button variant="primary" onClick={save} loading={busy}>
          Apply
        </Button>
      </div>
    </Card>
  )
}

export default function RouterTab() {
  return (
    <div className="space-y-3">
      <LanSection />
      <DnsSection />
    </div>
  )
}
