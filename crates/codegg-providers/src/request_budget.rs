use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const LIMIT_ENV: &str = "CODEGG_HARNESS_MAX_PROVIDER_REQUESTS";
const RECEIPT_ENV: &str = "CODEGG_HARNESS_PROVIDER_REQUEST_RECEIPT";

struct Budget {
    limit: usize,
    admitted: AtomicUsize,
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
                }))
            }
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("{LIMIT_ENV} is not valid Unicode"))
            }
        }
    }

    fn admit(&self) -> Result<usize, usize> {
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
                Ok(_) => return Ok(current + 1),
                Err(observed) => current = observed,
            }
        }
    }
}

pub(crate) fn admit_opencode_go_request() -> Result<(), crate::ProviderError> {
    static BUDGET: OnceLock<Result<Option<Budget>, String>> = OnceLock::new();
    static RECEIPT_LOCK: Mutex<()> = Mutex::new(());
    let budget = BUDGET.get_or_init(Budget::from_environment);
    match budget {
        Ok(None) => Ok(()),
        Ok(Some(budget)) => match budget.admit() {
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
                        "{{\"request_number\":{request_number},\"limit\":{}}}",
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
        },
        Err(message) => Err(crate::ProviderError::api(
            "harness_request_budget_configuration",
            message.clone(),
        )),
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
        };
        assert_eq!(budget.admit(), Ok(1));
        assert_eq!(budget.admit(), Ok(2));
        assert_eq!(budget.admit(), Err(2));
        assert_eq!(budget.admitted.load(Ordering::Acquire), 2);
    }
}
