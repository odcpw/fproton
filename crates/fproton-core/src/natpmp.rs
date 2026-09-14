//! One-shot NAT-PMP (RFC 6886) port forwarding through the tunnel (DESIGN.md §4 `port`).
//!
//! std UDP only, to 10.2.0.1:5351 inside the tunnel, 60 s lifetime. `status` never
//! sends a packet; only `fproton port` does.
//!
//! Wire format (RFC 6886 §3.3), the whole of what fproton speaks:
//!
//! ```text
//! request  12 bytes  version=0  op=1|2  reserved u16  internal u16  external u16  lifetime u32
//! response 16 bytes  version=0  op=128+op  result u16  epoch u32  internal u16  external u16  lifetime u32
//! ```

use std::net::UdpSocket;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::error::Error;
use crate::proc::CancelToken;

/// The NAT-PMP gateway inside a Proton tunnel (§1.3).
pub const GATEWAY: &str = "10.2.0.1";
pub const PORT: u16 = 5351;
/// Proton hands out 60 s leases; an external script must renew them.
pub const LIFETIME_SEC: u32 = 60;

/// Opcode 1 is UDP, 2 is TCP (RFC 6886 §3.3). Proton grants the same port for both.
const OP_MAP_UDP: u8 = 1;
/// RFC 6886 §3.1: first retry after 250 ms, doubling, and fproton stops at 4 tries
/// (≈ 3.75 s) rather than the RFC's 9, because `port` is a one-shot query.
const FIRST_RETRY: Duration = Duration::from_millis(250);
const MAX_TRIES: u32 = 4;

/// A granted mapping, as `fproton port --json` prints it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortMapping {
    pub port: u16,
    pub internal_port: u16,
    /// RFC 3339 with local offset.
    pub expires_at: String,
    pub lifetime_sec: u32,
    pub gateway: String,
    /// `udp` and `tcp` are requested separately; Proton grants the same port for both.
    pub protocol: String,
}

/// The 12-byte mapping request (RFC 6886 §3.3).
pub fn encode_request(op: u8, internal_port: u16, external_port: u16, lifetime: u32) -> [u8; 12] {
    let mut buf = [0u8; 12];
    buf[0] = 0; // version
    buf[1] = op;
    // buf[2..4] reserved, zero
    buf[4..6].copy_from_slice(&internal_port.to_be_bytes());
    buf[6..8].copy_from_slice(&external_port.to_be_bytes());
    buf[8..12].copy_from_slice(&lifetime.to_be_bytes());
    buf
}

/// What the gateway answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub op: u8,
    pub result: u16,
    pub epoch: u32,
    pub internal_port: u16,
    pub external_port: u16,
    pub lifetime: u32,
}

/// RFC 6886 §3.5 result codes, for an error a human can act on.
pub fn result_meaning(code: u16) -> &'static str {
    match code {
        0 => "success",
        1 => "unsupported NAT-PMP version",
        2 => "not authorised (port forwarding is off for this account or server)",
        3 => "network failure",
        4 => "out of resources",
        5 => "unsupported opcode",
        _ => "unknown result code",
    }
}

/// Decode a 16-byte response, checking the version and that it answers `op`.
pub fn decode_response(bytes: &[u8], op: u8) -> Result<Response, Error> {
    if bytes.len() < 16 {
        return Err(Error::upstream(format!(
            "the NAT-PMP gateway answered {} bytes, not 16",
            bytes.len()
        ))
        .with_hint("fproton status --json"));
    }
    if bytes[0] != 0 {
        return Err(Error::upstream(format!(
            "the NAT-PMP gateway answered version {}, not 0",
            bytes[0]
        ))
        .with_hint("fproton status --json"));
    }
    if bytes[1] != 128 + op {
        return Err(Error::upstream(format!(
            "the NAT-PMP gateway answered opcode {} for a request of {op}",
            bytes[1]
        ))
        .with_hint("fproton status --json"));
    }
    let u16_at = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Ok(Response {
        op: bytes[1] - 128,
        result: u16_at(2),
        epoch: u32_at(4),
        internal_port: u16_at(8),
        external_port: u16_at(10),
        lifetime: u32_at(12),
    })
}

// ---------------------------------------------------------------- frozen API

