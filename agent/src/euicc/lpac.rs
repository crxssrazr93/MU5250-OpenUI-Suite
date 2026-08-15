//! Bridge between `lpac` and this agent.
//!
//! Full RSP (downloading a profile from an SM-DP+) means activation-code
//! parsing, an ES9+ exchange over TLS, ASN.1 DER encode/decode and segmented
//! BPP loading. Hand-rolling that is exactly what the project's safety notes
//! warn against, so the tested implementation — `lpac` — does it, and this
//! module supplies the two things it needs from the host.
//!
//! `lpac` is run with `LPAC_APDU=stdio` and `LPAC_HTTP=stdio`, which makes it
//! speak a line-oriented JSON protocol on its stdin/stdout instead of linking
//! its own transports:
//!
//! ```text
//! lpac -> {"type":"apdu","payload":{"func":"transmit","param":"80E2910003BF2D0000"}}
//! host <- {"type":"apdu","payload":{"ecode":0,"data":"BF2D...9000"}}
//!
//! lpac -> {"type":"http","payload":{"url":"https://...","tx":"7B...","headers":[...]}}
//! host <- {"type":"http","payload":{"rcode":200,"rx":"7B..."}}
//! ```
//!
//! APDUs are served by the QMI/QRTR transport in `crate::qmi`, which is already
//! used for the read-only path. HTTP is delegated to the device's own `curl`,
//! which is present and HTTPS-capable, so the agent needs no TLS stack.
//!
//! Consequently `lpac` itself is built with neither libcurl nor libqmi/glib —
//! only the two stdio drivers — which keeps it to a few hundred KiB.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::relay;
use crate::qmi::uim::UimClient;

/// Where `deploy.sh` installs lpac. Overridable for development.
const DEFAULT_LPAC_DIR: &str = "/data/local/tmp/lpac";

/// Bounds a single lpac invocation. A profile download involves several
/// round trips to the SM-DP+, so this is generous, but it must terminate.
const HTTP_TIMEOUT_SECS: u64 = 60;

/// Refuse to relay absurd bodies rather than shelling out with them.
const MAX_HTTP_BODY: usize = 4 * 1024 * 1024;

pub fn lpac_dir() -> PathBuf {
    std::env::var("ZTE_AGENT_LPAC_DIR")
        .unwrap_or_else(|_| DEFAULT_LPAC_DIR.to_string())
        .into()
}

/// Whether lpac is installed and executable.
pub fn available() -> bool {
    lpac_dir().join("lpac").is_file()
}

/// Outcome of one lpac invocation.
#[derive(Debug)]
pub struct LpacResult {
    /// The `payload` of lpac's final `lpa` message.
    pub payload: Value,
    /// Progress messages, in order. Useful for surfacing download steps.
    pub progress: Vec<String>,
}

