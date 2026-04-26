export function normalizeOptionalFormText(
  value: FormDataEntryValue | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function normalizeRequiredFormText(
  value: FormDataEntryValue | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  return value.trim();
}
