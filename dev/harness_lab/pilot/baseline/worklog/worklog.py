"""Append-only JSONL worklog. Implement the public API described in README.md."""

class Worklog:
    def __init__(self, path):
        raise NotImplementedError

    def append(self, text, author, created_at=None):
        raise NotImplementedError

    def events(self, author=None, newest_first=False):
        raise NotImplementedError

    def summarize(self):
        raise NotImplementedError
