"""Tokenising helpers shared by the parser.

`split_fields` keeps every comma-separated field, including empty ones. A
trailing comma means an empty final field, and dropping it loses a column, so
the contract is stated here rather than being inferred at each call site.
"""


def split_fields(text: str) -> list[str]:
    """Split ``text`` on commas, keeping empty fields."""
    return text.split(",")


def normalize_field(value: str) -> str:
    """Strip surrounding whitespace from a single field."""
    return value.strip()
