# Daily usage report

`absent-report.txt` is this workspace's daily usage report. It is written by
the nightly export step (`scripts/export_report.sh`), which runs at 03:00 and
writes the file next to this README.

The export step did not run in this environment: there is no scheduler here and
no `scripts/` directory in the tree, so the report is not checked in. The file
appears only once the export step has run; until then, reading it fails.
