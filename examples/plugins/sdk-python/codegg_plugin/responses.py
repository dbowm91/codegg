"""Builder functions for ``PluginResponse`` JSON objects."""


def diagnostic(level, message):
    """Return a single diagnostic dict."""
    return {"level": level, "message": message}


def ok_response(effects=None, data=None, diagnostics=None):
    """Build a successful ``PluginResponse``."""
    return {
        "ok": True,
        "effects": effects or [],
        "data": data or {},
        "diagnostics": diagnostics or [],
    }


def error_response(message, data=None):
    """Build a failure ``PluginResponse`` with an error diagnostic."""
    return {
        "ok": False,
        "effects": [],
        "data": data or {},
        "diagnostics": [{"level": "error", "message": message}],
    }
