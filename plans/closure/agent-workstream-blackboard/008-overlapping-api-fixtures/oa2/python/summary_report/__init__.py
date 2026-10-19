"""M008 fixture starter: strict project-summary consumer."""

from .consumer import SummaryError, render_report, validate_summary

__all__ = ["SummaryError", "render_report", "validate_summary"]
