import type { OrgMember } from '../api/orgs';

export interface OrgAuditActorOption {
  userId: string;
  username: string;
  label: string;
}

export interface OrgAuditActorInputState {
  syncKey: string;
  input: string;
}

export function buildAuditActorOptions(
  members: OrgMember[],
  remoteOptions: OrgAuditActorOption[]
): OrgAuditActorOption[] {
  return dedupeAuditActorOptions([
    ...buildRemoteAuditActorOptions(members),
    ...remoteOptions,
  ]);
}

export function buildRemoteAuditActorOptions(
  members: OrgMember[] | null | undefined
): OrgAuditActorOption[] {
  return (members || [])
    .map(toAuditActorOption)
    .filter((option): option is OrgAuditActorOption => option !== null)
    .sort((left, right) => left.username.localeCompare(right.username));
}

export function dedupeAuditActorOptions(
  options: OrgAuditActorOption[]
): OrgAuditActorOption[] {
  const seen = new Set<string>();
  const merged: OrgAuditActorOption[] = [];

  for (const option of options) {
    if (seen.has(option.userId)) {
      continue;
    }
    seen.add(option.userId);
    merged.push(option);
  }

  return merged;
}

export function nextAuditActorInputState(
  currentSyncKey: string,
  currentInput: string,
  actorUserId: string,
  actorUsername: string
): OrgAuditActorInputState {
  const syncKey = `${actorUserId}|${actorUsername}`;
  if (syncKey === currentSyncKey) {
    return {
      syncKey: currentSyncKey,
      input: currentInput,
    };
  }

  return {
    syncKey,
    input: actorUsername || actorUserId || '',
  };
}

function toAuditActorOption(member: OrgMember): OrgAuditActorOption | null {
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
