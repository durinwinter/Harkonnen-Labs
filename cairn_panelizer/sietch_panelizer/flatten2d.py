"""Exact planar unrolling of flat triangular panels into 2D outlines.

A triangle is always planar, so it can be unrolled into 2D while preserving
all three edge lengths exactly via the law of cosines — no approximation.
"""

from __future__ import annotations

import math


def flatten_triangle(edge_lengths: list[float]) -> list[tuple[float, float]]:
    """Unroll a planar triangle into 2D, preserving its three edge lengths.

    `edge_lengths` follows the Panel convention: edges[i] is the length of
    the edge from vertex i to vertex (i+1) % 3, so edges[0] connects v0->v1
    and edges[2] connects v2->v0.
    """
    e0, e1, e2 = edge_lengths
    p0 = (0.0, 0.0)
    p1 = (e0, 0.0)
    cos_angle = (e0 ** 2 + e2 ** 2 - e1 ** 2) / (2.0 * e0 * e2)
    cos_angle = max(-1.0, min(1.0, cos_angle))
    angle = math.acos(cos_angle)
    p2 = (e2 * math.cos(angle), e2 * math.sin(angle))
    return [p0, p1, p2]