/// Ask the gateway for a mapping. Exit 6 when there is no tunnel.
pub fn request_port(ctx: &Context, cancel: &CancelToken) -> Result<PortMapping, Error> {
    // No tunnel, no gateway: this is exit 6, not a 4 s wait for a reply that cannot come.
    if !ctx.paths.sysfs_device(crate::TUNNEL_DEVICE).exists() {
        return Err(Error::not_found(format!(
            "there is no {} tunnel, so nothing can forward a port",
            crate::TUNNEL_DEVICE
        ))
        .with_hint("fproton connect p2p"));
    }

    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| Error::failure(format!("cannot open a UDP socket: {e}")))?;
    socket
        .connect((GATEWAY, PORT))
        .map_err(|e| Error::backend(format!("cannot reach {GATEWAY}:{PORT}: {e}")))?;

    let request = encode_request(OP_MAP_UDP, 0, 0, LIFETIME_SEC);
    let mut wait = FIRST_RETRY;
    let mut last: Option<String> = None;
    for _ in 0..MAX_TRIES {
        if cancel.is_cancelled() {
            return Err(Error::Interrupted);
        }
        socket
            .set_read_timeout(Some(wait))
            .map_err(|e| Error::failure(format!("cannot set a socket deadline: {e}")))?;
        if let Err(e) = socket.send(&request) {
            last = Some(e.to_string());
        } else {
            let mut buf = [0u8; 32];
            match socket.recv(&mut buf) {
                Ok(n) => {
                    let response = decode_response(&buf[..n], OP_MAP_UDP)?;
                    if response.result != 0 {
                        return Err(Error::upstream(format!(
                            "the NAT-PMP gateway refused: {} (code {})",
                            result_meaning(response.result),
                            response.result
                        ))
                        .with_hint("fproton config set port-forwarding on"));
                    }
                    let lifetime = if response.lifetime == 0 {
                        LIFETIME_SEC
                    } else {
                        response.lifetime
                    };
                    let mapping = PortMapping {
                        port: response.external_port,
                        internal_port: response.internal_port,
                        expires_at: crate::rfc3339_from_unix(
                            crate::state::unix_now() as i64 + lifetime as i64,
                        )
                        .unwrap_or_else(crate::now_rfc3339),
                        lifetime_sec: lifetime,
                        gateway: GATEWAY.to_string(),
                        protocol: "udp".to_string(),
                    };
                    let stored = mapping.clone();
                    crate::state::update(ctx, |s| {
                        s.port_forwarding = crate::status::PortForwarding {
                            port: Some(stored.port),
                            expires_at: Some(stored.expires_at.clone()),
                        };
                    })?;
                    return Ok(mapping);
                }
                Err(e) => last = Some(e.to_string()),
            }
        }
        wait *= 2;
    }
    Err(Error::upstream(format!(
        "the NAT-PMP gateway {GATEWAY}:{PORT} did not answer in {} tries ({})",
        MAX_TRIES,
        last.unwrap_or_else(|| "no reply".into())
    ))
    .with_hint("fproton config set port-forwarding on"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mapping_request_is_the_twelve_bytes_the_rfc_describes() {
        let bytes = encode_request(OP_MAP_UDP, 0, 0, LIFETIME_SEC);
        assert_eq!(
            bytes,
            [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 60],
            "version 0, op 1 (UDP), reserved 0, internal 0, external 0, lifetime 60"
        );
        let with_ports = encode_request(2, 51413, 51413, 3600);
        assert_eq!(with_ports[1], 2, "op 2 is TCP");
        assert_eq!(&with_ports[4..6], &51413u16.to_be_bytes());
        assert_eq!(&with_ports[8..12], &3600u32.to_be_bytes());
    }

    #[test]
    fn a_granted_mapping_decodes_into_its_external_port() {
        // version 0, op 129 (UDP reply), result 0, epoch 1000, internal 0,
        // external 62522, lifetime 60.
        let mut reply = [0u8; 16];
        reply[1] = 129;
        reply[4..8].copy_from_slice(&1000u32.to_be_bytes());
        reply[10..12].copy_from_slice(&62522u16.to_be_bytes());
        reply[12..16].copy_from_slice(&60u32.to_be_bytes());
        let r = decode_response(&reply, OP_MAP_UDP).expect("decodes");
        assert_eq!(r.op, 1);
        assert_eq!(r.result, 0);
        assert_eq!(r.epoch, 1000);
        assert_eq!(r.external_port, 62522);
        assert_eq!(r.lifetime, 60);
    }

    #[test]
    fn a_refusal_keeps_its_result_code_and_a_readable_meaning() {
        let mut reply = [0u8; 16];
        reply[1] = 129;
        reply[2..4].copy_from_slice(&2u16.to_be_bytes());
        let r = decode_response(&reply, OP_MAP_UDP).unwrap();
        assert_eq!(r.result, 2);
        assert!(result_meaning(r.result).contains("not authorised"));
        assert_eq!(result_meaning(0), "success");
        assert_eq!(result_meaning(99), "unknown result code");
    }

    #[test]
    fn a_short_wrong_version_or_wrong_opcode_reply_is_exit_five() {
        assert_eq!(decode_response(&[0u8; 4], 1).unwrap_err().exit_code(), 5);
        let mut bad_version = [0u8; 16];
        bad_version[0] = 3;
        bad_version[1] = 129;
        assert_eq!(decode_response(&bad_version, 1).unwrap_err().exit_code(), 5);
        let mut wrong_op = [0u8; 16];
        wrong_op[1] = 130; // a TCP reply to a UDP request
        assert_eq!(decode_response(&wrong_op, 1).unwrap_err().exit_code(), 5);
    }

    /// Live on this machine: there is no tunnel today, so `port` must be exit 6 —
    /// the one `port` path §4b calls GREEN.
    #[test]
    fn without_a_tunnel_the_query_is_exit_six_and_sends_no_packet() {
        let dir = std::env::temp_dir().join(format!(
            "fproton-natpmp-test-{}-{}",
            std::process::id(),
            crate::state::unix_now()
        ));
        let ctx = Context::for_test(&dir).expect("test context");
        if ctx.paths.sysfs_device(crate::TUNNEL_DEVICE).exists() {
            return; // a tunnel is up: this assertion is about the no-tunnel path only
        }
        let e = request_port(&ctx, &CancelToken::new()).unwrap_err();
        assert_eq!(e.exit_code(), 6);
        assert!(e.to_string().contains("proton0"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
