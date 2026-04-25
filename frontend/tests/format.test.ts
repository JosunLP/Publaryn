import { describe, expect, test } from 'bun:test';

import { formatFileSize } from '../src/utils/format';

describe('format helpers', () => {
  test('formats file sizes across binary boundaries', () => {
    expect(formatFileSize(undefined)).toBe('0 B');
    expect(formatFileSize(-1)).toBe('0 B');
    expect(formatFileSize(0)).toBe('0 B');
    expect(formatFileSize(1023)).toBe('1023 B');
    expect(formatFileSize(1024)).toBe('1.0 KiB');
    expect(formatFileSize(1024 * 1024 - 1)).toBe('1024.0 KiB');
    expect(formatFileSize(1024 * 1024)).toBe('1.0 MiB');
    expect(formatFileSize(1024 * 1024 * 1024 - 1)).toBe('1024.0 MiB');
    expect(formatFileSize(1024 * 1024 * 1024)).toBe('1.0 GiB');
  });
});
