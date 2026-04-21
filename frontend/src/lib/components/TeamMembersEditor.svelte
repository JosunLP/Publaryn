<script lang="ts">
  import type { TeamMember } from '../../api/orgs';
  import type { OrgMemberPickerOption } from '../../pages/org-member-picker';
  import { formatDate } from '../../utils/format';

  export let members: TeamMember[] = [];
  export let membersError: string | null = null;
  export let eligibleOptions: OrgMemberPickerOption[] = [];
  export let eligibleOptionsError: string | null = null;
  export let emptyMembersMessage = 'No members belong to this team yet.';
  export let emptyEligibleMessage =
    'Every current organization member is already part of this team.';
  export let formId = 'team-member-form';
  export let inputId = 'team-member-input';
  export let datalistId = 'team-member-options';
  export let submitLabel = 'Add member';
  export let submitClass = 'btn btn-primary';
  export let removeButtonIdPrefix = 'team-member-remove-';
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
  export let handleRemoveMember: (username: string) => void | Promise<void> = () => {};
</script>

{#if membersError}
  <div class="alert alert-error">{membersError}</div>
{:else if members.length === 0}
  <p class="settings-copy">{emptyMembersMessage}</p>
{:else}
  <div class="token-list">
    {#each members as member}
      <div class="token-row">
        <div class="token-row__main">
          <div class="token-row__title">
            {member.display_name || member.username || 'Unknown member'}
          </div>
          <div class="token-row__meta">
            <span>@{member.username || 'unknown'}</span>
            <span>Added {formatDate(member.added_at)}</span>
          </div>
        </div>
        {#if member.username}
          <div class="token-row__actions">
            <button
              id={`${removeButtonIdPrefix}${encodeURIComponent(member.username)}`}
              class="btn btn-secondary btn-sm"
              type="button"
              on:click={() => handleRemoveMember(member.username || '')}>Remove</button
            >
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

{#if eligibleOptionsError}
  <div class="alert alert-error mt-4">{eligibleOptionsError}</div>
{:else if eligibleOptions.length === 0}
  <p class="settings-copy mt-4">{emptyEligibleMessage}</p>
{:else}
  <form id={formId} class="settings-subsection" on:submit={handleSubmit}>
    <div class="form-group">
      <label for={inputId}>Add organization member</label>
      <input
        id={inputId}
        name="username"
        class="form-input"
        list={datalistId}
        placeholder="Search username or paste user id"
        autocomplete="off"
        required
      />
      <datalist id={datalistId}>
        {#each eligibleOptions as option}
          <option value={option.username}>{option.label}</option>
          <option value={option.userId}>{option.label}</option>
        {/each}
      </datalist>
    </div>
    <button type="submit" class={submitClass}>{submitLabel}</button>
  </form>
{/if}
