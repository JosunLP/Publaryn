const KB = 1024;
const MB = KB * 1024;
const GB = MB * 1024;

function formatBinaryUnit(bytes: number, unitSize: number, suffix: string): string {
  const truncated = Math.floor((bytes / unitSize) * 10) / 10;
  return `${truncated.toFixed(1)} ${suffix}`;
}

/**
 * Format a number with locale-aware separators.
 */
export function formatNumber(
  value: number | string | null | undefined
): string {
  if (value == null) {
    return '0';
  }

  return Number(value).toLocaleString('en-US');
}

/**
 * Format byte counts using human-readable units.
 */
export function formatFileSize(
  value: number | string | null | undefined
): string {
  const bytes = Number(value ?? 0);
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 B';
  }

  if (bytes < KB) {
    return `${bytes.toFixed(0)} B`;
  }
  if (bytes < MB) {
    return formatBinaryUnit(bytes, KB, 'KiB');
  }
  if (bytes < GB) {
    return formatBinaryUnit(bytes, MB, 'MiB');
  }

  return formatBinaryUnit(bytes, GB, 'GiB');
}

/**
 * Format an ISO date string as a relative or absolute date.
 */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) {
    return '';
  }

  const date = new Date(iso);
  const now = Date.now();
  const diff = now - date.getTime();
  const seconds = Math.floor(diff / 1000);

  if (seconds < 60) {
    return 'just now';
  }

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }

  const days = Math.floor(hours / 24);
  if (days < 30) {
    return `${days}d ago`;
  }

  return date.toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

/**
 * Escape HTML special characters to prevent injection.
 */
export function escapeHtml(value: string | number | null | undefined): string {
  const text = String(value ?? '');

  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

/**
 * Copy text to clipboard.
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
