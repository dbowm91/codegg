"""codegg_plugin — vendorable Python helper for codegg process plugins."""

from codegg_plugin.effects import (
    add_status_item,
    emit_chat,
    markdown_node,
    open_dialog,
    open_panel,
    show_toast,
    table_node,
    text_node,
    key_value_node,
)
from codegg_plugin.io import InvalidInvocationError, read_invocation, write_diagnostic, write_response
from codegg_plugin.protocol import PLUGIN_PROTOCOL_VERSION, is_valid_invocation
from codegg_plugin.responses import diagnostic, error_response, ok_response

__all__ = [
    "PLUGIN_PROTOCOL_VERSION",
    "is_valid_invocation",
    "read_invocation",
    "write_response",
    "write_diagnostic",
    "InvalidInvocationError",
    "ok_response",
    "error_response",
    "diagnostic",
    "emit_chat",
    "show_toast",
    "open_dialog",
    "open_panel",
    "add_status_item",
    "text_node",
    "markdown_node",
    "table_node",
    "key_value_node",
]
