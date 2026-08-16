//! Read-only eUICC (eSIM) discovery over QMI UIM.
//!
//! Portability note: this module and `crate::qmi` depend only on `std` and
//! `libc`. The HTTP surface lives in `crate::euicc::api` so the transport and
//! ES10 layers can be lifted into another agent unchanged.
//!
//! Safety posture for this phase — every operation here is read-only:
//!
//! * only ES10c `GetEID` and `GetProfilesInfo` are ever sent;
//! * the logical channel is closed on every success, error and panic path;
//! * all card access is serialized behind one process-wide mutex, because the
//!   card has a small fixed number of logical channels and concurrent probes
//!   would exhaust them;
//! * EID and ICCID are masked before they leave this module.

pub mod api;
pub mod bertlv;
pub mod es10;
pub mod lpac;
pub mod relay;

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::qmi::uim::{Card, UimClient};
use crate::ubus;

/// The ubus object carrying the modem's radio operating mode.
const RADIO_OBJECT: &str = "zte_nwinfo_api";

/// Serializes all card access. A poisoned lock is recovered rather than
/// propagated: the guarded resource is the card itself, and the channel-close
/// guard below has already run by the time a panic unwinds past it.
static CARD_LOCK: Mutex<()> = Mutex::new(());

fn lock_card() -> MutexGuard<'static, ()> {
    CARD_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Chained-response status: the card has `n` more bytes to hand over.
const SW_MORE_DATA: u8 = 0x61;
/// Wrong length: retry with the length the card asks for.
const SW_WRONG_LENGTH: u8 = 0x6C;
const SW_OK: u16 = 0x9000;

/// Bounds the GET RESPONSE chain so a misbehaving card cannot spin a worker.
const MAX_CHAINED_READS: usize = 32;

#[derive(Debug, Clone)]
pub struct EuiccStatus {
    pub card_present: bool,
    pub isdr_available: bool,
    pub detail: String,
}

/// Holds a logical channel open and closes it when dropped.
///
/// This is the only thing standing between an early `?` and a leaked channel,
/// so the close is unconditional and its failure is logged rather than
/// swallowed silently.
struct ChannelGuard<'a> {
    uim: &'a mut UimClient,
    channel: Option<u8>,
}

impl<'a> ChannelGuard<'a> {
    fn open(uim: &'a mut UimClient, aid: &[u8]) -> Result<Self, String> {
        let channel = uim.open_logical_channel(aid)?;
        Ok(Self {
            uim,
            channel: Some(channel),
        })
    }

    fn channel(&self) -> u8 {
        self.channel.unwrap_or(0)
    }

    /// Send an ES10 command and collect the full chained response.
    fn transmit(&mut self, command: &[u8]) -> Result<Vec<u8>, String> {
        let channel = self.channel();
        let mut response = self.uim.send_apdu(channel, command)?;
        let mut collected = Vec::new();

        for _ in 0..MAX_CHAINED_READS {
            let (data, sw) = es10::split_status(&response)?;
            collected.extend_from_slice(data);

            let [sw1, sw2] = sw.to_be_bytes();
            match sw1 {
                SW_MORE_DATA => {
                    response = self.uim.send_apdu(channel, &es10::get_response(sw2))?;
                }
                SW_WRONG_LENGTH => {
                    // The card wants a different Le; reissue the same command.
                    let mut retry = command.to_vec();
                    if let Some(last) = retry.last_mut() {
                        *last = sw2;
                    }
                    collected.clear();
                    response = self.uim.send_apdu(channel, &retry)?;
                }
                _ if sw == SW_OK => return Ok(collected),
                _ => return Err(format!("card returned status 0x{sw:04X}")),
            }
        }
        Err("card kept requesting continuation reads".into())
    }
}

impl Drop for ChannelGuard<'_> {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            if let Err(e) = self.uim.close_logical_channel(channel) {
                eprintln!("[euicc] failed to close logical channel {channel}: {e}");
            }
        }
    }
}

/// Open the UIM service and read card status.
fn card_status() -> Result<Vec<Card>, String> {
    let mut uim = UimClient::connect()?;
    uim.card_status()
}

