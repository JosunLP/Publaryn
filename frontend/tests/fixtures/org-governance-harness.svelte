<script lang="ts">
  import type {
    OrgMember,
    OrgInvitation,
    OrganizationDetail,
  } from '../../src/api/orgs';
  import {
    createOrgGovernanceController,
    type OrgGovernanceMutations,
  } from '../../src/pages/org-governance';

  export let slug = 'source-org';
  export let loadState: (options?: {
    notice?: string | null;
    error?: string | null;
  }) => Promise<{
    org: OrganizationDetail;
    members: OrgMember[];
    invitations: OrgInvitation[];
  }>;
  export let mutations: OrgGovernanceMutations | undefined = undefined;

  let notice: string | null = null;
  let error: string | null = null;
  let org: OrganizationDetail | null = null;
  let members: OrgMember[] = [];
  let invitations: OrgInvitation[] = [];
  let ownershipTransferConfirmationOpen = false;
  let ownershipTransferConfirmed = false;
  let transferringOwnership = false;
  let invitationRevokeTargetId: string | null = null;
  let invitationRevokeConfirmed = false;
  let revokingInvitationId: string | null = null;
  let memberRemoveTargetUsername: string | null = null;
  let memberRemoveConfirmed = false;
  let removingMemberUsername: string | null = null;

  async function reload(
    options: {
      notice?: string | null;
      error?: string | null;
    } = {}
  ): Promise<void> {
    notice = options.notice ?? null;
    error = options.error ?? null;
    const state = await loadState(options);
    org = state.org;
    members = state.members;
    invitations = state.invitations;
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }

  function formatRole(role: string): string {
    return role
      .split('_')
      .filter(Boolean)
      .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
      .join(' ');
  }

  const controller = createOrgGovernanceController({
    getOrgSlug: () => slug,
    reload,
    toErrorMessage,
    formatRole,
    resolveOwnerUsername: (value) => value.trim(),
    clearFlash: () => {
      notice = null;
      error = null;
    },
    setError: (value) => {
      error = value;
    },
    setOwnershipTransferConfirmationOpen: (value) => {
      ownershipTransferConfirmationOpen = value;
    },
    getOwnershipTransferConfirmed: () => ownershipTransferConfirmed,
    setOwnershipTransferConfirmed: (value) => {
      ownershipTransferConfirmed = value;
    },
    setTransferringOwnership: (value) => {
      transferringOwnership = value;
    },
    setInvitationRevokeTargetId: (value) => {
      invitationRevokeTargetId = value;
    },
    getInvitationRevokeConfirmed: () => invitationRevokeConfirmed,
    setInvitationRevokeConfirmed: (value) => {
      invitationRevokeConfirmed = value;
    },
    setRevokingInvitationId: (value) => {
      revokingInvitationId = value;
    },
    setMemberRemoveTargetUsername: (value) => {
      memberRemoveTargetUsername = value;
    },
    getMemberRemoveConfirmed: () => memberRemoveConfirmed,
    setMemberRemoveConfirmed: (value) => {
      memberRemoveConfirmed = value;
    },
    setRemovingMemberUsername: (value) => {
      removingMemberUsername = value;
    },
    mutations,
  });

  queueMicrotask(() => {
    void reload();
  });
</script>

