<script lang="ts">
  import type { TokenRecord } from '../../api/tokens';
  import { copyToClipboard, formatDate } from '../../utils/format';

  export let createdToken: string | null = null;
  export let tokenName = '';
  export let tokenExpiryDays = '';
  export let selectedScopes = new Set<string>();
  export let tokenScopeOptions: readonly string[] = [];
  export let creatingToken = false;
  export let tokens: TokenRecord[] = [];
  export let handleScopeToggle: (scope: string, checked: boolean) => void;
  export let handleTokenSubmit: (event: SubmitEvent) => void | Promise<void>;
  export let handleRevokeToken: (tokenId: string) => void | Promise<void>;
</script>

<section class="card settings-section mt-6">
  <div class="settings-token-header">
    <div>
      <h2>API tokens</h2>
      <p class="text-muted settings-copy">
        Create personal automation tokens and revoke old ones.
      </p>
    </div>
  </div>

  {#if createdToken}
    <div class="alert alert-success">
      <div style="margin-bottom:8px;">
        <strong>New token created.</strong> Copy it now — it will not be shown
        again.
      </div>
      <div class="code-block">
        <button
          class="copy-btn"
          type="button"
          on:click={() => copyToClipboard(createdToken || '')}>Copy</button
        ><code>{createdToken}</code>
      </div>
    </div>
  {/if}

  <form
    id="token-form"
    class="settings-subsection"
    on:submit={handleTokenSubmit}
  >
    <div class="form-group">
      <label for="token-name">Token name</label>
      <input
        id="token-name"
        bind:value={tokenName}
        class="form-input"
        placeholder="CI / local development / deploy"
        required
      />
    </div>
    <div class="form-group">
      <label for="token-expiry">Expires in days (optional)</label>
      <input
        id="token-expiry"
        bind:value={tokenExpiryDays}
        type="number"
        min="1"
        class="form-input"
        placeholder="30"
      />
    </div>
    <div class="form-group">
      <div class="text-sm font-medium">Scopes</div>
      <div class="settings-scope-grid">
        {#each tokenScopeOptions as scope}
          <label class="settings-checkbox">
            <input
              type="checkbox"
              checked={selectedScopes.has(scope)}
              on:change={(event) =>
                handleScopeToggle(
                  scope,
                  (event.currentTarget as HTMLInputElement).checked
                )}
            />
            <span>{scope}</span>
          </label>
        {/each}
      </div>
    </div>
    <button type="submit" class="btn btn-primary" disabled={creatingToken}>
      {creatingToken ? 'Creating…' : 'Create token'}
    </button>
  </form>

  <div class="settings-subsection">
    <h3>Active tokens</h3>
    {#if tokens.length === 0}
      <div class="empty-state">
        <h3>No tokens yet</h3>
        <p>Create one above for CI, publishing, or local automation.</p>
      </div>
    {:else}
      <div class="token-list">
        {#each tokens as token}
          <div class="token-row">
            <div class="token-row__main">
              <div class="token-row__title">
                {token.name || 'Unnamed token'}
              </div>
              <div class="token-row__meta">
                <span>{token.kind || 'personal'}</span>
                <span>created {formatDate(token.created_at)}</span>
                {#if token.last_used_at}<span
                    >last used {formatDate(token.last_used_at)}</span
                  >{:else}<span>never used</span>{/if}
                {#if token.expires_at}<span
                    >expires {formatDate(token.expires_at)}</span
                  >{:else}<span>no expiry</span>{/if}
              </div>
              <div class="token-row__scopes">
                {#each token.scopes || [] as scope}
                  <span class="badge badge-ecosystem">{scope}</span>
                {/each}
              </div>
            </div>
            <div class="token-row__actions">
              <button
                class="btn btn-secondary btn-sm"
                type="button"
                on:click={() => copyToClipboard(token.prefix || 'pub_')}
                >Copy prefix</button
              >
              {#if token.id}<button
                  class="btn btn-danger btn-sm"
                  type="button"
                  on:click={() => handleRevokeToken(token.id || '')}
                  >Revoke</button
                >{/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>
