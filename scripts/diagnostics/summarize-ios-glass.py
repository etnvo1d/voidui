#!/usr/bin/env python3
"""Summarize this probe's stable loupe layer tree without object addresses.
Usage: summarize-ios-glass.py loupe-stable.txt output.json
Raw logs remain local. This records observed parameters, not UIKit's policy.
"""
import hashlib
import json
from pathlib import Path
import re
import sys

source = Path(sys.argv[1]).read_text()
marker = "delegateClass=_UITextMagnifiedGlassLoupeView"
if marker not in source:
    raise SystemExit("The system text loupe was not present in this capture.")
subtree = source[source.index(marker):]
filters = []
for part in re.split(r"^\s+CABackdropLayer$", subtree, flags=re.M)[1:]:
    if "filter name = glassBackground" not in part:
        continue
    part = re.split(r"^\s+CASDFLayer", part, maxsplit=1, flags=re.M)[0]
    values = dict(re.findall(r"filter (\w+) = ([^\n]+)", part))
    values = {key: value for key, value in values.items() if value != "null"}
    scale = re.search(r"^\s+scale = ([^\n]+)", part, re.M)
    if scale:
        filters.append({"backdrop_scale": float(scale[1]), "filter": values})
# Report only the refractive backdrop, avoiding unrelated keyboard filters.
face = next(f for f in filters if float(f["filter"].get("inputInnerRefractionAmount", 0)) < 0)
report = {
    "scope": "Own UIKit probe, UITextLoupeSession held open after presentation; not Notification Center or a physical device.",
    "raw_capture_sha256": hashlib.sha256(source.encode()).hexdigest(),
    "loupe_bounds": re.search(r"bounds=([^\n]+)", subtree)[1],
    "refractive_backdrop": face,
    "highlight": dict(re.findall(r"effect ((?:key|fill|curvature)\w*) = ([^\n]+)", subtree)),
    "has_source_portal": "sourceLayer = <CAPortalLayer:" in subtree,
}
Path(sys.argv[2]).write_text(json.dumps(report, indent=2) + "\n")
