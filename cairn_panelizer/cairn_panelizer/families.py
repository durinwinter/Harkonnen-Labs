"""Group structurally-similar panels into mold/repetition families.

Two panels belong to the same family when their sorted edge lengths match
within `family_tolerance_mm`. This is a simple bucketing approach (round each
edge length to the nearest tolerance step, then key on the sorted tuple) —
reliable and easy to audit, even if it isn't a true clustering algorithm.
"""

from __future__ import annotations

from collections import OrderedDict

from .config import DomeConfig
from .panel import Panel


def assign_families(panels: list[Panel], config: DomeConfig) -> int:
    """Assign mold_family_id to every non-opening panel. Returns family count."""
    signature_to_family: "OrderedDict[tuple[float, ...], str]" = OrderedDict()

    for panel in panels:
        if panel.is_opening:
            continue
        signature = panel.edge_signature(config.family_tolerance_mm)
        if signature not in signature_to_family:
            family_id = f"F{len(signature_to_family):03d}"
            signature_to_family[signature] = family_id
        panel.mold_family_id = signature_to_family[signature]

    return len(signature_to_family)
