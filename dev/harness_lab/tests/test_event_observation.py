"""Loss-aware ACP observation contract shared by autonomy and sustained runs."""
import unittest

from dev.harness_lab.event_observation import observe_notifications


def _update(kind, **fields):
    return {"method": "session/update", "params": {"update": {
        "sessionUpdate": kind, **fields,
    }}}


class EventObservationTest(unittest.TestCase):
    def test_gap_is_unmeasured_and_counts_dropped_events(self):
        receipt = observe_notifications([
            _update("tool_call", toolCallId="call-1"),
            _update("codegg_event_gap", dropped=3, source="daemon_socket"),
            _update("tool_call_update", toolCallId="call-1", status="completed"),
        ], drained=True)

        self.assertEqual(receipt["event_observation_state"], "unmeasured_gap")
        self.assertEqual(receipt["event_gap_count"], 1)
        self.assertEqual(receipt["event_gap_dropped"], 3)
        self.assertEqual(receipt["tool_unpaired_start_count"], 0)

    def test_gap_free_drained_stream_is_observed_and_tool_pairing_is_counted(self):
        receipt = observe_notifications([
            _update("tool_call", toolCallId="call-1"),
            _update("tool_call_update", toolCallId="call-1", status="failed"),
        ], drained=True)

        self.assertEqual(receipt["event_observation_state"], "observed_complete")
        self.assertEqual(receipt["event_gap_count"], 0)
        self.assertEqual(receipt["tool_started_count"], 1)
        self.assertEqual(receipt["tool_terminal_count"], 1)
        self.assertEqual(receipt["tool_unpaired_start_count"], 0)

    def test_undrained_stream_is_not_claimed_gap_free(self):
        receipt = observe_notifications([], drained=False)
        self.assertEqual(receipt["event_observation_state"], "not_instrumented")


if __name__ == "__main__":
    unittest.main()