/// Run `lpac <args>` with this agent as its APDU and HTTP backend.
pub fn run(args: &[&str]) -> Result<LpacResult, String> {
    let dir = lpac_dir();
    let binary = dir.join("lpac");
    if !binary.is_file() {
        return Err(format!(
            "lpac is not installed at {}",
            binary.display()
        ));
    }

    let mut child = Command::new(&binary)
        .args(args)
        .current_dir(&dir)
        .env("LPAC_APDU", "stdio")
        .env("LPAC_HTTP", "stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start lpac: {e}"))?;

    let mut stdin = child.stdin.take().ok_or("lpac stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("lpac stdout unavailable")?;
    let mut reader = BufReader::new(stdout);

    let mut session = ApduSession::new();
    let mut progress = Vec::new();
    let mut payload = Value::Null;
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                let _ = child.kill();
                return Err(format!("reading lpac output: {e}"));
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            // lpac also writes non-JSON diagnostics; ignore rather than abort.
            continue;
        };

        match message["type"].as_str().unwrap_or_default() {
            "apdu" => {
                let reply = session.handle(&message["payload"]);
                write_reply(&mut stdin, "apdu", reply)?;
            }
            "http" => {
                let reply = handle_http(&message["payload"]);
                write_reply(&mut stdin, "http", reply)?;
            }
            "progress" => {
                if let Some(text) = message["payload"]["message"].as_str() {
                    progress.push(text.to_string());
                }
            }
            "lpa" => payload = message["payload"].clone(),
            _ => {}
        }
    }

    // The Drop on ApduSession closes any channel lpac left open.
    drop(session);

    let status = child
        .wait()
        .map_err(|e| format!("waiting for lpac: {e}"))?;

    let stderr = child
        .stderr
        .take()
        .map(|mut s| {
            let mut buf = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut buf);
            buf
        })
        .unwrap_or_default();

    if !status.success() && payload.is_null() {
        return Err(format!(
            "lpac exited with {}: {}",
            status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(LpacResult { payload, progress })
}

fn write_reply(stdin: &mut impl Write, kind: &str, payload: Value) -> Result<(), String> {
    let message = json!({"type": kind, "payload": payload});
    writeln!(stdin, "{message}").map_err(|e| format!("writing to lpac: {e}"))?;
    stdin.flush().map_err(|e| format!("flushing to lpac: {e}"))
}

// --- APDU backend ---

/// Holds the UIM connection and the logical channel for one lpac run.
struct ApduSession {
    uim: Option<UimClient>,
    channel: Option<u8>,
}

impl ApduSession {
    fn new() -> Self {
        Self {
            uim: None,
            channel: None,
        }
    }

    fn handle(&mut self, payload: &Value) -> Value {
        let func = payload["func"].as_str().unwrap_or_default();
        let param = payload["param"].as_str().unwrap_or_default();

        match func {
            "connect" => match UimClient::connect() {
                Ok(uim) => {
                    self.uim = Some(uim);
                    json!({"ecode": 0})
                }
                Err(e) => {
                    eprintln!("[lpac] UIM connect failed: {e}");
                    json!({"ecode": -1})
                }
            },
            "disconnect" => {
                self.close_channel();
                self.uim = None;
                json!({"ecode": 0})
            }
            "logic_channel_open" => {
                let Ok(aid) = decode_hex(param) else {
                    return json!({"ecode": -1});
                };
                let Some(uim) = self.uim.as_mut() else {
                    return json!({"ecode": -1});
                };
                match uim.open_logical_channel(&aid) {
                    Ok(channel) => {
                        self.channel = Some(channel);
                        // lpac reads the channel id out of `ecode`.
                        json!({"ecode": channel})
                    }
                    Err(e) => {
                        eprintln!("[lpac] open logical channel failed: {e}");
                        json!({"ecode": -1})
                    }
                }
            }
            "logic_channel_close" => {
                self.close_channel();
                json!({"ecode": 0})
            }
            "transmit" => {
                let Ok(command) = decode_hex(param) else {
                    return json!({"ecode": -1});
                };
                let channel = self.channel.unwrap_or(0);
                let Some(uim) = self.uim.as_mut() else {
                    return json!({"ecode": -1});
                };
                match uim.send_apdu(channel, &command) {
                    Ok(response) => json!({"ecode": 0, "data": encode_hex(&response)}),
                    Err(e) => {
                        eprintln!("[lpac] APDU transmit failed: {e}");
                        json!({"ecode": -1})
                    }
                }
            }
            other => {
                eprintln!("[lpac] unknown APDU function: {other}");
                json!({"ecode": -1})
            }
        }
    }

    fn close_channel(&mut self) {
        if let (Some(uim), Some(channel)) = (self.uim.as_mut(), self.channel.take()) {
            if let Err(e) = uim.close_logical_channel(channel) {
                eprintln!("[lpac] failed to close logical channel {channel}: {e}");
            }
        }
    }
}

impl Drop for ApduSession {
    fn drop(&mut self) {
        // lpac crashing mid-session must not leave a channel open: the card has
        // a small fixed number and a leaked one needs a modem reset.
        self.close_channel();
    }
}

// --- HTTP backend ---

/// Perform one ES9+ request using the device's `curl`.
///
/// The URL comes from the activation code and from SM-DP+ redirects, so it is
/// not fully trusted: only HTTPS is allowed, and it is passed to curl as a
/// distinct argument rather than through a shell.
fn handle_http(payload: &Value) -> Value {
    let url = payload["url"].as_str().unwrap_or_default();
    if !url.starts_with("https://") {
        eprintln!("[lpac] refusing non-HTTPS URL");
        return json!({"rcode": 0});
    }

    // When a client is carrying our traffic, hand the request over instead of
    // trying to reach the network from a router that has no WAN yet.
    if relay::is_active() {
        let headers = payload["headers"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|h| h.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let body_hex = payload["tx"].as_str().unwrap_or_default().to_string();
        return match relay::submit(url, headers, body_hex) {
            Ok((status, body_hex)) => json!({"rcode": status, "rx": body_hex}),
            Err(e) => {
                eprintln!("[lpac] relay failed: {e}");
                json!({"rcode": 0})
            }
        };
    }

    let body = match decode_hex(payload["tx"].as_str().unwrap_or_default()) {
        Ok(b) if b.len() <= MAX_HTTP_BODY => b,
        Ok(_) => {
            eprintln!("[lpac] refusing oversized request body");
            return json!({"rcode": 0});
        }
        Err(_) => return json!({"rcode": 0}),
    };

    let mut cmd = Command::new("curl");
    cmd.arg("--silent")
        .arg("--show-error")
        .arg("--max-time")
        .arg(HTTP_TIMEOUT_SECS.to_string())
        .arg("--write-out")
        .arg("%{http_code}")
        .arg("--output")
        .arg("-");

    // Downloading a profile needs internet, but on a router whose only WAN is
    // the cellular link that the profile itself provides, there is none yet.
    // A proxy breaks that cycle — during provisioning it can point at a machine
    // on the LAN, or at an adb-reversed port.
    if let Ok(proxy) = std::env::var("ZTE_AGENT_LPAC_PROXY") {
        if !proxy.is_empty() {
            cmd.arg("--proxy").arg(proxy);
        }
    }

    if let Some(headers) = payload["headers"].as_array() {
        for header in headers {
            if let Some(header) = header.as_str() {
                cmd.arg("--header").arg(header);
            }
        }
    }

    if !body.is_empty() {
        cmd.arg("--request").arg("POST").arg("--data-binary").arg("@-");
    }
    cmd.arg(url);

    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[lpac] failed to start curl: {e}");
            return json!({"rcode": 0});
        }
    };

    if !body.is_empty() {
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(&body) {
                eprintln!("[lpac] failed to write request body: {e}");
            }
        }
    } else {
        drop(child.stdin.take());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[lpac] curl failed: {e}");
            return json!({"rcode": 0});
        }
    };

    // `--write-out %{http_code}` appends the status to the body on stdout.
    let (response, code) = split_status_suffix(&output.stdout);
    if code == 0 {
        eprintln!(
            "[lpac] curl produced no status: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    json!({"rcode": code, "rx": encode_hex(response)})
}

/// Split curl's trailing `%{http_code}` off the response body.
fn split_status_suffix(stdout: &[u8]) -> (&[u8], u32) {
    // The status is the trailing run of ASCII digits, always 3 for real
    // responses. Take exactly the last 3 when they are digits.
    if stdout.len() < 3 {
        return (stdout, 0);
    }
    let split = stdout.len() - 3;
    let tail = &stdout[split..];
    if tail.iter().all(|b| b.is_ascii_digit()) {
        let code = std::str::from_utf8(tail)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        (&stdout[..split], code)
    } else {
        (stdout, 0)
    }
}

// --- hex ---

pub fn encode_hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

pub fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("hex string has odd length".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| format!("bad hex: {e}")))
        .collect()
}

