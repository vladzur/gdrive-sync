// SPDX-License-Identifier: GPL-3.0-or-later
//! Política de reintentos con espera exponencial y *jitter* (RNF-05).

use std::time::Duration;

use crate::error::{DriveError, ErrorClass};

/// Política de reintentos.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Espera inicial.
    pub base: Duration,
    /// Espera máxima entre intentos.
    pub max_delay: Duration,
    /// Número máximo de reintentos (además del intento inicial).
    pub max_retries: u32,
    /// Fracción de aleatoriedad aplicada a cada espera (`0.0` = sin *jitter*).
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(2),
            max_delay: Duration::from_secs(300),
            max_retries: 8,
            jitter: 0.2,
        }
    }
}

impl RetryPolicy {
    /// Política sin espera útil para tests que no deben dormir.
    #[must_use]
    pub fn immediate() -> Self {
        Self {
            base: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
            max_retries: 3,
            jitter: 0.0,
        }
    }

    /// Espera para el intento `attempt`, con `sample` en `[0, 1)` para el *jitter*.
    ///
    /// El resultado queda en `[delay*(1-jitter), delay*(1+jitter)]`, acotado por
    /// [`RetryPolicy::max_delay`] antes de aplicar el *jitter*.
    #[must_use]
    pub fn delay(&self, attempt: u32, sample: f64) -> Duration {
        let mut seconds = self.base.as_secs_f64();
        for _ in 0..attempt.min(20) {
            seconds *= 2.0;
        }
        let seconds = seconds.min(self.max_delay.as_secs_f64());

        let sample = sample.clamp(0.0, 1.0);
        let factor = 1.0 - self.jitter + (2.0 * self.jitter * sample);
        Duration::from_secs_f64((seconds * factor).max(0.0))
    }

    /// Indica si corresponde reintentar.
    #[must_use]
    pub fn should_retry(&self, attempt: u32, error: &DriveError) -> bool {
        attempt < self.max_retries && error.class() == ErrorClass::Retryable
    }

    /// Espera concreta antes del siguiente intento.
    ///
    /// Si el servidor envió `Retry-After`, se respeta ese valor en lugar del
    /// cálculo exponencial: es información autoritativa sobre cuándo reintentar.
    #[must_use]
    pub fn wait_for(&self, attempt: u32, error: &DriveError) -> Duration {
        error
            .retry_after()
            .unwrap_or_else(|| self.delay(attempt, jitter_sample()))
    }
}

/// Muestra pseudoaleatoria en `[0, 1)` para el *jitter*.
///
/// No se agrega un generador criptográfico: el *jitter* solo busca desincronizar
/// reintentos, y la sub-precisión del reloj alcanza para eso.
fn jitter_sample() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());

    f64::from(nanos) / 1_000_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RetryPolicy {
        RetryPolicy {
            base: Duration::from_secs(2),
            max_delay: Duration::from_secs(300),
            max_retries: 8,
            jitter: 0.0,
        }
    }

    #[test]
    fn doubles_the_delay_on_each_attempt() {
        let policy = policy();
        let delays: Vec<u64> = (0..5)
            .map(|attempt| policy.delay(attempt, 0.0).as_secs())
            .collect();

        assert_eq!(delays, vec![2, 4, 8, 16, 32]);
    }

    #[test]
    fn caps_the_delay_at_the_configured_maximum() {
        let policy = policy();

        assert_eq!(policy.delay(30, 0.0), Duration::from_secs(300));
    }

    #[test]
    fn applies_jitter_within_the_configured_bounds() {
        let policy = RetryPolicy {
            base: Duration::from_secs(10),
            jitter: 0.2,
            ..policy()
        };

        // sample 0 → -20%; sample 1 → +20%.
        assert_eq!(policy.delay(0, 0.0), Duration::from_secs(8));
        assert_eq!(policy.delay(0, 1.0), Duration::from_secs(12));
        assert_eq!(policy.delay(0, 0.5), Duration::from_secs(10));
    }

    #[test]
    fn only_retryable_errors_are_retried_and_only_within_the_limit() {
        let policy = policy();
        let retryable = DriveError::Network("timeout".to_owned());
        let fatal = DriveError::NotFound;

        assert!(policy.should_retry(0, &retryable));
        assert!(policy.should_retry(7, &retryable));
        assert!(
            !policy.should_retry(8, &retryable),
            "no debe superar max_retries"
        );
        assert!(!policy.should_retry(0, &fatal));
        assert!(!policy.should_retry(0, &DriveError::Unauthorized));
    }

    #[test]
    fn honours_the_server_retry_after_hint() {
        let policy = policy();
        let error = DriveError::RateLimited {
            retry_after: Some(Duration::from_secs(7)),
        };

        assert_eq!(policy.wait_for(0, &error), Duration::from_secs(7));
    }
}
