#[cfg(test)]
mod tests {
    use codegg::permission::DoomLoopDetector;

    fn args() -> serde_json::Value {
        serde_json::json!({})
    }

    #[test]
    fn test_doom_loop_detector_no_loop() {
        let mut detector = DoomLoopDetector::new(10, 5);

        detector.record_tool_call("read", &args());
        detector.record_tool_call("edit", &args());
        detector.record_tool_call("bash", &args());

        assert!(!detector.is_doom_loop());
    }

    #[test]
    fn test_doom_loop_detector_detects_loop() {
        let mut detector = DoomLoopDetector::new(10, 5);

        for _ in 0..5 {
            detector.record_tool_call("read", &args());
        }

        assert!(detector.is_doom_loop());
    }

    #[test]
    fn test_doom_loop_detector_resets() {
        let mut detector = DoomLoopDetector::new(10, 5);

        for _ in 0..5 {
            detector.record_tool_call("read", &args());
        }

        assert!(detector.is_doom_loop());

        detector.reset();

        assert!(!detector.is_doom_loop());
    }

    #[test]
    fn test_doom_loop_detector_window_eviction() {
        let mut detector = DoomLoopDetector::new(3, 2);

        detector.record_tool_call("read", &args());
        detector.record_tool_call("edit", &args());
        detector.record_tool_call("bash", &args());

        assert!(!detector.is_doom_loop());

        detector.record_tool_call("read", &args());
        detector.record_tool_call("edit", &args());

        assert!(!detector.is_doom_loop());
    }

    #[test]
    fn test_doom_loop_detector_mixed_calls() {
        let mut detector = DoomLoopDetector::new(10, 3);

        for _ in 0..3 {
            detector.record_tool_call("read", &args());
        }
        detector.record_tool_call("edit", &args());

        assert!(!detector.is_doom_loop());
    }

    #[test]
    fn test_doom_loop_detector_consecutive_resets() {
        let mut detector = DoomLoopDetector::new(10, 3);

        for _ in 0..3 {
            detector.record_tool_call("read", &args());
        }
        assert!(detector.is_doom_loop());

        detector.record_tool_call("edit", &args());
        assert!(!detector.is_doom_loop());

        for _ in 0..3 {
            detector.record_tool_call("read", &args());
        }
        assert!(detector.is_doom_loop());
    }

    #[test]
    fn test_doom_loop_detector_threshold_not_reached() {
        let mut detector = DoomLoopDetector::new(10, 5);

        for _ in 0..4 {
            detector.record_tool_call("read", &args());
        }

        assert!(!detector.is_doom_loop());
    }
}
