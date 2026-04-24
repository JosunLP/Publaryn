import type { AddMemberInput, SendInvitationInput, TransferOwnershipResult, UpdateOrgInput } from '../api/orgs';
import {
  addMember,
  removeMember,
  revokeInvitation,
  sendInvitation,
  transferOwnership,
  updateOrg,
} from '../api/orgs';

export const ORG_OWNERSHIP_TRANSFER_CONFIRMATION_MESSAGE =
  'Please confirm the ownership transfer.';
export const ORG_INVITATION_REVOKE_CONFIRMATION_MESSAGE =
  'Please confirm that you want to revoke this invitation immediately.';
export const ORG_MEMBER_REMOVE_CONFIRMATION_MESSAGE =
  'Please confirm that you want to remove this member from the organization.';

export interface OrgGovernanceReloadOptions {
  notice?: string | null;
  error?: string | null;
}

export interface OrgGovernanceMutations {
  updateOrg: typeof updateOrg;
  sendInvitation: typeof sendInvitation;
  addMember: typeof addMember;
  transferOwnership: typeof transferOwnership;
  revokeInvitation: typeof revokeInvitation;
  removeMember: (slug: string, username: string) => Promise<void>;
}

export interface OrgGovernanceControllerOptions {
  getOrgSlug: () => string;
  reload: (options?: OrgGovernanceReloadOptions) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  formatRole: (role: string) => string;
  resolveOwnerUsername: (value: string) => string;
  clearFlash: () => void;
  setError: (value: string | null) => void;
  setOwnershipTransferConfirmationOpen: (value: boolean) => void;
  getOwnershipTransferConfirmed: () => boolean;
  setOwnershipTransferConfirmed: (value: boolean) => void;
  setTransferringOwnership: (value: boolean) => void;
  setInvitationRevokeTargetId: (value: string | null) => void;
  getInvitationRevokeConfirmed: () => boolean;
  setInvitationRevokeConfirmed: (value: boolean) => void;
  setRevokingInvitationId: (value: string | null) => void;
  setMemberRemoveTargetUsername: (value: string | null) => void;
  getMemberRemoveConfirmed: () => boolean;
  setMemberRemoveConfirmed: (value: boolean) => void;
  setRemovingMemberUsername: (value: string | null) => void;
  mutations?: OrgGovernanceMutations;
}

const DEFAULT_ORG_GOVERNANCE_MUTATIONS: OrgGovernanceMutations = {
  updateOrg,
  sendInvitation,
  addMember,
  transferOwnership,
  revokeInvitation,
  removeMember,
};

