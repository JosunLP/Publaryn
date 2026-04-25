import type { BundleAnalysisSummary, BundleRiskSummary } from '../api/packages';
import { formatFileSize, formatNumber } from './format';
import { titleCase } from './strings';

export interface BundleAnalysisStat {
  label: string;
  value: string;
}

function metricValue(
  value: number | null | undefined,
  formatter: (value: number) => string = formatNumber
): string | null {
  return value != null ? formatter(value) : null;
}

export function buildBundleAnalysisStats(
  summary: BundleAnalysisSummary | null | undefined
): BundleAnalysisStat[] {
  if (!summary) {
    return [];
  }

  const stats: BundleAnalysisStat[] = [];
  const metricEntries: Array<
    [string, number | null | undefined, (value: number) => string]
  > = [
    ['Compressed size', summary.compressed_size_bytes, formatFileSize],
    ['Install size', summary.install_size_bytes, formatFileSize],
    ['Total artifact size', summary.total_artifact_size_bytes, formatFileSize],
    ['Files', summary.file_count, formatNumber],
    ['Direct deps', summary.direct_dependency_count, formatNumber],
    ['Runtime deps', summary.runtime_dependency_count, formatNumber],
    ['Dev deps', summary.development_dependency_count, formatNumber],
    ['Peer deps', summary.peer_dependency_count, formatNumber],
    ['Optional deps', summary.optional_dependency_count, formatNumber],
    ['Bundled deps', summary.bundled_dependency_count, formatNumber],
    ['Dependency groups', summary.dependency_group_count, formatNumber],
    ['Extras', summary.extra_count, formatNumber],
    ['Package types', summary.package_type_count, formatNumber],
    ['Layers', summary.layer_count, formatNumber],
    ['Install scripts', summary.install_script_count, formatNumber],
  ];

  for (const [label, value, formatter] of metricEntries) {
    const rendered = metricValue(value, formatter);
    if (rendered) {
      stats.push({ label, value: rendered });
    }
  }

  if (summary.artifact_count != null) {
    stats.unshift({
      label: 'Artifacts',
      value: formatNumber(summary.artifact_count),
    });
  }

  return stats;
}

export function buildBundleAnalysisHighlights(
  summary: BundleAnalysisSummary | null | undefined
): string[] {
  if (!summary) {
    return [];
  }

  const highlights: string[] = [];

  if ((summary.install_script_count ?? 0) > 0) {
    highlights.push(
      `${formatNumber(summary.install_script_count)} install script${summary.install_script_count === 1 ? '' : 's'}`
    );
  }
  if (summary.has_cli_entrypoints) {
    highlights.push('CLI entrypoints');
  }
  if (summary.has_tree_shaking_hints) {
    highlights.push('Tree-shaking hints');
  }
  if (summary.has_native_code) {
    highlights.push('Native build hints');
  }

  return highlights;
}

export function buildBundleAnalysisQuickFacts(
  summary: BundleAnalysisSummary | null | undefined
): string[] {
  if (!summary) {
    return [];
  }

  const facts = [
    summary.compressed_size_bytes != null
      ? `compressed ${formatFileSize(summary.compressed_size_bytes)}`
      : null,
    summary.install_size_bytes != null
      ? `install ${formatFileSize(summary.install_size_bytes)}`
      : null,
    summary.direct_dependency_count != null
      ? `${formatNumber(summary.direct_dependency_count)} direct dep${summary.direct_dependency_count === 1 ? '' : 's'}`
      : null,
    summary.layer_count != null
      ? `${formatNumber(summary.layer_count)} layer${summary.layer_count === 1 ? '' : 's'}`
      : null,
  ].filter((fact): fact is string => Boolean(fact));

  return facts.slice(0, 3);
}

export function bundleAnalysisNotes(
  summary: BundleAnalysisSummary | null | undefined
): string[] {
  return (summary?.notes || []).filter(
    (note): note is string => typeof note === 'string' && note.trim().length > 0
  );
}

export function bundleAnalysisRisk(
  summary: BundleAnalysisSummary | null | undefined
): BundleRiskSummary | null {
  return summary?.risk || null;
}

export function bundleAnalysisRiskBadgeSeverity(
  summary: BundleAnalysisSummary | null | undefined
): 'critical' | 'high' | 'medium' | 'low' | 'info' {
  switch ((bundleAnalysisRisk(summary)?.level || '').toLowerCase()) {
    case 'critical':
      return 'critical';
    case 'high':
      return 'high';
    case 'moderate':
      return 'medium';
    case 'low':
      return 'low';
    default:
      return 'info';
  }
}

export function bundleAnalysisRiskLabel(
  summary: BundleAnalysisSummary | null | undefined
): string {
  const level = bundleAnalysisRisk(summary)?.level;
  return level ? `${titleCase(level)} risk` : 'Risk pending';
}

export function bundleAnalysisRiskScoreLabel(
  summary: BundleAnalysisSummary | null | undefined
): string | null {
  const score = bundleAnalysisRisk(summary)?.score;
  return typeof score === 'number' && Number.isFinite(score)
    ? `${formatNumber(score)} / 100`
    : null;
}

export function bundleAnalysisRiskSeverityLabel(
  summary: BundleAnalysisSummary | null | undefined
): string | null {
  const severity = bundleAnalysisRisk(summary)?.worst_unresolved_severity;
  return severity ? titleCase(severity) : null;
}

export function bundleAnalysisRiskFactors(
  summary: BundleAnalysisSummary | null | undefined
): string[] {
  return (bundleAnalysisRisk(summary)?.factors || []).filter(
    (factor): factor is string =>
      typeof factor === 'string' && factor.trim().length > 0
  );
}
