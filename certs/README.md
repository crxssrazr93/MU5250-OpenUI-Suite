# GSMA RSP trust anchors

`gsma-rsp2-root-ci1.pem` is the **GSM Association - RSP2 Root CI1**. Its key id
is `81370f5125d0b1d408d4c3b232e6d25e795bebfb`. It is valid until 2052-02-21.

## Why this is here

ES9+ TLS to an SM-DP+ is not anchored on the public web PKI. Some operators
present a WebPKI certificate. Others present one issued by a GSMA Certificate
Issuer. Truphone/1GLOBAL is one of them. That CA is deliberately absent from
every OS and browser trust store. A stock `curl` or `urllib` therefore fails
with `unable to get local issuer certificate`, and the download cannot start.

Clients need this root *in addition to* the system bundle. It is not installed
system-wide. It is not a substitute for the system CAs.

## Why you can trust this file

This file was not taken on faith from a download. Two independent checks pin it.

1. **The card attests to it.** `lpac chip info` reports the eUICC's
   `euiccCiPKIdListForVerification` as `81370f5125d0b1d408d4c3b232e6d25e795bebfb`.
   This certificate's Subject Key Identifier is byte-for-byte the same value. So
   the eUICC in the device names this exact CA as one it trusts.
2. **It validates a live server.** The SM-DP+ certificate served by
   `rsp.truphone.com` carries Authority Key Identifier `81:37:0F:…:EB:FB`. Run
   `openssl verify -CAfile gsma-rsp2-root-ci1.pem` against that leaf. It returns
   OK.

A different certificate that satisfied both checks would need the CI private
key.

Re-check at any time:

```sh
openssl x509 -in certs/gsma-rsp2-root-ci1.pem -noout -text \
  | grep -A1 "Subject Key Identifier"
```

Source: <https://euicc-manual.osmocom.org/docs/pki/ci/> (Osmocom euicc-manual),
file `81370f.txt`. That page lists other live CIs. Add one here the same way
when a carrier needs it. Verify each against the card the same two ways.