/// Path to the installed lpac binary, for diagnostics.
pub fn binary_path() -> PathBuf {
    lpac_dir().join("lpac")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(encode_hex(&[0x80, 0xE2, 0x00]), "80E200");
        assert_eq!(decode_hex("80E200").unwrap(), vec![0x80, 0xE2, 0x00]);
        assert_eq!(decode_hex("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(decode_hex("ABC").is_err());
        assert!(decode_hex("ZZ").is_err());
    }

    #[test]
    fn splits_curl_status_suffix() {
        let (body, code) = split_status_suffix(b"hello200");
        assert_eq!(body, b"hello");
        assert_eq!(code, 200);

        // Empty body, status only.
        let (body, code) = split_status_suffix(b"404");
        assert_eq!(body, b"");
        assert_eq!(code, 404);
    }

    #[test]
    fn handles_missing_status_suffix() {
        // curl died before writing a status.
        let (body, code) = split_status_suffix(b"");
        assert_eq!(body, b"");
        assert_eq!(code, 0);
        let (body, code) = split_status_suffix(b"ab");
        assert_eq!(body, b"ab");
        assert_eq!(code, 0);
    }

    #[test]
    fn keeps_trailing_digits_that_are_part_of_the_body() {
        // A body ending in digits still parses — the last three are the status
        // by construction, because curl always appends exactly three.
        let (body, code) = split_status_suffix(b"{\"v\":1}200");
        assert_eq!(body, b"{\"v\":1}");
        assert_eq!(code, 200);
    }

    #[test]
    fn rejects_non_https_urls() {
        let reply = handle_http(&json!({"url": "http://example.com", "tx": ""}));
        assert_eq!(reply["rcode"], 0);
        let reply = handle_http(&json!({"url": "file:///etc/passwd", "tx": ""}));
        assert_eq!(reply["rcode"], 0);
    }
}
