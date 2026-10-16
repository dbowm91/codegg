use m008_retry_delay::{retry_delay_ms, RetryDelayError};

#[test]
fn exponential_growth_is_capped() {
    assert_eq!(retry_delay_ms(1, 100, 450, None), Ok(100));
    assert_eq!(retry_delay_ms(2, 100, 450, None), Ok(200));
    assert_eq!(retry_delay_ms(3, 100, 450, None), Ok(400));
    assert_eq!(retry_delay_ms(4, 100, 450, None), Ok(450));
}

#[test]
fn retry_after_is_a_lower_bound_before_the_cap() {
    assert_eq!(retry_delay_ms(1, 100, 450, Some(250)), Ok(250));
    assert_eq!(retry_delay_ms(3, 100, 450, Some(250)), Ok(400));
    assert_eq!(retry_delay_ms(1, 100, 450, Some(900)), Ok(450));
}

#[test]
fn zero_attempt_is_rejected() {
    assert_eq!(retry_delay_ms(0, 100, 450, None), Err(RetryDelayError::AttemptZero));
}

#[test]
fn cap_below_base_and_zero_cap_are_valid() {
    assert_eq!(retry_delay_ms(1, 100, 50, None), Ok(50));
    assert_eq!(retry_delay_ms(2, 100, 0, Some(900)), Ok(0));
}

#[test]
fn large_attempt_saturates_without_overflow() {
    assert_eq!(retry_delay_ms(u32::MAX, 1, 900, None), Ok(900));
}
