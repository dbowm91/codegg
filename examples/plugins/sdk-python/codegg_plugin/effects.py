"""Builder functions for ``UiEffect`` and ``UiNode`` JSON objects."""


def emit_chat(content, fmt="markdown"):
    """Return an ``EmitChat`` effect."""
    return {
        "type": "emit_chat",
        "block": {
            "format": fmt,
            "content": content,
        },
    }


def show_toast(level, message):
    """Return a ``ShowToast`` effect."""
    return {
        "type": "show_toast",
        "toast": {
            "level": level,
            "message": message,
        },
    }


def open_dialog(id, title, body, modal=True):
    """Return an ``OpenDialog`` effect."""
    return {
        "type": "open_dialog",
        "dialog": {
            "id": id,
            "title": title,
            "body": body,
            "modal": modal,
        },
    }


def open_panel(id, title, placement, body):
    """Return an ``OpenPanel`` effect."""
    return {
        "type": "open_panel",
        "panel": {
            "id": id,
            "title": title,
            "placement": placement,
            "body": body,
        },
    }


def add_status_item(id, placement, body, label=None):
    """Return an ``AddStatusItem`` effect."""
    item = {
        "id": id,
        "placement": placement,
        "body": body,
    }
    if label is not None:
        item["label"] = label
    return {
        "type": "add_status_item",
        "item": item,
    }


def text_node(text):
    """Return a ``Text`` UiNode."""
    return {"kind": "text", "text": text}


def markdown_node(md):
    """Return a ``Markdown`` UiNode."""
    return {"kind": "markdown", "markdown": md}


def table_node(columns, rows):
    """Return a ``Table`` UiNode."""
    return {"kind": "table", "columns": columns, "rows": rows}


def key_value_node(entries):
    """Return a ``KeyValue`` UiNode.

    *entries* is a list of ``(key, value)`` tuples.
    """
    return {
        "kind": "key_value",
        "entries": [{"key": k, "value": v} for k, v in entries],
    }
