// Shared domain types for the agent API.

export interface CarrierComponent {
  label: string // "PCC", "SCC0", "SCC1", etc.
  band: string // "B8", "n78"
  pci: number
  earfcn: number
  bandwidth: string // "10 MHz"
  freq?: number // MHz, calculated from EARFCN
  rsrp?: number
  rsrq?: number
  sinr?: number
  rssi?: number
  ul_configured?: boolean
  active?: boolean
}

export interface SignalInfo {
  type?: string
  carrier?: string
  signal_bars?: number
  cell_id?: string
  lte_carriers: CarrierComponent[]
  nr_carriers: CarrierComponent[]
  net_select?: string
  lte_band_lock?: number[]
  nr_band_lock?: number[]
  raw_lte_band_lock?: string
  raw_nr_band_lock?: string
  rsrp?: number
  band?: string
}

export interface BatteryInfo {
  percent: number
  charging: boolean
  voltage_mv?: number
  temperature_c?: number
  current_ma?: number
}

/** Live WAN throughput, in **bytes** per second (`formatSpeed` converts to bits). */
export interface SpeedInfo {
  rx_bps: number
  tx_bps: number
  max_rx_bps: number
  max_tx_bps: number
}

export interface DeviceInfo {
  model: string
  firmware?: string
  uptime_secs?: number
  load_avg?: number[]
}

export interface WanInfo {
  connected: boolean
  ipv4?: string
  ipv6?: string
  gateway?: string
  dns?: string[]
  apn?: string
}

export interface Wan6Info {
  connected: boolean
  ipv6?: string
  prefix?: string
  dns?: string[]
}

export interface Client {
  mac: string
  ip?: string
  hostname?: string
  medium?: 'wifi' | 'usb-c' | 'ethernet' | 'wired'
  medium_detail?: 'wifi_2ghz' | 'wifi_5ghz' | 'usb_c' | 'ethernet'
  interface?: string
  wifi_band?: string
  signal_dbm?: number
  tx_bitrate_mbps?: number
  rx_bitrate_mbps?: number
  expected_throughput_mbps?: number
  connected_secs?: number
  wired_link_mbps?: number
}

export interface CpuInfo {
  overall: number
  cores: number[]
}

export type UsbMode = 'ecm' | 'rndis' | 'ncm'

export interface UsbModeCapability {
  mode: UsbMode
  supported: boolean
  experimental: boolean
  function?: string
  note?: string
}

export interface UsbLink {
  negotiated?: string
  negotiated_label?: string
  negotiated_mbps?: number
  max?: string
  max_label?: string
  max_mbps?: number
  at_full_speed?: boolean
}

export interface UsbStatus {
  active_mode: UsbMode | null
  default_mode?: UsbMode
  link?: UsbLink
  ncm_persist_on_boot?: boolean
  supported_modes: string[]
  experimental_modes?: string[]
  mode_capabilities?: UsbModeCapability[]
  composition_functions?: string[]
  configfs?: { present?: boolean; ncm?: boolean; gsi_ecm?: boolean; gsi_rndis?: boolean }
  bridge?: { name?: string; members?: string[] }
  interfaces?: { ecm0?: boolean; rndis0?: boolean; ncm0?: boolean; ncm_ifname?: string | null }
  usb_ids?: { vendor?: string | null; product?: string | null }
  ncm_last_error?: string
  connect?: number
  typec_cc?: string
}

export interface MemInfo {
  total_kb: number
  used_kb: number
  free_kb: number
  usage_pct: number
}

export interface WifiBand {
  ssid?: string
  enabled: boolean
  channel?: number
  bandwidth?: string
  configuredChannel?: string
  configuredBandwidth?: string
  actualChannel?: number
  actualBandwidth?: string
  password?: string
  security?: string
  hidden: boolean
  clients?: number
}

export interface WifiAll {
  band_2g: WifiBand
  band_5g: WifiBand
  guest_ssid?: string
  master_supported: boolean
  master_enabled: boolean
  wifi6_supported: boolean
  wifi6_enabled?: boolean
}

export interface DnsConfig {
  primary: string
  secondary: string
  ipv6_primary?: string
  ipv6_secondary?: string
}

