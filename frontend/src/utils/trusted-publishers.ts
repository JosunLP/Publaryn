type NullableString = string | null | undefined;

export interface TrustedPublisherFormInput {
  issuer: NullableString;
  subject: NullableString;
  repository?: NullableString;
  workflowRef?: NullableString;
  environment?: NullableString;
}

export interface NormalizedTrustedPublisherInput {
  issuer: string;
  subject: string;
  repository?: string;
  workflowRef?: string;
  environment?: string;
}

export interface TrustedPublisherLike {
  issuer?: NullableString;
  subject?: NullableString;
  repository?: NullableString;
  workflow_ref?: NullableString;
  environment?: NullableString;
}

export interface TrustedPublisherBindingField {
  label: 'Repository' | 'Workflow' | 'Environment';
  value: string;
}

export function normalizeTrustedPublisherInput(
  input: TrustedPublisherFormInput
): NormalizedTrustedPublisherInput {
  return {
    issuer: normalizeRequired(input.issuer, 'Issuer'),
    subject: normalizeRequired(input.subject, 'Subject'),
    repository: normalizeOptional(input.repository),
    workflowRef: normalizeOptional(input.workflowRef),
    environment: normalizeOptional(input.environment),
  };
}

export function trustedPublisherHeading(
  publisher: TrustedPublisherLike
): string {
  return (
    normalizeOptional(publisher.repository) ||
    normalizeOptional(publisher.subject) ||
    normalizeOptional(publisher.issuer) ||
    'Trusted publisher'
  );
}

export function trustedPublisherBindingFields(
  publisher: TrustedPublisherLike
): TrustedPublisherBindingField[] {
  const fields: TrustedPublisherBindingField[] = [];

  const repository = normalizeOptional(publisher.repository);
  const workflowRef = normalizeOptional(publisher.workflow_ref);
  const environment = normalizeOptional(publisher.environment);

  if (repository) {
    fields.push({ label: 'Repository', value: repository });
  }
  if (workflowRef) {
    fields.push({ label: 'Workflow', value: workflowRef });
  }
  if (environment) {
    fields.push({ label: 'Environment', value: environment });
  }

  return fields;
}

function normalizeRequired(value: NullableString, label: string): string {
  const normalized = normalizeOptional(value);
  if (!normalized) {
    throw new Error(`${label} is required.`);
  }

  return normalized;
}

function normalizeOptional(value: NullableString): string | undefined {
  if (typeof value !== 'string') {
    return undefined;
  }

  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}
