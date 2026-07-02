"""Protocol constants and validation helpers for the codegg plugin wire format."""

PLUGIN_PROTOCOL_VERSION = 1


def is_valid_invocation(obj):
    """Check that *obj* is a dict with a compatible ``protocol_version``."""
    if not isinstance(obj, dict):
        return False
    version = obj.get("protocol_version")
    return isinstance(version, int) and version == PLUGIN_PROTOCOL_VERSION