export function createOrgGovernanceController(options: OrgGovernanceControllerOptions) {
  const mutations = options.mutations || DEFAULT_ORG_GOVERNANCE_MUTATIONS;

  return {
    async submitProfile(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const name = normalizeRequiredFormText(formData.get('name'));

      if (!name) {
        options.clearFlash();
        options.setError('Organization name is required.');
        return;
      }

      try {
        await mutations.updateOrg(options.getOrgSlug(), {
          name,
          description: normalizeOptionalFormText(formData.get('description')),
          website: normalizeOptionalFormText(formData.get('website')),
          email: normalizeOptionalFormText(formData.get('email')),
          mfaRequired: formData.has('mfa_required'),
          memberDirectoryIsPrivate: formData.has('member_directory_is_private'),
        } satisfies UpdateOrgInput);
        await options.reload({ notice: 'Organization profile updated.' });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to update organization profile.'
          ),
        });
      }
    },

    async submitInvitation(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const form = event.currentTarget as HTMLFormElement;
      const formData = new FormData(form);

      try {
        await mutations.sendInvitation(options.getOrgSlug(), {
          usernameOrEmail:
            formData.get('username_or_email')?.toString().trim() || '',
          role: formData.get('role')?.toString() || 'viewer',
          expiresInDays:
            Number(formData.get('expires_in_days')?.toString() || '7') || 7,
        } satisfies SendInvitationInput);
        form.reset();
        await options.reload({ notice: 'Invitation sent successfully.' });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to send invitation.'),
        });
      }
    },

    async submitMember(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const form = event.currentTarget as HTMLFormElement;
      const formData = new FormData(form);

      try {
        await mutations.addMember(options.getOrgSlug(), {
          username: formData.get('username')?.toString().trim() || '',
          role: formData.get('role')?.toString() || 'viewer',
        } satisfies AddMemberInput);
        form.reset();
        await options.reload({ notice: 'Member added successfully.' });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to add member.'),
        });
      }
    },

    async updateMemberRole(
      event: SubmitEvent,
      username: string,
      currentRole: string
    ): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const role = formData.get('role')?.toString().trim() || 'viewer';

      if (role === currentRole) {
        await options.reload({
          notice: `@${username} already has the ${options.formatRole(role)} role.`,
        });
        return;
      }

      try {
        await mutations.addMember(options.getOrgSlug(), { username, role });
        await options.reload({
          notice: `Updated @${username} to ${options.formatRole(role)}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to update member role.'),
        });
      }
    },

    openOwnershipTransferConfirmation(): void {
      options.setOwnershipTransferConfirmationOpen(true);
      options.setOwnershipTransferConfirmed(false);
      options.setTransferringOwnership(false);
      options.clearFlash();
    },

    cancelOwnershipTransferConfirmation(): void {
      options.setOwnershipTransferConfirmationOpen(false);
      options.setOwnershipTransferConfirmed(false);
      options.setTransferringOwnership(false);
      options.setError(null);
    },

    async submitOwnershipTransfer(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const username = options.resolveOwnerUsername(
        formData.get('username')?.toString() || ''
      );

      if (!options.getOwnershipTransferConfirmed()) {
        options.clearFlash();
        options.setError(ORG_OWNERSHIP_TRANSFER_CONFIRMATION_MESSAGE);
        return;
      }

      options.setTransferringOwnership(true);
      options.clearFlash();

      try {
        const result: TransferOwnershipResult = await mutations.transferOwnership(
          options.getOrgSlug(),
          { username }
        );
        await options.reload({
          notice: `Ownership transferred to @${result.new_owner?.username || 'the selected user'}.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(
            caughtError,
            'Failed to transfer organization ownership.'
          )
        );
        options.setTransferringOwnership(false);
      }
    },

    openInvitationRevokeConfirmation(invitationId: string): void {
      options.setInvitationRevokeTargetId(invitationId);
      options.setInvitationRevokeConfirmed(false);
      options.setRevokingInvitationId(null);
      options.clearFlash();
    },

    cancelInvitationRevokeConfirmation(): void {
      options.setInvitationRevokeTargetId(null);
      options.setInvitationRevokeConfirmed(false);
      options.setRevokingInvitationId(null);
      options.setError(null);
    },

    async submitInvitationRevoke(
      event: SubmitEvent,
      invitationId: string
    ): Promise<void> {
      event.preventDefault();

      if (!options.getInvitationRevokeConfirmed()) {
        options.clearFlash();
        options.setError(ORG_INVITATION_REVOKE_CONFIRMATION_MESSAGE);
        return;
      }

      options.setRevokingInvitationId(invitationId);
      options.clearFlash();

      try {
        await mutations.revokeInvitation(options.getOrgSlug(), invitationId);
        await options.reload({ notice: 'Invitation revoked.' });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(caughtError, 'Failed to revoke invitation.')
        );
        options.setRevokingInvitationId(null);
      }
    },

    openMemberRemoveConfirmation(username: string): void {
      options.setMemberRemoveTargetUsername(username);
      options.setMemberRemoveConfirmed(false);
      options.setRemovingMemberUsername(null);
      options.clearFlash();
    },

    cancelMemberRemoveConfirmation(): void {
      options.setMemberRemoveTargetUsername(null);
      options.setMemberRemoveConfirmed(false);
      options.setRemovingMemberUsername(null);
      options.setError(null);
    },

    async submitMemberRemoval(
      event: SubmitEvent,
      username: string
    ): Promise<void> {
      event.preventDefault();

      if (!options.getMemberRemoveConfirmed()) {
        options.clearFlash();
        options.setError(ORG_MEMBER_REMOVE_CONFIRMATION_MESSAGE);
        return;
      }

      options.setRemovingMemberUsername(username);
      options.clearFlash();

      try {
        await mutations.removeMember(options.getOrgSlug(), username);
        await options.reload({
          notice: `Removed @${username} from the organization.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(caughtError, 'Failed to remove member.')
        );
        options.setRemovingMemberUsername(null);
      }
    },
  };
}

/**
 * Normalize optional form text values by trimming strings and collapsing
 * empty or non-string entries to null before sending them to the API.
 */
function normalizeOptionalFormText(
  value: FormDataEntryValue | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeRequiredFormText(
  value: FormDataEntryValue | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  return value.trim();
}