{#if notice}<div class="alert alert-success">{notice}</div>{/if}
{#if error}<div class="alert alert-error">{error}</div>{/if}

{#if org}
  <form id="org-profile-form" on:submit={(event) => controller.submitProfile(event)}>
    <input id="org-profile-name" name="name" value={org.name || ''} />
    <textarea id="org-profile-description" name="description">{org.description || ''}</textarea>
    <input id="org-profile-website" name="website" value={org.website || ''} />
    <input id="org-profile-email" name="email" value={org.email || ''} />
    <input
      id="org-profile-mfa-required"
      name="mfa_required"
      type="checkbox"
      checked={Boolean(org.mfa_required)}
    />
    <input
      id="org-profile-member-directory-private"
      name="member_directory_is_private"
      type="checkbox"
      checked={Boolean(org.member_directory_is_private)}
    />
    <button type="submit">Save profile</button>
  </form>

  <form id="org-invite-form" on:submit={(event) => controller.submitInvitation(event)}>
    <input id="org-invite-target" name="username_or_email" />
    <select id="org-invite-role" name="role">
      <option value="viewer">Viewer</option>
      <option value="publisher">Publisher</option>
      <option value="admin">Admin</option>
    </select>
    <input id="org-invite-expiry" name="expires_in_days" value="7" />
    <button type="submit">Send invitation</button>
  </form>

  <form id="org-member-form" on:submit={(event) => controller.submitMember(event)}>
    <input id="org-member-username" name="username" />
    <select id="org-member-role" name="role">
      <option value="viewer">Viewer</option>
      <option value="publisher">Publisher</option>
      <option value="admin">Admin</option>
    </select>
    <button type="submit">Add member</button>
  </form>

  <form
    id="org-ownership-transfer-form"
    on:submit={(event) => controller.submitOwnershipTransfer(event)}
  >
    <input id="org-transfer-owner" name="username" />
    {#if ownershipTransferConfirmationOpen}
      <input
        id="org-ownership-transfer-confirm"
        bind:checked={ownershipTransferConfirmed}
        type="checkbox"
      />
      <button id="org-ownership-transfer-submit" type="submit">
        {transferringOwnership ? 'Transferring…' : 'Transfer ownership'}
      </button>
      <button type="button" on:click={controller.cancelOwnershipTransferConfirmation}>
        Keep current owner
      </button>
    {:else}
      <button
        id="org-ownership-transfer-toggle"
        type="button"
        on:click={controller.openOwnershipTransferConfirmation}
      >
        Transfer ownership…
      </button>
    {/if}
  </form>

  <div data-test="invitations">
    {#each invitations as invitation}
      <div data-test={`invitation-${invitation.id || 'unknown'}`}>
        <span>{invitation.invited_user?.email || invitation.invited_user?.username}</span>
        {#if invitation.id}
          {#if invitationRevokeTargetId === invitation.id}
            <form
              id={`invitation-revoke-form-${invitation.id}`}
              on:submit={(event) =>
                controller.submitInvitationRevoke(event, invitation.id || '')}
            >
              <input
                id={`invitation-revoke-confirm-${invitation.id}`}
                bind:checked={invitationRevokeConfirmed}
                type="checkbox"
              />
              <button type="submit">
                {revokingInvitationId === invitation.id ? 'Revoking…' : 'Revoke invitation'}
              </button>
              <button type="button" on:click={controller.cancelInvitationRevokeConfirmation}>
                Keep invitation
              </button>
            </form>
          {:else}
            <button
              id={`invitation-revoke-toggle-${invitation.id}`}
              type="button"
              on:click={() =>
                controller.openInvitationRevokeConfirmation(invitation.id || '')}
            >
              Revoke…
            </button>
          {/if}
        {/if}
      </div>
    {/each}
  </div>

  <div data-test="members">
    {#each members as member}
      <div data-test={`member-${member.username || 'unknown'}`}>
        <span>{member.username}</span>
        <form
          id={`member-role-form-${member.username || 'member'}`}
          on:submit={(event) =>
            controller.updateMemberRole(
              event,
              member.username || '',
              member.role || 'viewer'
            )}
        >
          <select id={`member-role-${member.username || 'member'}`} name="role">
            <option value="viewer" selected={member.role === 'viewer'}>Viewer</option>
            <option value="publisher" selected={member.role === 'publisher'}>Publisher</option>
            <option value="admin" selected={member.role === 'admin'}>Admin</option>
          </select>
          <button type="submit">Save role</button>
        </form>
        {#if memberRemoveTargetUsername === member.username}
          <form
            id={`member-remove-form-${member.username || 'member'}`}
            on:submit={(event) =>
              controller.submitMemberRemoval(event, member.username || '')}
          >
            <input
              id={`member-remove-confirm-${member.username || 'member'}`}
              bind:checked={memberRemoveConfirmed}
              type="checkbox"
            />
            <button type="submit">
              {removingMemberUsername === member.username ? 'Removing…' : 'Remove member'}
            </button>
            <button type="button" on:click={controller.cancelMemberRemoveConfirmation}>
              Keep member
            </button>
          </form>
        {:else}
          <button
            id={`member-remove-toggle-${member.username || 'member'}`}
            type="button"
            on:click={() =>
              controller.openMemberRemoveConfirmation(member.username || '')}
          >
            Remove…
          </button>
        {/if}
      </div>
    {/each}
  </div>
{/if}
