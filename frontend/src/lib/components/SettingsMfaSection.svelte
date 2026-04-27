<script lang="ts">
  import type { MfaSetupState, UserProfile } from '../../api/auth';
  import { copyToClipboard } from '../../utils/format';

  export let user: UserProfile;
  export let mfaSetupState: MfaSetupState | null = null;
  export let mfaDisableCode = '';
  export let disablingMfa = false;
  export let startingMfaSetup = false;
  export let mfaVerifyCode = '';
  export let verifyingMfa = false;
  export let handleStartMfaSetup: () => void | Promise<void>;
  export let handleVerifyMfa: (event: SubmitEvent) => void | Promise<void>;
  export let handleDisableMfa: (event: SubmitEvent) => void | Promise<void>;
</script>

<section class="card settings-section">
  <h2>Multi-factor authentication</h2>
  <p class="text-muted settings-copy">
    Status: <strong>{user.mfa_enabled ? 'Enabled' : 'Disabled'}</strong>
  </p>

  {#if user.mfa_enabled}
    <form id="mfa-disable-form" on:submit={handleDisableMfa}>
      <div class="form-group">
        <label for="mfa-disable-code">Authenticator or recovery code</label>
        <input
          id="mfa-disable-code"
          bind:value={mfaDisableCode}
          class="form-input"
          placeholder="123456 or xxxx-yyyy"
          required
        />
      </div>
      <button
        type="submit"
        class="btn btn-danger"
        disabled={disablingMfa}
      >
        {disablingMfa ? 'Disabling…' : 'Disable MFA'}
      </button>
    </form>
  {:else}
    <button
      id="mfa-setup-btn"
      class="btn btn-primary"
      type="button"
      on:click={handleStartMfaSetup}
      disabled={startingMfaSetup}
    >
      {startingMfaSetup ? 'Preparing…' : 'Start MFA setup'}
    </button>
    <p class="text-muted mt-4">
      Use an authenticator app like 1Password, Bitwarden, Authy, or Google
      Authenticator.
    </p>
  {/if}

  {#if mfaSetupState}
    <div class="settings-subsection">
      <h3>Step 1: Add the secret to your authenticator app</h3>
      <div class="code-block">
        <button
          class="copy-btn"
          type="button"
          on:click={() => copyToClipboard(mfaSetupState?.secret || '')}>Copy</button
        ><code>{mfaSetupState.secret}</code>
      </div>
      <div class="mt-4">
        <div class="text-muted" style="display:block; margin-bottom:6px;">
          Provisioning URI
        </div>
        <div class="code-block">
          <button
            class="copy-btn"
            type="button"
            on:click={() => copyToClipboard(mfaSetupState?.provisioning_uri || '')}
            >Copy</button
          ><code>{mfaSetupState.provisioning_uri}</code>
        </div>
      </div>
      <div class="mt-4">
        <div class="text-muted" style="display:block; margin-bottom:6px;">
          Recovery codes (store these somewhere safe)
        </div>
        <div class="code-block">
          <button
            class="copy-btn"
            type="button"
            on:click={() =>
              copyToClipboard(mfaSetupState?.recovery_codes.join('\n') || '')}
            >Copy</button
          ><code>{mfaSetupState.recovery_codes.join('\n')}</code>
        </div>
      </div>
      <form id="mfa-verify-form" class="mt-4" on:submit={handleVerifyMfa}>
        <div class="form-group">
          <label for="mfa-verify-code"
            >Step 2: Enter a code from your authenticator app</label
          >
          <input
            id="mfa-verify-code"
            bind:value={mfaVerifyCode}
            class="form-input"
            placeholder="123456"
            required
          />
        </div>
        <button
          type="submit"
          class="btn btn-primary"
          disabled={verifyingMfa}
        >
          {verifyingMfa ? 'Enabling…' : 'Enable MFA'}
        </button>
      </form>
    </div>
  {/if}
</section>
