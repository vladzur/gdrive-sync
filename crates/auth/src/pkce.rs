// SPDX-License-Identifier: GPL-3.0-or-later
//! PKCE (`RFC 7636`): verificación del desafío S256.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};
use sha2::{Digest, Sha256};

/// Calcula el `code_challenge` con el método `S256` (`RFC 7636` §4.2).
///
/// `code_challenge = BASE64URL(SHA256(code_verifier))`, sin relleno.
#[must_use]
pub fn s256_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Genera un par PKCE nuevo: verificador aleatorio y desafío `S256`.
///
/// El verificador se genera con un CSPRNG y cumple el rango de longitud que
/// exige la norma (43 a 128 caracteres).
#[must_use]
pub fn generate() -> (PkceCodeChallenge, PkceCodeVerifier) {
    PkceCodeChallenge::new_random_sha256()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vector de prueba del `RFC 7636`, Apéndice B.
    const RFC_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const RFC_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn matches_the_rfc_7636_appendix_b_vector() {
        assert_eq!(s256_challenge(RFC_VERIFIER), RFC_CHALLENGE);
    }

    #[test]
    fn agrees_with_the_oauth2_implementation() {
        let (challenge, verifier) = generate();

        assert_eq!(
            challenge.as_str(),
            s256_challenge(verifier.secret()),
            "el cálculo propio debe coincidir con el de oauth2"
        );
    }

    #[test]
    fn generates_a_verifier_within_the_rfc_length_range() {
        let (_, verifier) = generate();
        let len = verifier.secret().len();

        assert!((43..=128).contains(&len), "longitud inválida: {len}");
    }

    #[test]
    fn verifier_is_url_safe() {
        let (_, verifier) = generate();

        assert!(
            verifier.secret().chars().all(|c| c.is_ascii_alphanumeric()
                || c == '-'
                || c == '_'
                || c == '.'
                || c == '~'),
            "el verificador contiene caracteres fuera de RFC 3986"
        );
    }
}
