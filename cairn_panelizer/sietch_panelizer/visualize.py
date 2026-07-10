"""3D preview rendering: panels colored by mold family, exported as a PNG."""

from __future__ import annotations

from pathlib import Path

import matplotlib
matplotlib.use("Agg")

import matplotlib.pyplot as plt
from matplotlib.colors import hsv_to_rgb
from mpl_toolkits.mplot3d.art3d import Poly3DCollection

from .panel import Panel


def _family_color(family_id: str | None, palette: dict[str, tuple]) -> tuple:
    if family_id is None:
        return (0.6, 0.6, 0.6, 1.0)
    if family_id not in palette:
        hue = (len(palette) * 0.6180339887) % 1.0
        palette[family_id] = (*hsv_to_rgb([hue, 0.55, 0.9]), 1.0)
    return palette[family_id]


def render_preview(panels: list[Panel], path: str | Path, label_panels: bool = False) -> None:
    fig = plt.figure(figsize=(10, 10))
    ax = fig.add_subplot(111, projection="3d")

    palette: dict[str, tuple] = {}
    polys = []
    colors = []
    for panel in panels:
        if panel.is_opening:
            continue
        polys.append(panel.vertices)
        colors.append(_family_color(panel.mold_family_id, palette))

    collection = Poly3DCollection(polys, facecolors=colors, edgecolors="black", linewidths=0.2)
    ax.add_collection3d(collection)

    if label_panels:
        for panel in panels:
            if panel.is_opening:
                continue
            ax.text(*panel.centroid, panel.panel_id, fontsize=4, ha="center")

    all_xyz = [v for p in panels if not p.is_opening for v in p.vertices]
    xs, ys, zs = zip(*all_xyz)
    ax.set_xlim(min(xs), max(xs))
    ax.set_ylim(min(ys), max(ys))
    ax.set_zlim(min(zs), max(zs))
    ax.set_box_aspect((max(xs) - min(xs), max(ys) - min(ys), max(zs) - min(zs)))
    ax.set_xlabel("X (mm)")
    ax.set_ylabel("Y (mm)")
    ax.set_zlabel("Z (mm)")
    ax.set_title("Cairn Trillium Dome — panels colored by mold family")
    ax.view_init(elev=28, azim=-60)

    fig.tight_layout()
    fig.savefig(str(path), dpi=180)
    plt.close(fig)
