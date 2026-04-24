<script lang="ts">
  import SettingsMfaSection from '../../src/lib/components/SettingsMfaSection.svelte';
  import { createSettingsMfaController } from '../../src/pages/settings-mfa';
  import type {
    MfaSetupState,
    UserProfile,
  } from '../../src/api/auth';
  import type { SettingsMfaActions } from '../../src/pages/settings-mfa';

  export let user: UserProfile;
  export let actions: SettingsMfaActions;
  export let initialMfaSetupState: MfaSetupState | null = null;

  let notice: string | null = null;
  let error: string | null = null;
  let mfaSetupState = initialMfaSetupState;
  let mfaDisableCode = '';
  let disablingMfa = false;
  let startingMfaSetup = false;
  let mfaVerifyCode = '';
  let verifyingMfa = false;

  async function loadSettings(
    options: {
      notice?: string | null;
      error?: string | null;
      mfaSetupState?: MfaSetupState | null;
    } = {}
  ): Promise<void> {
    notice = options.notice ?? null;
    error = options.error ?? null;
    mfaSetupState = options.mfaSetupState ?? null;

    if (options.notice === 'MFA enabled successfully.') {
      user = { ...user, mfa_enabled: true };
    } else if (options.notice === 'MFA disabled.') {
      user = { ...user, mfa_enabled: false };
    }
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }

  const controller = createSettingsMfaController({
    loadSettings,
    toErrorMessage,
    getMfaSetupState: () => mfaSetupState,
    getMfaVerifyCode: () => mfaVerifyCode,
    setMfaVerifyCode: (value) => {
      mfaVerifyCode = value;
    },
    getMfaDisableCode: () => mfaDisableCode,
    setMfaDisableCode: (value) => {
      mfaDisableCode = value;
    },
    setStartingMfaSetup: (value) => {
      startingMfaSetup = value;
    },
    setVerifyingMfa: (value) => {
      verifyingMfa = value;
    },
    setDisablingMfa: (value) => {
      disablingMfa = value;
    },
    actions,
  });
</script>

{#if notice}<div class="alert alert-success">{notice}</div>{/if}
{#if error}<div class="alert alert-error">{error}</div>{/if}

<SettingsMfaSection
  {user}
  {mfaSetupState}
  bind:mfaDisableCode
  {disablingMfa}
  {startingMfaSetup}
  bind:mfaVerifyCode
  {verifyingMfa}
  handleStartMfaSetup={() => controller.startSetup()}
  handleVerifyMfa={(event) => controller.verify(event)}
  handleDisableMfa={(event) => controller.disable(event)}
/>
