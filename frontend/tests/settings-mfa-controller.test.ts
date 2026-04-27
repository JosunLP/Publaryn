/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { MfaSetupState } from '../src/api/auth';
import { createSettingsMfaController } from '../src/pages/settings-mfa';

describe('createSettingsMfaController', () => {
  test('clears setup state explicitly after successful verification', async () => {
    const loadCalls: Array<{
      notice?: string | null;
      error?: string | null;
      mfaSetupState?: MfaSetupState | null;
    }> = [];

    const controller = createSettingsMfaController({
      loadSettings: async (options) => {
        loadCalls.push(options ?? {});
      },
      toErrorMessage: (_caughtError, fallback) => fallback,
      getMfaSetupState: () => ({
        secret: 'MANUALSECRET123',
        provisioning_uri: 'otpauth://totp/Publaryn:alice?secret=MANUALSECRET123',
        recovery_codes: ['recovery-a1'],
      }),
      getMfaVerifyCode: () => '123456',
      setMfaVerifyCode: () => {},
      getMfaDisableCode: () => '',
      setMfaDisableCode: () => {},
      setStartingMfaSetup: () => {},
      setVerifyingMfa: () => {},
      setDisablingMfa: () => {},
      actions: {
        setupMfa: async () => {
          throw new Error('setup should not be called');
        },
        verifyMfaSetup: async () => ({ ok: true }),
        disableMfa: async () => {
          throw new Error('disable should not be called');
        },
      },
    });

    await controller.verify(createSubmitEvent());

    expect(loadCalls).toEqual([
      {
        notice: 'MFA enabled successfully.',
        mfaSetupState: null,
      },
    ]);
  });

  test('clears setup state explicitly after successful disable', async () => {
    const loadCalls: Array<{
      notice?: string | null;
      error?: string | null;
      mfaSetupState?: MfaSetupState | null;
    }> = [];

    const controller = createSettingsMfaController({
      loadSettings: async (options) => {
        loadCalls.push(options ?? {});
      },
      toErrorMessage: (_caughtError, fallback) => fallback,
      getMfaSetupState: () => ({
        secret: 'MANUALSECRET123',
        provisioning_uri: 'otpauth://totp/Publaryn:alice?secret=MANUALSECRET123',
        recovery_codes: ['recovery-a1'],
      }),
      getMfaVerifyCode: () => '',
      setMfaVerifyCode: () => {},
      getMfaDisableCode: () => '654321',
      setMfaDisableCode: () => {},
      setStartingMfaSetup: () => {},
      setVerifyingMfa: () => {},
      setDisablingMfa: () => {},
      actions: {
        setupMfa: async () => {
          throw new Error('setup should not be called');
        },
        verifyMfaSetup: async () => {
          throw new Error('verify should not be called');
        },
        disableMfa: async () => ({ ok: true }),
      },
    });

    await controller.disable(createSubmitEvent());

    expect(loadCalls).toEqual([
      {
        notice: 'MFA disabled.',
        mfaSetupState: null,
      },
    ]);
  });
});

function createSubmitEvent(): SubmitEvent {
  return {
    preventDefault() {},
  } as SubmitEvent;
}
