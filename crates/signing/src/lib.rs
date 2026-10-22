//! Requests from the hosted gateway to a user's Sculptured carry the user's id and an HMAC over the
//! request, so an instance only ever acts for the user the gateway authenticated.
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub const USER_HEADER: &str = "x-sculptured-user";
pub const TS_HEADER: &str = "x-sculptured-ts";
pub const SIG_HEADER: &str = "x-sculptured-sig";
pub const MAX_SKEW_SECS: i64 = 60;

fn mac(secret: &[u8], method: &str, path_and_query: &str, user: &str, ts: i64) -> Hmac<Sha256> {
    let mut m = Hmac::<Sha256>::new_from_slice(secret).expect("any key length");
    m.update(format!("{method}\n{path_and_query}\n{user}\n{ts}").as_bytes());
    m
}

#[must_use]
pub fn sign(secret: &[u8], method: &str, path_and_query: &str, user: &str, ts: i64) -> String {
    hex::encode(
        mac(secret, method, path_and_query, user, ts)
            .finalize()
            .into_bytes(),
    )
}

#[must_use]
pub fn verify(
    secret: &[u8],
    method: &str,
    path_and_query: &str,
    user: &str,
    ts: i64,
    sig: &str,
    now: i64,
) -> bool {
    if (now - ts).abs() > MAX_SKEW_SECS {
        return false;
    }
    let Ok(given) = hex::decode(sig) else {
        return false;
    };
    let want = mac(secret, method, path_and_query, user, ts)
        .finalize()
        .into_bytes();
    given.len() == want.len() && bool::from(given.as_slice().ct_eq(&want[..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies_only_for_the_same_request_user_and_time_window() {
        let s = b"secret";
        let sig = sign(s, "GET", "/api/teams?x=1", "u1", 1_000);
        assert_eq!(sig.len(), 64);
        assert!(verify(s, "GET", "/api/teams?x=1", "u1", 1_000, &sig, 1_030));
        assert!(
            !verify(s, "GET", "/api/teams?x=1", "u2", 1_000, &sig, 1_030),
            "other user"
        );
        assert!(
            !verify(s, "POST", "/api/teams?x=1", "u1", 1_000, &sig, 1_030),
            "other method"
        );
        assert!(
            !verify(s, "GET", "/api/teams", "u1", 1_000, &sig, 1_030),
            "other path"
        );
        assert!(
            !verify(b"other", "GET", "/api/teams?x=1", "u1", 1_000, &sig, 1_030),
            "other secret"
        );
        assert!(
            !verify(s, "GET", "/api/teams?x=1", "u1", 1_000, &sig, 1_061),
            "too old"
        );
        assert!(
            !verify(s, "GET", "/api/teams?x=1", "u1", 1_000, &sig, 939),
            "from the future"
        );
        assert!(
            !verify(s, "GET", "/api/teams?x=1", "u1", 1_000, "zz", 1_000),
            "malformed"
        );
    }

    #[test]
    fn a_fixed_vector_never_changes() {
        // The gateway and controld are released separately; this pins the format.
        assert_eq!(
            sign(b"k", "GET", "/api/teams", "u", 0),
            "eca69cfb4f922286131c22342e0d6c1a579cf8694fe8fc7b061886d7e111e190"
        );
    }
}
