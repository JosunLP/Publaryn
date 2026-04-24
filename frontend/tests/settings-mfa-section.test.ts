/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import { changeValue, click, renderSvelte, submitForm } from './svelte-dom';

type SettingsMfaActions =
  import('../src/pages/settings-mfa').SettingsMfaActions;

const HarnessPath =
  '/home/runner/work/Publaryn/Publaryn/frontend/tests/fixtures/settings-mfa-harness.svelte';

describe('settings MFA section', () => {
  test('runs the MFA setup flow and switches to the enabled state after verification', async () => {
    const calls = {
      setup: 0,
      verify: [] as string[],
    };

    const actions: SettingsMfaActions = {
      async setupMfa() {
        calls.setup += 1;
        return {
          secret: 'MANUALSECRET123',
          provisioning_uri:
            'otpauth://totp/Publaryn:alice?secret=MANUALSECRET123&issuer=Publaryn',
          recovery_codes: ['recovery-a1', 'recovery-b2'],
        };
      },
      async verifyMfaSetup(code: string) {
        calls.verify.push(code);
        return { ok: true };
      },
      async disableMfa() {
        throw new Error('disableMfa should not be called during setup flow');
      },
    };

    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      user: {
        id: 'user-1',
        username: 'alice',
        email: 'alice@example.test',
        mfa_enabled: false,
      },
      actions,
    });

    try {
      await waitFor(() => {
        flush();
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Disabled');
        expect(target.querySelector('#mfa-setup-btn')).not.toBeNull();
      });

      click(queryRequiredButton(target, '#mfa-setup-btn'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'MFA setup initialized. Verify one code to enable it.'
        );
        expect(target.textContent).toContain('MANUALSECRET123');
        expect(target.textContent).toContain('recovery-a1');
      });

      changeValue(queryRequiredInput(target, '#mfa-verify-code'), '123456');
      submitForm(queryRequiredForm(target, '#mfa-verify-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('MFA enabled successfully.');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Enabled');
        expect(target.querySelector('#mfa-disable-form')).not.toBeNull();
      });

      expect(calls.setup).toBe(1);
      expect(calls.verify).toEqual(['123456']);
    } finally {
      unmount();
    }
  });

  test('runs the MFA disable flow and returns to setup mode', async () => {
    const calls = {
      disable: [] as string[],
    };

    const actions: SettingsMfaActions = {
      async setupMfa() {
        throw new Error('setupMfa should not be called during disable flow');
      },
      async verifyMfaSetup() {
        throw new Error('verifyMfaSetup should not be called during disable flow');
      },
      async disableMfa(code: string) {
        calls.disable.push(code);
        return { ok: true };
      },
    };

    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      user: {
        id: 'user-1',
        username: 'alice',
        email: 'alice@example.test',
        mfa_enabled: true,
      },
      actions,
    });

    try {
      await waitFor(() => {
        flush();
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Enabled');
        expect(target.querySelector('#mfa-disable-form')).not.toBeNull();
      });

      changeValue(queryRequiredInput(target, '#mfa-disable-code'), 'recovery-a1');
      submitForm(queryRequiredForm(target, '#mfa-disable-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('MFA disabled.');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Disabled');
        expect(target.querySelector('#mfa-setup-btn')).not.toBeNull();
      });

      expect(calls.disable).toEqual(['recovery-a1']);
    } finally {
      unmount();
    }
  });
});

function queryRequiredButton(target: HTMLElement, selector: string): HTMLButtonElement {
  const button = target.querySelector(selector);
  expect(button).not.toBeNull();
  return button as HTMLButtonElement;
}

function queryRequiredInput(target: HTMLElement, selector: string): HTMLInputElement {
  const input = target.querySelector(selector);
  expect(input).not.toBeNull();
  return input as HTMLInputElement;
}

function queryRequiredForm(target: HTMLElement, selector: string): HTMLFormElement {
  const form = target.querySelector(selector);
  expect(form).not.toBeNull();
  return form as HTMLFormElement;
}

function normalizeWhitespace(value: string | null | undefined): string {
  return (value || '').replace(/\s+/g, ' ').trim();
}

async function waitFor(
  assertion: () => void,
  { timeout = 1000, interval = 10 }: { timeout?: number; interval?: number } = {}
): Promise<void> {
  const startedAt = Date.now();
  let lastError: unknown;

  while (Date.now() - startedAt < timeout) {
    try {
      assertion();
      return;
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, interval));
    }
  }

  throw lastError instanceof Error ? lastError : new Error('Timed out waiting for assertion.');
}
