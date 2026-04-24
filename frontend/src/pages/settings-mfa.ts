import {
  disableMfa,
  setupMfa,
  verifyMfaSetup,
  type MfaSetupState,
} from '../api/auth';

export interface SettingsMfaReloadOptions {
  notice?: string | null;
  error?: string | null;
  mfaSetupState?: MfaSetupState | null;
}

export interface SettingsMfaActions {
  setupMfa: typeof setupMfa;
  verifyMfaSetup: typeof verifyMfaSetup;
  disableMfa: typeof disableMfa;
}

export interface SettingsMfaControllerOptions {
  loadSettings: (options?: SettingsMfaReloadOptions) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  getMfaSetupState: () => MfaSetupState | null;
  getMfaVerifyCode: () => string;
  setMfaVerifyCode: (value: string) => void;
  getMfaDisableCode: () => string;
  setMfaDisableCode: (value: string) => void;
  setStartingMfaSetup: (value: boolean) => void;
  setVerifyingMfa: (value: boolean) => void;
  setDisablingMfa: (value: boolean) => void;
  actions?: SettingsMfaActions;
}

const DEFAULT_SETTINGS_MFA_ACTIONS: SettingsMfaActions = {
  setupMfa,
  verifyMfaSetup,
  disableMfa,
};

export function createSettingsMfaController(options: SettingsMfaControllerOptions) {
  const actions = options.actions || DEFAULT_SETTINGS_MFA_ACTIONS;

  return {
    async startSetup(): Promise<void> {
      options.setStartingMfaSetup(true);

      try {
        const setup = await actions.setupMfa();
        await options.loadSettings({
          notice: 'MFA setup initialized. Verify one code to enable it.',
          mfaSetupState: setup,
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(
            caughtError,
            'Failed to initialize MFA setup.'
          ),
        });
      } finally {
        options.setStartingMfaSetup(false);
      }
    },

    async verify(event: SubmitEvent): Promise<void> {
      event.preventDefault();

      const code = options.getMfaVerifyCode().trim();
      if (!code) {
        await options.loadSettings({
          error: 'A verification code is required.',
          mfaSetupState: options.getMfaSetupState(),
        });
        return;
      }

      options.setVerifyingMfa(true);

      try {
        await actions.verifyMfaSetup(code);
        options.setMfaVerifyCode('');
        await options.loadSettings({ notice: 'MFA enabled successfully.' });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(caughtError, 'Failed to verify MFA setup.'),
          mfaSetupState: options.getMfaSetupState(),
        });
      } finally {
        options.setVerifyingMfa(false);
      }
    },

    async disable(event: SubmitEvent): Promise<void> {
      event.preventDefault();

      const code = options.getMfaDisableCode().trim();
      if (!code) {
        await options.loadSettings({ error: 'A code is required to disable MFA.' });
        return;
      }

      options.setDisablingMfa(true);

      try {
        await actions.disableMfa(code);
        options.setMfaDisableCode('');
        await options.loadSettings({ notice: 'MFA disabled.' });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(caughtError, 'Failed to disable MFA.'),
        });
      } finally {
        options.setDisablingMfa(false);
      }
    },
  };
}
