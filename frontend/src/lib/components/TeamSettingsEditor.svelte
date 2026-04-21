<script lang="ts">
  import type { Team } from '../../api/orgs';

  export let team: Team;
  export let teamSlug: string;
  export let formId = '';
  export let formClass = '';
  export let nameFieldId = 'team-name';
  export let slugFieldId = 'team-slug';
  export let descriptionFieldId = 'team-description';
  export let showSlugField = false;
  export let submitLabel = 'Save team details';
  export let submitClass = 'btn btn-primary';
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
</script>

<form id={formId || undefined} class={formClass} on:submit={handleSubmit}>
  <div class="grid gap-4 xl:grid-cols-2">
    <div class="form-group">
      <label for={nameFieldId}>Team name</label>
      <input
        id={nameFieldId}
        name="name"
        class="form-input"
        value={team.name || ''}
        required
      />
    </div>
    {#if showSlugField}
      <div class="form-group">
        <label for={slugFieldId}>Team slug</label>
        <input
          id={slugFieldId}
          class="form-input"
          value={team.slug || teamSlug}
          disabled
        />
      </div>
    {/if}
  </div>
  <div class="form-group">
    <label for={descriptionFieldId}>Description</label>
    <textarea
      id={descriptionFieldId}
      name="description"
      class="form-input"
      rows="3">{team.description || ''}</textarea
    >
  </div>
  <button type="submit" class={submitClass}>{submitLabel}</button>
</form>
