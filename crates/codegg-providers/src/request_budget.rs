use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const LIMIT_ENV: &str = "CODEGG_HARNESS_MAX_PROVIDER_REQUESTS";
const ALLOWANCE_LIMIT_ENV: &str = "CODEGG_HARNESS_MAX_PROVIDER_ALLOWANCE_USD";
const RECEIPT_ENV: &str = "CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT";
const MICROS_PER_USD: f64 = 1_000_000.0;

struct Budget {
    limit: usize,
    admitted: AtomicUsize,
    inference_requests: AtomicUsize,
    allowance_limit_micros: Option<u64>,
    allowance_used_micros: AtomicU64,
    allowance_observations: AtomicUsize,
}

impl Budget {
    fn from_environment() -> Result<Option<Self>, String> {
        match std::env::var(LIMIT_ENV) {
            Ok(value) => {
                let limit = value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| format!("{LIMIT_ENV} must be a positive integer"))?;
                if limit == 0 {
                    return Err(format!("{LIMIT_ENV} must be a positive integer"));
                }
                Ok(Some(Self {
                    limit,
                    admitted: AtomicUsize::new(0),
                    inference_requests: AtomicUsize::new(0),
                    allowance_limit_micros: allowance_limit_micros()?,
                    allowance_used_micros: AtomicU64::new(0),
                    allowance_observations: AtomicUsize::new(0),
                }))
            }
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("{LIMIT_ENV} is not valid Unicode"))
            }
        }
    }

    fn admit(&self, expect_allowance: bool) -> Result<usize, usize> {
        let mut current = self.admitted.load(Ordering::Acquire);
        loop {
            if current >= self.limit {
                return Err(self.limit);
            }
            match self.admitted.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if expect_allowance {
                        self.inference_requests.fetch_add(1, Ordering::AcqRel);
                    }
                    return Ok(current + 1);
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn allowance_available(&self) -> bool {
        self.allowance_limit_micros.is_none_or(|limit| {
            self.allowance_observations.load(Ordering::Acquire)
                >= self.inference_requests.load(Ordering::Acquire)
                && self.allowance_used_micros.load(Ordering::Acquire) < limit
        })
    }
}

fn allowance_limit_micros() -> Result<Option<u64>, String> {
    match std::env::var(ALLOWANCE_LIMIT_ENV) {
        Ok(value) => {
            let usd = value.trim().parse::<f64>().map_err(|_| {
                format!("{ALLOWANCE_LIMIT_ENV} must be a positive finite dollar amount")
            })?;
            let micros = usd * MICROS_PER_USD;
            if !micros.is_finite() || micros <= 0.0 || micros > u64::MAX as f64 {
                return Err(format!(
                    "{ALLOWANCE_LIMIT_ENV} must be a positive finite dollar amount"
                ));
            }
            Ok(Some(micros.round() as u64))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("{ALLOWANCE_LIMIT_ENV} is not valid Unicode"))
        }
    }
}

static BUDGET: OnceLock<Result<Option<Budget>, String>> = OnceLock::new();
static RECEIPT_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn admit_opencode_go_request(
    expect_allowance: bool,
) -> Result<(), crate::ProviderError> {
    static ADMISSION_LOCK: Mutex<()> = Mutex::new(());
    let budget = BUDGET.get_or_init(Budget::from_environment);
    match budget {
        Ok(None) => Ok(()),
        Ok(Some(budget)) => {
            let _admission = ADMISSION_LOCK.lock().map_err(|_| {
                crate::ProviderError::api(
                    "harness_request_budget_admission",
                    "provider request budget admission lock is unavailable",
                )
            })?;
            if !budget.allowance_available() {
                return Err(crate::ProviderError::api(
                    "harness_allowance_budget_exhausted",
                    "provider allowance budget exhausted; refusing further OpenCode Go requests",
                ));
            }
            match budget.admit(expect_allowance) {
                Ok(request_number) => {
                    if let Some(path) = std::env::var_os(RECEIPT_ENV) {
                        let _guard = RECEIPT_LOCK.lock().map_err(|_| {
                            crate::ProviderError::api(
                                "harness_request_budget_receipt",
                                "provider request receipt lock is unavailable",
                            )
                        })?;
                        let mut file = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)
                            .map_err(|_| {
                                crate::ProviderError::api(
                                    "harness_request_budget_receipt",
                                    "provider request receipt could not be opened",
                                )
                            })?;
                        use std::io::Write as _;
                        writeln!(
                            file,
                            "{{\"kind\":\"request\",\"request_number\":{request_number},\"limit\":{}}}",
                            budget.limit
                        )
                        .map_err(|_| {
                            crate::ProviderError::api(
                                "harness_request_budget_receipt",
                                "provider request receipt could not be written",
                            )
                        })?;
                    }
                    tracing::info!(
                        target: "codegg_providers::request_budget",
                        request_number,
                        limit = budget.limit,
                        "harness provider request admitted"
                    );
                    Ok(())
                }
                Err(limit) => Err(crate::ProviderError::api(
                    "harness_request_budget_exhausted",
                    format!("provider request budget exhausted at {limit} requests"),
                )),
            }
        }
        Err(message) => Err(crate::ProviderError::api(
            "harness_request_budget_configuration",
            message.clone(),
        )),
    }
}

