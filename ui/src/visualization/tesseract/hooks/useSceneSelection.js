import { useState, useCallback } from 'react';

/**
 * Tracks the currently selected scene object.
 *
 * A selection is { type: 'episode' | 'cause', id: string, data: object }
 * or null when nothing is selected.
 */
export function useSceneSelection() {
  const [selected, setSelected] = useState(null);

  const select = useCallback((item) => setSelected(item), []);

  const clear = useCallback(() => setSelected(null), []);

  const toggle = useCallback((item) => {
    setSelected((current) =>
      current?.id === item?.id ? null : item,
    );
  }, []);

  return { selected, select, clear, toggle };
}
