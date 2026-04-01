---
tags: [lamdet, parity, ceres-station, sling, laminate, peak-detection]
summary: Middle laminate peak near the LamDet mid-band may be a sling-like anomaly rather than a true laminate peak.
---
During LamDet parity work against the Ceres Station example corpus, a recurring mismatch pattern appeared where Rust detected a middle laminate peak around the LamDet mid band while the LamDet Python reference did not.

Working note for Coobie:
- A detected middle laminate peak in the 100-120 region may in fact be a sling-like anomaly or related false-positive thermal structure rather than a valid laminate-mid peak.
- Treat repeated Rust-only middle-band detections as a causal investigation target during parity tuning.
- When reviewing SHIM/TAB disagreements, check whether the middle-band peak should be classified as sling behavior before assuming it is a valid laminate line.
