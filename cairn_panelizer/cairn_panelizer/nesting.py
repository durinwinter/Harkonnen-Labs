"""Greedy shelf-packing of flattened panel outlines onto fixed-size sheets.

This is not a true nesting optimizer (no rotation search, no irregular
boundary packing) — it's a predictable shelf algorithm that places panel
bounding boxes left-to-right in rows and starts a new sheet when the
current one fills up. That's enough to turn "1,688 triangles" into a
believable, orderable cut-sheet plan; a real nesting pass is future work
(see README "Next steps").
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .config import DomeConfig
from .flatten2d import flatten_triangle
from .panel import Panel


@dataclass
class NestedPanel:
    panel_id: str
    family_id: str | None
    outline_mm: list[tuple[float, float]]
    bbox_width_mm: float
    bbox_height_mm: float
    area_mm2: float


@dataclass
class NestingSheet:
    sheet_id: str
    width_mm: float
    height_mm: float
    margin_mm: float
    panels: list[NestedPanel] = field(default_factory=list)

    @property
    def usable_area_mm2(self) -> float:
        return max(self.width_mm - 2 * self.margin_mm, 0.0) * max(self.height_mm - 2 * self.margin_mm, 0.0)

    @property
    def utilization_pct(self) -> float:
        if self.usable_area_mm2 <= 0:
            return 0.0
        used = sum(p.area_mm2 for p in self.panels)
        return 100.0 * used / self.usable_area_mm2

    def to_record(self) -> dict:
        return {
            "sheet_id": self.sheet_id,
            "width_mm": self.width_mm,
            "height_mm": self.height_mm,
            "panel_count": len(self.panels),
            "utilization_pct": round(self.utilization_pct, 2),
            "panels": [
                {
                    "panel_id": p.panel_id,
                    "family_id": p.family_id,
                    "outline_mm": [[round(x, 2), round(y, 2)] for x, y in p.outline_mm],
                    "bbox_width_mm": round(p.bbox_width_mm, 2),
                    "bbox_height_mm": round(p.bbox_height_mm, 2),
                }
                for p in self.panels
            ],
        }


def _bbox(outline: list[tuple[float, float]]) -> tuple[float, float, float, float]:
    xs = [p[0] for p in outline]
    ys = [p[1] for p in outline]
    return max(xs) - min(xs), max(ys) - min(ys), min(xs), min(ys)


def nest_panels(panels: list[Panel], config: DomeConfig) -> list[NestingSheet]:
    """Pack every fabricated panel's flattened outline onto sheets, largest first."""
    cfg = config.nesting
    spacing = cfg.spacing_mm
    margin = cfg.margin_mm
    usable_w = cfg.sheet_width_mm - 2 * margin
    usable_h = cfg.sheet_height_mm - 2 * margin

    fab_panels = [p for p in panels if not p.is_opening]
    ordered = sorted(fab_panels, key=lambda p: p.area_mm2, reverse=True)

    sheets: list[NestingSheet] = []
    sheet = cursor_x = cursor_y = row_height = None

    def start_sheet() -> None:
        nonlocal sheet, cursor_x, cursor_y, row_height
        sheet = NestingSheet(
            sheet_id=f"SHEET-{len(sheets):02d}",
            width_mm=cfg.sheet_width_mm,
            height_mm=cfg.sheet_height_mm,
            margin_mm=margin,
        )
        sheets.append(sheet)
        cursor_x = cursor_y = row_height = 0.0

    start_sheet()

    for panel in ordered:
        outline = flatten_triangle(panel.edge_lengths)
        width, height, min_x, min_y = _bbox(outline)

        if cursor_x + width > usable_w:
            cursor_x = 0.0
            cursor_y += row_height + spacing
            row_height = 0.0

        if cursor_y + height > usable_h:
            start_sheet()

        offset_x = margin + cursor_x - min_x
        offset_y = margin + cursor_y - min_y

        sheet.panels.append(NestedPanel(
            panel_id=panel.panel_id,
            family_id=panel.mold_family_id,
            outline_mm=[(x + offset_x, y + offset_y) for x, y in outline],
            bbox_width_mm=width,
            bbox_height_mm=height,
            area_mm2=panel.area_mm2,
        ))

        cursor_x += width + spacing
        row_height = max(row_height, height)

    return sheets
