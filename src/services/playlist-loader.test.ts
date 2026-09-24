import { describe, expect, it, vi } from 'vitest';
import { loadAllPages } from './playlist-loader';

describe('loadAllPages', () => {
  it('loads every page and removes duplicate items', async () => {
    const next = vi.fn()
      .mockResolvedValueOnce([{ id: '2' }, { id: '3' }])
      .mockResolvedValueOnce([{ id: '3' }, { id: '4' }])
      .mockResolvedValueOnce([]);

    await expect(loadAllPages([{ id: '1' }], next, (item) => item.id)).resolves.toEqual([
      { id: '1' }, { id: '2' }, { id: '3' }, { id: '4' },
    ]);
    expect(next).toHaveBeenCalledTimes(3);
  });

  it('stops when a provider repeats a page', async () => {
    const next = vi.fn().mockResolvedValue([{ id: '1' }]);
    await expect(loadAllPages([{ id: '1' }], next, (item) => item.id)).resolves.toEqual([{ id: '1' }]);
    expect(next).toHaveBeenCalledTimes(1);
  });
});
