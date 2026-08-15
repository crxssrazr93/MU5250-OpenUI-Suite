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

/// Enable a profile.
///
/// `refresh` asks the card to issue a REFRESH proactive command so the modem
/// re-reads the card immediately instead of at the next reboot.
///
/// It defaults to **off**, which is the opposite of what you would want, because
/// this device rejects it: `ES10c EnableProfile` with the refresh flag set fails
/// with `es10c_enable_profile`, while the same call without it succeeds. That
/// matches the rest of this firmware — it exposes no usable STK/CAT path, so the
/// card has no way to deliver a REFRESH to the modem.
///
/// The consequence is visible to the user and must be surfaced, not hidden: the
/// profile really is switched on the card, but the modem keeps using the old one
/// until the router reboots. Callers should tell the user that.
pub fn enable_profile(
    iccid: &str,
    refresh: bool,
    use_relay: bool,
) -> Result<lpac::LpacResult, String> {
    let iccid = validate_iccid(iccid)?;
    with_notification(use_relay, || {
        with_radio_parked(|| {
            let mut args = vec!["profile", "enable", iccid.as_str()];
            if refresh {
                args.push("1");
            }
            lpac::run(&args)
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
        with_radio_parked(|| {
            let mut args = vec!["profile", "disable", iccid.as_str()];
            if refresh {
                args.push("1");
            }
            lpac::run(&args)
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