/// Probe for an eUICC: is a card present, and does it expose the ISD-R?
pub fn status() -> Result<EuiccStatus, String> {
    let _lock = lock_card();

    let cards = card_status()?;
    let card_present = cards.iter().any(|c| c.is_present());
    if !card_present {
        return Ok(EuiccStatus {
            card_present: false,
            isdr_available: false,
            detail: "no card present in slot 1".into(),
        });
    }

    // Selecting the ISD-R is the actual test: the vendor firmware exposes eSIM
    // strings on every SKU, so only a successful SELECT proves a real eUICC.
    let mut uim = UimClient::connect()?;
    // Bind the result so the guard is dropped — and the channel closed — before
    // `uim` goes out of scope at the end of the function.
    let selected = ChannelGuard::open(&mut uim, &es10::ISD_R_AID).map(|_| ());
    match selected {
        Ok(()) => Ok(EuiccStatus {
            card_present: true,
            isdr_available: true,
            detail: "ISD-R selected successfully".into(),
        }),
        Err(e) => Ok(EuiccStatus {
            card_present: true,
            isdr_available: false,
            detail: format!("card present but ISD-R not available: {e}"),
        }),
    }
}

/// Read the EID. Returns the full 32-digit value; callers mask it.
pub fn eid() -> Result<String, String> {
    let _lock = lock_card();
    let mut uim = UimClient::connect()?;
    let mut guard = ChannelGuard::open(&mut uim, &es10::ISD_R_AID)?;
    let response = guard.transmit(&es10::get_eid_request()?)?;
    es10::parse_eid(&response)
}

/// Enumerate installed profiles.
pub fn profiles() -> Result<Vec<es10::ProfileInfo>, String> {
    let _lock = lock_card();
    let mut uim = UimClient::connect()?;
    let mut guard = ChannelGuard::open(&mut uim, &es10::ISD_R_AID)?;
    let response = guard.transmit(&es10::get_profiles_request()?)?;
    es10::parse_profiles(&response)
}

