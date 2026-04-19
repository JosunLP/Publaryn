import type { OrgMember } from '../api/orgs';

export interface OrgMemberPickerOption {
  userId: string;
  username: string;
  label: string;
}

export function buildOrgMemberPickerOptions(
  members: OrgMember[] | null | undefined,
  excludedUsernames: string[] = []
): OrgMemberPickerOption[] {
  const excluded = new Set(
    excludedUsernames
      .map((username) => username.trim().toLowerCase())
      .filter(Boolean)
  );
  const seen = new Set<string>();
  const options: OrgMemberPickerOption[] = [];

  for (const member of members || []) {
    const option = toOrgMemberPickerOption(member);
    if (!option) {
      continue;
    }

    if (
      excluded.has(option.username.toLowerCase()) ||
      seen.has(option.userId) ||
      seen.has(option.username.toLowerCase())
    ) {
      continue;
    }

    seen.add(option.userId);
    seen.add(option.username.toLowerCase());
    options.push(option);
  }

  return options.sort((left, right) => left.username.localeCompare(right.username));
}

export function resolveOrgMemberPickerInput(
  input: string,
  options: OrgMemberPickerOption[]
): string {
  const trimmed = input.trim();
  if (!trimmed) {
    return '';
  }

  const normalized = trimmed.toLowerCase();
  const matched = options.find(
    (option) =>
      option.username.toLowerCase() === normalized ||
      option.userId.toLowerCase() === normalized
  );

  return matched?.username || trimmed;
}

function toOrgMemberPickerOption(
  member: OrgMember
): OrgMemberPickerOption | null {
  if (
    typeof member.user_id !== 'string' ||
    !member.user_id.trim() ||
    typeof member.username !== 'string' ||
    !member.username.trim()
  ) {
    return null;
  }

  const userId = member.user_id.trim();
  const username = member.username.trim();
  const displayName = member.display_name?.trim();

  return {
    userId,
    username,
    label: displayName ? `${displayName} (@${username})` : `@${username}`,
  };
}
