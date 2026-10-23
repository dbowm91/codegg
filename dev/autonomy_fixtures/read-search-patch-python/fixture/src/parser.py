"""A small record parser built on the shared tokenizer."""
from tokenizer import normalize_field, split_fields


def parse_record(line: str) -> list[str]:
    """Parse one comma-separated record into its normalized fields.

    Every field in the record is significant, including an empty trailing
    field: ``"a,b,"`` has three fields, the last of which is empty.
    """
    fields = split_fields(line)
    while fields and fields[-1] == "":
        fields.pop()
    return [normalize_field(field) for field in fields]