pub(crate) fn observe_opencode_go_allowance(
    allowance_micros: u64,
) -> Result<(), crate::ProviderError> {
    if let Some(Ok(Some(budget))) = BUDGET.get() {
        let used = saturating_atomic_add(&budget.allowance_used_micros, allowance_micros);
        let cumulative_micros = used.saturating_add(allowance_micros);
        if let Some(path) = std::env::var_os(RECEIPT_ENV) {
            let _guard = RECEIPT_LOCK.lock().map_err(|_| {
                crate::ProviderError::api(
                    "harness_allowance_receipt",
                    "provider allowance receipt lock is unavailable",
                )
            })?;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|_| {
                    crate::ProviderError::api(
                        "harness_allowance_receipt",
                        "provider allowance receipt could not be opened",
                    )
                })?;
            use std::io::Write as _;
            let observation_number = budget.allowance_observations.load(Ordering::Acquire) + 1;
            writeln!(
                file,
                "{{\"kind\":\"allowance_observed\",\"observation_number\":{observation_number},\"allowance_micros\":{allowance_micros},\"cumulative_allowance_micros\":{cumulative_micros}}}"
            )
            .map_err(|_| {
                crate::ProviderError::api(
                    "harness_allowance_receipt",
                    "provider allowance receipt could not be written",
                )
            })?;
        }
        budget.allowance_observations.fetch_add(1, Ordering::AcqRel);
    }
    Ok(())
}

fn saturating_atomic_add(target: &AtomicU64, increment: u64) -> u64 {
    let mut previous = target.load(Ordering::Acquire);
    loop {
        let next = previous.saturating_add(increment);
        match target.compare_exchange_weak(previous, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return previous,
            Err(observed) => previous = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_admits_exactly_its_cap() {
        let budget = Budget {
            limit: 2,
            admitted: AtomicUsize::new(0),
            inference_requests: AtomicUsize::new(0),
            allowance_limit_micros: None,
            allowance_used_micros: AtomicU64::new(0),
            allowance_observations: AtomicUsize::new(0),
        };
        assert_eq!(budget.admit(true), Ok(1));
        assert_eq!(budget.admit(true), Ok(2));
        assert_eq!(budget.admit(true), Err(2));
        assert_eq!(budget.admitted.load(Ordering::Acquire), 2);
    }

    #[test]
    fn allowance_cap_requires_observation_and_blocks_the_next_request() {
        let budget = Budget {
            limit: 5,
            admitted: AtomicUsize::new(0),
            inference_requests: AtomicUsize::new(0),
            allowance_limit_micros: Some(10_000_000),
            allowance_used_micros: AtomicU64::new(0),
            allowance_observations: AtomicUsize::new(0),
        };
        assert!(budget.allowance_available());
        budget.admitted.store(1, Ordering::Release);
        budget.inference_requests.store(1, Ordering::Release);
        assert!(!budget.allowance_available());
        budget
            .allowance_used_micros
            .store(10_000_000, Ordering::Release);
        budget.allowance_observations.store(1, Ordering::Release);
        assert!(!budget.allowance_available());
    }

    #[test]
    fn discovery_requests_do_not_wait_for_inference_allowance_receipts() {
        let budget = Budget {
            limit: 3,
            admitted: AtomicUsize::new(0),
            inference_requests: AtomicUsize::new(0),
            allowance_limit_micros: Some(10_000_000),
            allowance_used_micros: AtomicU64::new(0),
            allowance_observations: AtomicUsize::new(0),
        };
        assert_eq!(budget.admit(false), Ok(1));
        assert!(budget.allowance_available());
        assert_eq!(budget.admit(true), Ok(2));
        assert!(!budget.allowance_available());
    }

    #[test]
    fn allowance_accumulation_saturates_at_the_atomic_limit() {
        let used = AtomicU64::new(u64::MAX - 2);
        assert_eq!(saturating_atomic_add(&used, 10), u64::MAX - 2);
        assert_eq!(used.load(Ordering::Acquire), u64::MAX);
    }
}