export interface LanConfig {
  ip: string
  netmask: string
  dhcp_start: string
  dhcp_end: string
  dhcp_lease: string
}

export interface ThermalInfo {
  cpu_temp_c?: number
}

export interface ThermalAll {
  cpu_0?: number
  cpu_1?: number
  cpu_2?: number
  cpu_3?: number
  modem?: number
  modem_ss0?: number
  modem_ss1?: number
  modem_ss2?: number
  battery?: number
  usb?: number
  eth_phy?: number
  pmic?: number
  xo_therm?: number
  pa?: number
  sdr?: number
}

export interface BatteryBspInfo {
  online: boolean
  low_power: boolean
  using_hw_fg_chip: boolean
  time_to_full_mins?: number
  time_to_empty_mins?: number
}

export interface BatteryDetail {
  capacity: number
  status: string
  voltage_mv: number
  voltage_max_mv: number
  voltage_ocv_mv: number
  current_ma: number
  power_mw: number
  temperature_c: number
  charge_type: string
  health: string
  cycle_count: number
  charge_counter_mah: number
  charge_full_mah: number
  charge_full_design_mah: number
  time_to_full_secs: number
  time_to_empty_secs: number
}

export interface ChargeControlState {
  charging_stopped: boolean
  battery_status: string
  capacity: number
  charge_limit_enabled: boolean
  charge_limit: number
  hysteresis: number
  manual_override: boolean
}

export interface ApnProfile {
  profilename: string
  wanapn: string
  username: string
  password: string
  pdpType: number
  pppAuthMode: number
  profileId: string
  isEnable: boolean
}

export interface SimInfo {
  iccid?: string
  imsi?: string
  state?: string
  mcc?: string
  mnc?: string
}

export interface UsagePeriod {
  rx_bytes: number
  tx_bytes: number
  time_secs: number
}

export interface DataUsage {
  day: UsagePeriod
  month: UsagePeriod
  cycle?: UsagePeriod
  since_power_on?: UsagePeriod
  total: UsagePeriod
  reset_day?: number
  reset_enabled?: boolean
  clear_date_record?: string
  next_clear_date?: string
}

export interface SmsMessage {
  id: number
  /** Sender (inbox) or recipient (sent) number. */
  number: string
  content: string
  date?: string
  /** 0=unread, 1=read, 2=sent, 3=draft */
  tag: number
  /** 0=SIM, 1=NV device storage */
  mem_store?: number
}

/** One row of `GET /api/system/top` — mirrors agent/src/system.rs::ProcessEntry. */
export interface ProcessInfo {
  pid: number
  name: string
  cpu_pct: number
  rss_kb: number
  state: string
  /** True for daemons on the agent's kill-safe bloat allowlist. */
  is_bloat: boolean
}

/** `GET /api/system/top` — mirrors agent/src/system.rs::ProcessListResult. */
export interface ProcessListResult {
  processes: ProcessInfo[]
  total_count: number
  bloat_count: number
  bloat_cpu_pct: number
  bloat_rss_kb: number
}

export interface KilledProcess {
  pid: number
  name: string
}

/** `POST /api/system/kill-bloat` — mirrors agent/src/system.rs::KillBloatResult. */
export interface KillBloatResult {
  killed: KilledProcess[]
  skipped: KilledProcess[]
  freed_rss_kb: number
}

export interface LoggerStatus {
  running: boolean
  samples?: number
  events?: number
  elapsed_secs: number
  duration_secs: number
  interval_secs: number
}

export interface LoggerDownload {
  csv: string
}

export interface TtlStatus {
  active?: boolean
  ipv6_active?: boolean
  ttl_value?: number
}

export interface AtSendResult {
  command?: string
  response: string
  port?: string
  elapsed_ms?: number
}

/** One merged poll of /api/dashboard — the home screen's single request. */
export interface HomeData {
  signal: SignalInfo | null
  battery: BatteryInfo | null
  speed: SpeedInfo | null
  device: DeviceInfo | null
  wan: WanInfo | null
  wan6: Wan6Info | null
  cpu: CpuInfo | null
  memory: MemInfo | null
  usage: DataUsage | null
  thermal: ThermalInfo | null
}

