"""Panel data model and extraction from a faceted mesh."""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np
import trimesh


@dataclass
class Panel:
    panel_id: str
    face_index: int
    vertices: list[tuple[float, float, float]]
    edge_lengths: list[float]
    area_mm2: float
    normal: tuple[float, float, float]
    centroid: tuple[float, float, float]
    neighbor_ids: list[str] = field(default_factory=list)
    dihedral_angles_deg: dict[str, float] = field(default_factory=dict)
    panel_type: str = "exterior_skin"
    is_opening: bool = False
    mold_family_id: str | None = None

    # Populated by materials.assign_recipes(): one entry per layer in the
    # Cairn material stack, each carrying its own volume/mass/cost estimate.
    layer_stack: list[str] = field(default_factory=list)
    recipe_assignments: list[dict] = field(default_factory=list)
    estimated_mass_kg: float | None = None
    estimated_cost_usd: float | None = None
    warnings: list[str] = field(default_factory=list)

    def edge_signature(self, tolerance_mm: float) -> tuple[float, ...]:
        """Rounded, sorted edge lengths used to group repeatable panel families."""
        if tolerance_mm <= 0:
            return tuple(sorted(self.edge_lengths))
        return tuple(sorted(round(length / tolerance_mm) * tolerance_mm for length in self.edge_lengths))

    def to_record(self) -> dict:
        """Flat dict representation suitable for JSON/CSV export."""
        return {
            "panel_id": self.panel_id,
            "panel_type": self.panel_type,
            "is_opening": self.is_opening,
            "mold_family_id": self.mold_family_id,
            "vertices": self.vertices,
            "edge_lengths_mm": [round(v, 2) for v in self.edge_lengths],
            "area_mm2": round(self.area_mm2, 2),
            "normal": [round(v, 4) for v in self.normal],
            "centroid": [round(v, 2) for v in self.centroid],
            "neighbor_ids": self.neighbor_ids,
            "dihedral_angles_deg": {k: round(v, 2) for k, v in self.dihedral_angles_deg.items()},
            "layer_stack": self.layer_stack,
            "recipe_assignments": self.recipe_assignments,
            "estimated_mass_kg": round(self.estimated_mass_kg, 3) if self.estimated_mass_kg is not None else None,
            "estimated_cost_usd": round(self.estimated_cost_usd, 2) if self.estimated_cost_usd is not None else None,
            "warnings": self.warnings,
        }


def _face_edge_lengths(coords: np.ndarray) -> list[float]:
    n = len(coords)
    return [float(np.linalg.norm(coords[(i + 1) % n] - coords[i])) for i in range(n)]


def build_panels(mesh: trimesh.Trimesh) -> list[Panel]:
    """Build one Panel per mesh face, including adjacency and dihedral angles."""
    n_faces = len(mesh.faces)
    panel_ids = [f"P{idx:04d}" for idx in range(n_faces)]

    panels: list[Panel] = []
    for idx in range(n_faces):
        coords = mesh.vertices[mesh.faces[idx]]
        panels.append(
            Panel(
                panel_id=panel_ids[idx],
                face_index=idx,
                vertices=[tuple(round(c, 2) for c in v) for v in coords],
                edge_lengths=_face_edge_lengths(coords),
                area_mm2=float(mesh.area_faces[idx]),
                normal=tuple(float(c) for c in mesh.face_normals[idx]),
                centroid=tuple(float(c) for c in coords.mean(axis=0)),
            )
        )

    # Adjacency + dihedral angles come straight from trimesh's face-adjacency tables.
    adjacency = mesh.face_adjacency
    angles_deg = np.degrees(mesh.face_adjacency_angles)
    for (a, b), angle in zip(adjacency, angles_deg):
        panels[a].neighbor_ids.append(panel_ids[b])
        panels[b].neighbor_ids.append(panel_ids[a])
        panels[a].dihedral_angles_deg[panel_ids[b]] = float(angle)
        panels[b].dihedral_angles_deg[panel_ids[a]] = float(angle)

    return panels
