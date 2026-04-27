import { describe, expect, test } from 'bun:test';

import {
  buildBundleAnalysisHighlights,
  buildBundleAnalysisStats,
  bundleAnalysisRiskScoreLabel,
  bundleAnalysisRiskBadgeSeverity,
  bundleAnalysisRiskLabel,
  bundleAnalysisRiskSeverityLabel,
} from '../src/utils/package-analysis';

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

  test('reuses shared risk formatting for bundle analysis summaries', () => {
    expect(
      bundleAnalysisRiskLabel({
        risk: {
          level: 'moderate',
        },
      })
    ).toBe('Medium risk');
    expect(
      bundleAnalysisRiskBadgeSeverity({
        risk: {
          level: 'moderate',
        },
      })
    ).toBe('medium');
    expect(bundleAnalysisRiskLabel({ risk: null })).toBe('Risk pending');
    expect(bundleAnalysisRiskBadgeSeverity({ risk: null })).toBe('info');
  });

  test('pluralizes install script highlights and includes bundle flags', () => {
    expect(
      buildBundleAnalysisHighlights({
        install_script_count: 2,
        has_cli_entrypoints: true,
        has_tree_shaking_hints: true,
        has_native_code: false,
      })
    ).toEqual([
      '2 install scripts',
      'CLI entrypoints',
      'Tree-shaking hints',
    ]);
    expect(
      buildBundleAnalysisHighlights({
        install_script_count: 1,
        has_cli_entrypoints: false,
        has_tree_shaking_hints: false,
        has_native_code: true,
      })
    ).toEqual(['1 install script', 'Native build hints']);
  });

  test('formats bundle risk score and worst severity labels', () => {
    expect(
      bundleAnalysisRiskScoreLabel({
        risk: {
          score: 42,
        },
      })
    ).toBe('42 / 100');
    expect(
      bundleAnalysisRiskSeverityLabel({
        risk: {
          worst_unresolved_severity: 'critical',
        },
      })
    ).toBe('Critical');
    expect(bundleAnalysisRiskScoreLabel({ risk: { score: null } })).toBeNull();
    expect(
      bundleAnalysisRiskSeverityLabel({
        risk: {
          worst_unresolved_severity: null,
        },
      })
    ).toBeNull();
  });
});
