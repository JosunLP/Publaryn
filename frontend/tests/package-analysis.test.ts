import { describe, expect, test } from 'bun:test';

import { buildBundleAnalysisStats } from '../src/utils/package-analysis';

describe('package analysis helpers', () => {
  test('labels artifact size separately from artifact count', () => {
    expect(
      buildBundleAnalysisStats({
        artifact_count: 3,
        total_artifact_size_bytes: 2048,
      })
    ).toEqual([
      { label: 'Artifacts', value: '3' },
      { label: 'Total artifact size', value: '2.0 KiB' },
    ]);
  });

  test('omits artifact count when the backend does not provide it', () => {
    expect(
      buildBundleAnalysisStats({
        total_artifact_size_bytes: 2048,
      })
    ).toEqual([{ label: 'Total artifact size', value: '2.0 KiB' }]);
  });
});