/// Mask an identifier, keeping only enough to tell two apart.
///
/// EIDs and ICCIDs are personal identifiers tied to a subscriber, so the API
/// returns them masked unless the caller explicitly asks for the full value.
pub fn mask(value: &str) -> String {
    let len = value.chars().count();
    if len <= 8 {
        return "*".repeat(len);
    }
    let head: String = value.chars().take(4).collect();
    let tail: String = value.chars().skip(len - 4).collect();
    format!("{head}{}{tail}", "*".repeat(len - 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_identifiers() {
        assert_eq!(
            mask("89044012345678901234567890123456"),
            "8904************************3456"
        );
        assert_eq!(mask("89490102186110201029"), "8949************1029");
    }

    #[test]
    fn masks_short_values_entirely() {
        // Too short to reveal a prefix and suffix without exposing all of it.
        assert_eq!(mask("12345678"), "********");
        assert_eq!(mask(""), "");
        assert_eq!(mask("123"), "***");
    }

    #[test]
    fn mask_preserves_length() {
        for value in ["1234567890", "89044012345678901234567890123456"] {
            assert_eq!(mask(value).len(), value.len());
        }
    }
}

// --- Write operations (via lpac) ---

/// A profile download request, in either of the two forms a user can supply.
///
/// EasyLPAC-style: either paste the whole activation code, or fill the parts in
/// by hand. `lpac` accepts both, so this only has to validate and forward.
#[derive(Debug, Default)]
pub struct DownloadRequest {
    pub activation_code: Option<String>,
    pub smdp: Option<String>,
    pub matching_id: Option<String>,
    pub confirmation_code: Option<String>,
    pub imei: Option<String>,
}

impl DownloadRequest {
    /// Build the lpac argument list, rejecting anything that could smuggle an
    /// extra argument in. Values go to `lpac` as distinct argv entries — never
    /// through a shell — but a leading `-` would still be read as a flag.
    pub fn to_args(&self) -> Result<Vec<String>, String> {
        let mut args = vec!["profile".to_string(), "download".to_string()];

        let mut push = |flag: &str, value: &str, what: &str| -> Result<(), String> {
            let value = value.trim();
            if value.is_empty() {
                return Err(format!("{what} is empty"));
            }
            if value.starts_with('-') {
                return Err(format!("{what} may not start with '-'"));
            }
            if value.len() > 512 {
                return Err(format!("{what} is too long"));
            }
            if value.chars().any(|c| c.is_control()) {
                return Err(format!("{what} contains control characters"));
            }
            args.push(flag.to_string());
            args.push(value.to_string());
            Ok(())
        };

        match (&self.activation_code, &self.smdp) {
            (Some(code), _) => push("-a", code, "activation code")?,
            (None, Some(smdp)) => {
                push("-s", smdp, "SM-DP+ address")?;
                if let Some(id) = &self.matching_id {
                    push("-m", id, "matching ID")?;
                }
            }
            (None, None) => {
                return Err("provide either an activation code or an SM-DP+ address".into())
            }
        }

        if let Some(code) = &self.confirmation_code {
            push("-c", code, "confirmation code")?;
        }
        if let Some(imei) = &self.imei {
            push("-i", imei, "IMEI")?;
        }
        Ok(args)
    }
}

/// Download and install a profile.
///
/// With `use_relay`, the ES9+ traffic is carried by a client on the LAN rather
/// than by the router itself — see [`relay`] for why that is usually necessary
/// on a device being provisioned for the first time.
pub fn download(request: &DownloadRequest, use_relay: bool) -> Result<lpac::LpacResult, String> {
    let args = request.to_args()?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    with_notification(use_relay, || lpac::run(&refs))
}

/// Put the modem's radio down for the duration of `work`, then bring it back.
///
/// The eUICC refuses `ES10c EnableProfile` and `DisableProfile` outright while
/// the modem holds a session on the enabled profile — the bare
/// `es10c_enable_profile` / `es10c_disable_profile` failures with no progress
/// steps and no explanation. SGP.22 expects the terminal to be told about the
/// switch via REFRESH, and this modem has no path for that, so the card simply
/// says no.
///
/// Parking the radio releases the SIM and the switch is accepted. Verified on
/// hardware: the identical enable that failed three times in a row succeeded
/// immediately with the radio in `low_power`.
///
/// The radio is restored on every path, including when the switch fails, so a
/// refused operation never leaves the router with its modem shut down.
fn with_radio_parked<T>(work: impl FnOnce() -> T) -> T {
    let parked = ubus::call(
        RADIO_OBJECT,
        "nwinfo_set_mode",
        Some(r#"{"operate_mode":"low_power"}"#),
    )
    .is_ok();

    // Give the modem a moment to actually let go of the card; the ubus call
    // returns before the detach completes.
    if parked {
        std::thread::sleep(Duration::from_millis(6_000));
    }

    let result = work();

    if parked {
        let _ = ubus::call(
            RADIO_OBJECT,
            "nwinfo_set_mode",
            Some(r#"{"operate_mode":"online"}"#),
        );
    }
    result
}

/// Reset the card over QMI so the modem re-selects the eUICC's applications.
///
/// This is the REFRESH substitute. `EnableProfile` switches the profile on the
/// card, but SGP.22 leaves the modem on the old one until a REFRESH proactive
/// command — which this firmware has no CAT path to deliver. A UIM
/// `POWER_OFF_SIM` / `POWER_ON_SIM` is a card reset the modem drives itself, so
/// the eUICC presents the newly enabled profile and the modem attaches to it
/// with no reboot. Verified on hardware: `AT+CIMI` follows the switch after this
/// runs, where it stayed on the old IMSI without it.
///
/// Run it with the radio parked. An online modem holds a session on the card and
/// the power-off races it; parked, the card is free and the reset is clean.
fn power_cycle_card() -> Result<(), String> {
    let mut uim = UimClient::connect().map_err(|e| format!("card power-cycle: connect: {e}"))?;
    uim.power_off_card()
        .map_err(|e| format!("card power-cycle: power off: {e}"))?;
    // The card needs a beat down before it is powered back up, the way a slot
    // reset would leave it.
    std::thread::sleep(Duration::from_millis(1_500));
    uim.power_on_card()
        .map_err(|e| format!("card power-cycle: power on: {e}"))?;
    // Card init runs asynchronously after POWER_ON; let it finish before the
    // modem — or the notification step that follows — reads the card again.
    std::thread::sleep(Duration::from_secs(3));
    Ok(())
}

/// Is `iccid` currently the enabled profile?
///
/// Reads the card directly, without taking the lock, so it is safe to call
/// inside an operation that already holds it — the same rule as
/// [`queued_sequences`].
fn profile_enabled(iccid: &str) -> bool {
    let Ok(listed) = lpac::run(&["profile", "list"]) else {
        return false;
    };
    listed.payload["data"]
        .as_array()
        .map(|items| {
            items.iter().any(|p| {
                p["iccid"].as_str() == Some(iccid)
                    && p["profileState"].as_str() == Some("enabled")
            })
        })
        .unwrap_or(false)
}

/// A synthetic success, for when the card is already in the state asked for.
fn state_reached() -> lpac::LpacResult {
    lpac::LpacResult {
        payload: serde_json::json!({"code": 0, "data": serde_json::Value::Null, "message": "success"}),
        progress: Vec::new(),
        applied_live: false,
    }
}

/// Run an enable/disable, with the card's actual state — not lpac's return code
/// — as the arbiter of success.
///
/// This eUICC intermittently fails `EnableProfile` / `DisableProfile` with a
/// transient error (`data: "unknown"`, no progress) while the modem still holds
/// the card, and once a retry has applied the switch it fails the *next* attempt
/// with `profileNotInDisabledState` / `profileNotInEnabledState` — the change
/// asked for has already happened. Neither is a real failure. So the operation
/// is idempotent: if the card already reads the way the caller wants, before or
/// after any attempt, that is success. A genuine refusal (bad ICCID, a policy
/// rule) never reaches the wanted state and still fails after the retries.
fn switch_until(
    args: &[&str],
    reached: impl Fn() -> bool,
) -> Result<lpac::LpacResult, String> {
    const ATTEMPTS: usize = 4;

    if reached() {
        return Ok(state_reached());
    }

    let mut result = lpac::run(args)?;
    let mut tries = 1;
    while !reached() && result.payload["code"].as_i64().unwrap_or(0) != 0 && tries < ATTEMPTS {
        std::thread::sleep(Duration::from_millis(2_500));
        result = lpac::run(args)?;
        tries += 1;
    }

    // The card is what matters. If it reached the wanted state, report success
    // even when the last attempt returned the "already done" error.
    if reached() && result.payload["code"].as_i64().unwrap_or(-1) != 0 {
        result.payload =
            serde_json::json!({"code": 0, "data": serde_json::Value::Null, "message": "success"});
    }
    Ok(result)
}

/// Make a profile switch that just succeeded on the card take effect on the
/// modem, and record which way it went.
///
/// A failed power-cycle is not fatal: the profile is switched on the card, so
/// the caller falls back to telling the user to reboot, exactly as before this
/// path existed.
fn apply_switch(result: &mut lpac::LpacResult) {
    match power_cycle_card() {
        Ok(()) => {
            result.applied_live = true;
            result.progress.push("switch_applied_via_card_power_cycle".into());
        }
        Err(e) => result.progress.push(format!("switch_pending_reboot: {e}")),
    }
}

/// Switch a profile and carry the switch to the modem.
///
/// Runs the enable/disable with the radio parked, retrying on the card's actual
/// state (see [`switch_until`]), then power-cycles the card so the modem picks up
/// the new profile without a reboot ([`apply_switch`]).
///
/// A note on `catBusy`, which the retries here paper over but cannot cure: after
/// an enable the eUICC queues a REFRESH proactive command, and this modem has no
/// CAT path to fetch it, so it can sit pending and hold the toolkit. While it
/// does, the next switch is refused with `catBusy`. Nothing reachable from
/// software reliably clears a pending proactive command on this firmware — a NAS
/// detach (`AT+COPS=2`), stopping the data manager, `low_power`, and a card
/// power-cycle were all tried and none is dependable; only a reboot is. So the
/// retries catch the common case where the command clears on its own within a
/// few seconds, and a switch that stays refused returns a clear reboot notice
/// rather than a silent failure. See [`switch_response`](api::switch_response).
fn robust_switch(
    args: &[&str],
    reached: impl Fn() -> bool,
) -> Result<lpac::LpacResult, String> {
    let mut result = with_radio_parked(|| switch_until(args, &reached))?;
    if reached() && result.payload["code"].as_i64().unwrap_or(0) == 0 {
        apply_switch(&mut result);
    }
    Ok(result)
}

/// Sequence numbers the card currently has queued.
///
/// Assumes the card lock is already held — this only runs inside an operation,
/// and `lock_card` is a plain mutex that would deadlock on a second acquire.
/// A card that cannot be listed reads as "nothing queued", which makes the
/// caller send nothing rather than send the wrong thing.
fn queued_sequences() -> Vec<u64> {
    let Ok(listed) = lpac::run(&["notification", "list"]) else {
        return Vec::new();
    };
    listed.payload["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["seqNumber"].as_u64())
                .collect()
        })
        .unwrap_or_default()
}

/// Which queued sequence numbers were not there before the operation ran.
///
/// Kept separate from the card so the rule that decides what gets sent to an
/// operator is testable without one.
fn fresh_sequences(before: &[u64], after: &[u64]) -> Vec<String> {
    after
        .iter()
        .filter(|seq| !before.contains(seq))
        .map(u64::to_string)
        .collect()
}

/// Report to the operator whatever this operation just queued.
///
/// Only notifications that appeared during the operation are sent. Sending
/// everything pending would add a dead SM-DP+ host's connect timeout to every
/// subsequent operation, which is how a single stuck notification turns into a
/// card that feels broken.
fn report_new_notifications(before: &[u64], result: &mut lpac::LpacResult) {
    let fresh = fresh_sequences(before, &queued_sequences());
    if fresh.is_empty() {
        return;
    }

    let mut args = vec!["notification", "process", "-r"];
    args.extend(fresh.iter().map(String::as_str));

    // Best effort by design: the profile operation already succeeded, and
    // failing it now would tell the user a lie about the card. An undelivered
    // notification stays queued and is listed for a retry instead.
    let delivered = matches!(
        lpac::run(&args),
        Ok(ref sent) if sent.payload["code"].as_i64().unwrap_or(0) == 0
    );
    result.progress.push(
        if delivered { "notification_delivered" } else { "notification_deferred" }.into(),
    );
}

/// Keep the card's notification store from filling up on a WAN-less router.
///
/// Every enable and disable queues a notification for the operator's SM-DP+, and
/// a router with no WAN can never deliver them, so they accumulate for as long
/// as the user keeps switching profiles. A full store makes the card refuse the
/// next switch. Once the backlog passes a high-water mark, the oldest
/// enable/disable notifications are dropped to make room.
///
/// Install and delete notifications are never dropped: those the operator
/// genuinely needs. A dropped delete strands the activation code at the SM-DP+,
/// so a profile can never be reinstalled — see [`delete_profile`]. Enable and
/// disable notifications are only informational, and undeliverable here anyway.
///
/// Best effort: this runs after the switch already succeeded, so a failure to
/// prune must not turn a good operation into a reported error. The lock is
/// already held by [`with_notification`].
fn prune_notification_backlog() {
    const KEEP: usize = 32;

    let Ok(listed) = lpac::run(&["notification", "list"]) else {
        return;
    };
    let Some(items) = listed.payload["data"].as_array() else {
        return;
    };
    if items.len() <= KEEP {
        return;
    }

    let mut prunable: Vec<u64> = items
        .iter()
        .filter(|n| {
            matches!(
                n["profileManagementOperation"].as_str(),
                Some("enable" | "disable")
            )
        })
        .filter_map(|n| n["seqNumber"].as_u64())
        .collect();
    prunable.sort_unstable(); // oldest sequence numbers first

    let overflow = items.len().saturating_sub(KEEP);
    let drop: Vec<String> = prunable
        .into_iter()
        .take(overflow)
        .map(|seq| seq.to_string())
        .collect();
    if drop.is_empty() {
        return;
    }

    let mut args = vec!["notification", "remove"];
    args.extend(drop.iter().map(String::as_str));
    let _ = lpac::run(&args);
    eprintln!(
        "[euicc] pruned {} old enable/disable notifications so the card's store cannot fill",
        drop.len()
    );
}

/// Run a profile operation and report it to the operator before returning.
///
/// SGP.22 has the card queue a notification for every profile change, and the
/// operator's records stay wrong until it is delivered. Doing that here rather
/// than leaving it to the client is not tidiness — a client cannot do it:
///
///   * Enabling returns `reboot_required` on this modem, so the dashboard that
///     would have sent the notification is about to be rebooted out from under
///     itself. In practice it never got sent.
///   * A download holds a relay guard so ES9+ traffic can reach the internet
///     over a LAN client. The notification needs that same path, and by the
///     time a client could ask for it separately the relay has been torn down.
///   * Deleting is the one that does lasting damage. The delete notification is
///     what releases the profile at the SM-DP+; without it the activation code
///     is spent and nobody — no app, no other device — can reinstall it.
///
/// The card lock is held across both steps, so nothing can queue a notification
/// in between and have it mistaken for this operation's.
fn with_notification(
    use_relay: bool,
    run_operation: impl FnOnce() -> Result<lpac::LpacResult, String>,
) -> Result<lpac::LpacResult, String> {
    let _lock = lock_card();
    let _relay = use_relay.then(relay::activate);

    let before = queued_sequences();
    let mut result = run_operation()?;
    if result.payload["code"].as_i64().unwrap_or(0) == 0 {
        report_new_notifications(&before, &mut result);
        prune_notification_backlog();
    }
    Ok(result)
}

/// Send pending notifications with a client carrying the traffic.
pub fn process_notifications_via_relay(sequences: &[u64]) -> Result<lpac::LpacResult, String> {
    let numbers = validate_sequence_numbers(sequences)?;
    let mut args = vec!["notification", "process", "-r"];
    args.extend(numbers.iter().map(String::as_str));
    let _lock = lock_card();
    let _relay = relay::activate();
    lpac::run(&args)
}

/// An ICCID as accepted from a client.
///
/// Digits only: it reaches `lpac` as an argv entry, and this keeps anything
/// resembling a flag or a path out of it.
pub fn validate_iccid(iccid: &str) -> Result<String, String> {
    let trimmed = iccid.trim();
    if trimmed.len() < 18 || trimmed.len() > 22 {
        return Err("ICCID must be 18-22 digits".into());
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err("ICCID must contain only digits".into());
    }
    Ok(trimmed.to_string())
}

/// Enable a profile, and make the switch take effect on the modem.
///
/// The card accepts `ES10c EnableProfile`, but SGP.22 leaves the modem on the
/// old profile until a REFRESH proactive command, which this firmware has no CAT
/// path to deliver. So `refresh` (the card-issued REFRESH) is rejected here —
/// `ES10c EnableProfile` with the flag set fails while the same call without it
/// succeeds. Instead, on the default path the modem is refreshed with a QMI card
/// power-cycle after the switch (see [`apply_switch`]), which needs no CAT path
/// and takes effect with no reboot. `LpacResult::applied_live` records whether
/// that succeeded, so the caller can fall back to a reboot notice if it did not.
///
/// `refresh: true` is kept for hardware that does support it, and marks the
/// switch live on its own when the card accepts it.
pub fn enable_profile(
    iccid: &str,
    refresh: bool,
    use_relay: bool,
) -> Result<lpac::LpacResult, String> {
    let iccid = validate_iccid(iccid)?;
    with_notification(use_relay, || {
        if refresh {
            // Legacy REFRESH path, kept for hardware that supports it. This modem
            // rejects it; the default path below is what actually works here.
            let mut result =
                with_radio_parked(|| lpac::run(&["profile", "enable", iccid.as_str(), "1"]))?;
            if result.payload["code"].as_i64().unwrap_or(0) == 0 {
                result.applied_live = true;
            }
            return Ok(result);
        }
        let target = iccid.clone();
        robust_switch(&["profile", "enable", target.as_str()], || {
            profile_enabled(&target)
        })
    })
}

/// Disable a profile.
///
/// Disabling the last enabled profile leaves the router with no usable SIM, so
/// it is refused unless the caller explicitly forces it. The eUICC stays
/// reachable either way — this guards against losing connectivity by accident,
/// not against an unrecoverable state.
pub fn disable_profile(
    iccid: &str,
    force: bool,
    refresh: bool,
    use_relay: bool,
) -> Result<lpac::LpacResult, String> {
    let iccid = validate_iccid(iccid)?;

    if !force {
        let enabled: Vec<_> = profiles()?
            .into_iter()
            .filter(|p| p.is_enabled())
            .collect();
        let is_last = enabled.len() <= 1
            && enabled
                .first()
                .and_then(|p| p.iccid.clone())
                .is_some_and(|active| active == iccid);
        if is_last {
            return Err(
                "refusing to disable the only enabled profile — this would leave the router \
                 with no SIM. Pass force to override."
                    .into(),
            );
        }
    }

    // Taken after the last-enabled check above, which reads the card itself.
    with_notification(use_relay, || {
        if refresh {
            let mut result =
                with_radio_parked(|| lpac::run(&["profile", "disable", iccid.as_str(), "1"]))?;
            if result.payload["code"].as_i64().unwrap_or(0) == 0 {
                result.applied_live = true;
            }
            return Ok(result);
        }
        let target = iccid.clone();
        robust_switch(&["profile", "disable", target.as_str()], || {
            !profile_enabled(&target)
        })
    })
}

/// Delete a profile. Irreversible: reinstalling needs a fresh activation code
/// from the carrier, and most codes are single-use.
pub fn delete_profile(iccid: &str, use_relay: bool) -> Result<lpac::LpacResult, String> {
    let iccid = validate_iccid(iccid)?;

    // The card rejects deleting an enabled profile, but failing here gives a
    // comprehensible message instead of an ES10 status code.
    if profiles()?
        .iter()
        .any(|p| p.iccid.as_deref() == Some(iccid.as_str()) && p.is_enabled())
    {
        return Err("disable the profile before deleting it".into());
    }

    with_notification(use_relay, || lpac::run(&["profile", "delete", iccid.as_str()]))
}

/// Set a profile's nickname.
pub fn set_nickname(iccid: &str, nickname: &str) -> Result<lpac::LpacResult, String> {
    let iccid = validate_iccid(iccid)?;
    let nickname = nickname.trim();
    if nickname.len() > 64 {
        return Err("nickname must be 64 characters or fewer".into());
    }
    if nickname.starts_with('-') {
        return Err("nickname may not start with '-'".into());
    }
    if nickname.chars().any(|c| c.is_control()) {
        return Err("nickname contains control characters".into());
    }
    let _lock = lock_card();
    lpac::run(&["profile", "nickname", &iccid, nickname])
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn builds_download_args_from_activation_code() {
        let request = DownloadRequest {
            activation_code: Some("LPA:1$rsp.example.com$MATCH-123".into()),
            ..Default::default()
        };
        assert_eq!(
            request.to_args().unwrap(),
            vec!["profile", "download", "-a", "LPA:1$rsp.example.com$MATCH-123"]
        );
    }

    #[test]
    fn builds_download_args_from_parts() {
        let request = DownloadRequest {
            smdp: Some("rsp.example.com".into()),
            matching_id: Some("MATCH-123".into()),
            confirmation_code: Some("1234".into()),
            imei: Some("350000000000000".into()),
            ..Default::default()
        };
        assert_eq!(
            request.to_args().unwrap(),
            vec![
                "profile", "download", "-s", "rsp.example.com", "-m", "MATCH-123", "-c", "1234",
                "-i", "350000000000000"
            ]
        );
    }

    #[test]
    fn download_requires_a_source() {
        assert!(DownloadRequest::default().to_args().is_err());
    }

    #[test]
    fn download_rejects_values_that_look_like_flags() {
        // Values reach lpac as argv entries, so a leading '-' would be read as
        // an option rather than data.
        let request = DownloadRequest {
            activation_code: Some("-p".into()),
            ..Default::default()
        };
        assert!(request.to_args().is_err());

        let request = DownloadRequest {
            smdp: Some("rsp.example.com".into()),
            matching_id: Some("--help".into()),
            ..Default::default()
        };
        assert!(request.to_args().is_err());
    }

    #[test]
    fn download_rejects_control_characters_and_empties() {
        for bad in ["", "  ", "code\nwith-newline", "code\0nul"] {
            let request = DownloadRequest {
                activation_code: Some(bad.into()),
                ..Default::default()
            };
            assert!(request.to_args().is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn validates_iccids() {
        assert_eq!(
            validate_iccid(" 89490102186110201029 ").unwrap(),
            "89490102186110201029"
        );
        assert!(validate_iccid("8949010218611020102").is_ok());
        // Too short, too long, non-digits, and flag-like values.
        assert!(validate_iccid("894901").is_err());
        assert!(validate_iccid(&"8".repeat(23)).is_err());
        assert!(validate_iccid("89490102186110201O29").is_err());
        assert!(validate_iccid("--force").is_err());
        assert!(validate_iccid("").is_err());
    }
}

/// eUICC chip information: EID, firmware, free space, RSP capabilities.
pub fn chip_info() -> Result<lpac::LpacResult, String> {
    let _lock = lock_card();
    lpac::run(&["chip", "info"])
}

/// Pending RSP notifications.
///
/// After installing, enabling, disabling or deleting a profile the eUICC queues
/// a notification for the operator's SM-DP+. Leaving them unsent is a common
/// cause of a carrier believing a profile was never installed, so they are
/// surfaced rather than hidden.
pub fn notifications() -> Result<lpac::LpacResult, String> {
    let _lock = lock_card();
    lpac::run(&["notification", "list"])
}

/// Sequence numbers as accepted from a client.
fn validate_sequence_numbers(sequences: &[u64]) -> Result<Vec<String>, String> {
    if sequences.is_empty() {
        return Err("no sequence numbers given".into());
    }
    if sequences.len() > 64 {
        return Err("too many sequence numbers".into());
    }
    Ok(sequences.iter().map(|s| s.to_string()).collect())
}

/// Send pending notifications to the operator, then remove them from the card.
pub fn process_notifications(sequences: &[u64]) -> Result<lpac::LpacResult, String> {
    let numbers = validate_sequence_numbers(sequences)?;
    let mut args = vec!["notification", "process", "-r"];
    args.extend(numbers.iter().map(String::as_str));
    let _lock = lock_card();
    lpac::run(&args)
}

/// Drop notifications without sending them.
pub fn remove_notifications(sequences: &[u64]) -> Result<lpac::LpacResult, String> {
    let numbers = validate_sequence_numbers(sequences)?;
    let mut args = vec!["notification", "remove"];
    args.extend(numbers.iter().map(String::as_str));
    let _lock = lock_card();
    lpac::run(&args)
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn sends_only_what_the_operation_queued() {
        // The stuck one (7) is already there and must not be retried, or its
        // dead host's timeout is charged to every later operation.
        assert_eq!(fresh_sequences(&[7], &[7, 8]), vec!["8".to_string()]);
    }

    #[test]
    fn sends_nothing_when_the_operation_queued_nothing() {
        assert!(fresh_sequences(&[7, 8], &[7, 8]).is_empty());
        assert!(fresh_sequences(&[], &[]).is_empty());
    }

    #[test]
    fn sends_every_new_sequence_when_an_operation_queues_several() {
        assert_eq!(
            fresh_sequences(&[1], &[1, 2, 3]),
            vec!["2".to_string(), "3".to_string()]
        );
    }

    #[test]
    fn an_unreadable_baseline_falls_back_to_sending_everything() {
        // `queued_sequences` reads as empty when the card cannot be listed, so
        // a failed pre-read makes every queued notification look new and all of
        // them get sent. That is the right way round to be wrong: sending a
        // notification twice costs a round trip, while not sending a delete
        // notification strands the profile at the SM-DP+ permanently.
        assert_eq!(
            fresh_sequences(&[], &[7, 8]),
            vec!["7".to_string(), "8".to_string()]
        );
    }

    #[test]
    fn validates_sequence_numbers() {
        assert_eq!(validate_sequence_numbers(&[0, 3]).unwrap(), vec!["0", "3"]);
        assert!(validate_sequence_numbers(&[]).is_err());
        assert!(validate_sequence_numbers(&vec![1u64; 65]).is_err());
    }
}