// ── eUICC / eSIM ────────────────────────────────────────────────────────────

/**
 * Whether the hardware has a usable eUICC. `card_present` says a card is in the
 * slot; `euicc_available` is the stronger claim that the GSMA ISD-R selected,
 * which is the only proof the card is really an eSIM.
 */
export interface EuiccStatus {
  card_present: boolean
  euicc_available: boolean
  detail: string
}

/** EID, masked unless the caller explicitly asked for the full value. */
export interface EuiccEid {
  eid: string
  masked: boolean
}

export interface EuiccProfile {
  iccid: string | null
  isdp_aid: string | null
  /** 'enabled' | 'disabled' | 'unknown' */
  state: string
  enabled: boolean
  /** 'operational' | 'provisioning' | 'test' | 'unknown' */
  class: string
  nickname: string | null
  service_provider: string | null
  name: string | null
}

export interface EuiccProfiles {
  profiles: EuiccProfile[]
  count: number
  masked: boolean
}

/**
 * What this agent build serves. Clients are built against a superset of any one
 * build, so features absent here should be hidden rather than called and shown
 * to the user as an error.
 */
export interface Capabilities {
  agent_version: string
  api_version: number
  supported: Record<string, boolean>
  unsupported: Record<string, string>
}

/** A pending RSP notification the eUICC wants delivered to the operator. */
export interface EuiccNotification {
  seqNumber: number
  iccid: string
  notificationAddress: string
  profileManagementOperation: string
}

/** Result envelope shared by every lpac-backed write operation. */
export interface EuiccOperation {
  result: unknown
  progress: string[]
  /**
   * Set when the card was switched but the modem still reads the old profile.
   * This modem rejects ES10c EnableProfile's refresh flag, so a switch only
   * takes effect on reboot — a client that does not say so shows a successful
   * operation with no visible change, which reads as a failure.
   */
  reboot_required?: boolean
  notice?: string
}

/** Whether a relay-backed operation is running, and whether it wants a client. */
export interface EuiccRelayStatus {
  active: boolean
  waiting_for_client: boolean
  /** Whether a relay client is polling right now. */
  client_connected: boolean
}

/** `lpac chip info`, as far as the dashboard reads it. */
export interface EuiccChipInfo {
  eidValue?: string
  EUICCInfo2?: {
    profileVersion?: string
    svn?: string
    euiccFirmwareVer?: string
    /** lpac decodes SGP.22's extCardResource into byte counts. */
    extCardResource?: {
      installedApplication?: number
      freeNonVolatileMemory?: number
      freeVolatileMemory?: number
    }
    ppVersion?: string
    sasAcreditationNumber?: string
  }
}

/** Fields accepted by the download endpoint (EasyLPAC-style). */
export interface EuiccDownloadRequest {
  activation_code?: string
  smdp?: string
  matching_id?: string
  confirmation_code?: string
  imei?: string
  /** Have a LAN client carry the ES9+ traffic, for a router with no WAN. */
  relay?: boolean
}

/** `GET /api/tunnel/wireguard`. */
export interface WireguardState {
  /** False when the `wg` userspace tool is not installed on the router. */
  available: boolean
  /** Whether a private key has been generated or entered. */
  configured: boolean
  /** Whether WireGuard is the tunnel type the router would bring up. */
  is_active_tunnel: boolean
  connect_status: string
  settings: Record<string, string>
  masked: boolean
}

/** One network from a manual operator scan. */
export interface ScannedOperator {
  status: 'available' | 'current' | 'forbidden' | 'unknown'
  name: string
  plmn: string
  mcc: string
  mnc: string
  rat: string
  /** What `operatorSelect` wants back for this network. */
  select: string
}

/** `GET /api/operator/scan`. */
export interface OperatorScan {
  state: string
  scanning: boolean
  /** The sweep ran and failed, as opposed to finding nothing. */
  failed: boolean
  operators: ScannedOperator[]
}

/** A saved WireGuard profile. The private key is always masked. */
export interface WireguardProfile {
  id: number
  name: string
  /** Whether this profile's peer key is the one currently in the vendor config. */
  active: boolean
  settings: Record<string, string>
}
