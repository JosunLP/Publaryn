const UPPERCASE_ALLOWLIST = new Set(['OCI']);

export function titleCase(value: string): string {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((segment) => {
      const normalized = segment.toLowerCase();
      const uppercased = segment.toUpperCase();

      if (UPPERCASE_ALLOWLIST.has(uppercased)) {
        return uppercased;
      }

      return normalized.charAt(0).toUpperCase() + normalized.slice(1);
    })
    .join(' ');
}
