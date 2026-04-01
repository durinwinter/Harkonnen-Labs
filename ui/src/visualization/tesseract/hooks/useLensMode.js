import { useState, useCallback } from 'react';

export const LENS_MODES = ['failure', 'memory', 'intervention'];

/**
 * Tracks the active lens mode for the tesseract scene.
 *
 * failure      — emphasises inner cube + connectors; dims outer
 * memory       — balanced; shows both cubes equally
 * intervention — emphasises connector arcs; mutes both cubes
 */
export function useLensMode(initial = 'memory') {
  const [lensMode, setLensModeRaw] = useState(initial);

  const setLensMode = useCallback((mode) => {
    if (LENS_MODES.includes(mode)) setLensModeRaw(mode);
  }, []);

  const cycleLens = useCallback(() => {
    setLensModeRaw((current) => {
      const idx = LENS_MODES.indexOf(current);
      return LENS_MODES[(idx + 1) % LENS_MODES.length];
    });
  }, []);

  return { lensMode, setLensMode, cycleLens };
}
