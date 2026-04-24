/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';
import { fileURLToPath } from 'node:url';

import type {
  OrgMember,
  OrgInvitation,
  OrganizationDetail,
  UpdateOrgInput,
} from '../src/api/orgs';
import type { OrgGovernanceMutations } from '../src/pages/org-governance';
import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

const HarnessPath = fileURLToPath(
  new URL('./fixtures/org-governance-harness.svelte', import.meta.url)
);

describe('org governance controller harness', () => {
  test('saves the organization profile and reloads the updated org state', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async updateOrg(_slug, updates) {
          scenario.org = {
            ...scenario.org,
            name: updates.name || scenario.org.name,
            description: updates.description ?? null,
            website: updates.website ?? null,
            email: updates.email ?? null,
            mfa_required: Boolean(updates.mfaRequired),
            member_directory_is_private: Boolean(
              updates.memberDirectoryIsPrivate
            ),
          };
          scenario.updateOrgCalls.push(updates);
          return scenario.org;
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe('Source Org');
        expect(queryRequiredTextArea(target, '#org-profile-description').value).toBe(
          'Initial description'
        );
      });

      changeValue(queryRequiredInput(target, '#org-profile-name'), 'Source Registry');
      changeValue(
        queryRequiredTextArea(target, '#org-profile-description'),
        'Updated org profile copy'
      );
      changeValue(
        queryRequiredInput(target, '#org-profile-website'),
        'https://source.example.test'
      );
      changeValue(
        queryRequiredInput(target, '#org-profile-email'),
        'ops@example.test'
      );
      setChecked(queryRequiredInput(target, '#org-profile-mfa-required'), true);
      setChecked(
        queryRequiredInput(target, '#org-profile-member-directory-private'),
        true
      );
      submitForm(queryRequiredForm(target, '#org-profile-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Organization profile updated.');
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe(
          'Source Registry'
        );
        expect(queryRequiredTextArea(target, '#org-profile-description').value).toBe(
          'Updated org profile copy'
        );
        expect(queryRequiredInput(target, '#org-profile-website').value).toBe(
          'https://source.example.test'
        );
      });

      expect(scenario.updateOrgCalls).toEqual([
        {
          name: 'Source Registry',
          description: 'Updated org profile copy',
          website: 'https://source.example.test',
          email: 'ops@example.test',
          mfaRequired: true,
          memberDirectoryIsPrivate: true,
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('blocks blank organization names before submitting the profile mutation', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async updateOrg(_slug, updates) {
          scenario.updateOrgCalls.push(updates);
          return scenario.org;
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe('Source Org');
      });

      changeValue(queryRequiredInput(target, '#org-profile-name'), '   ');
      submitForm(queryRequiredForm(target, '#org-profile-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Organization name is required.');
      });

      expect(scenario.updateOrgCalls).toEqual([]);
    } finally {
      unmount();
    }
  });

  test('sends invitations, resets the form, and reloads invitation state', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async sendInvitation(_slug, input) {
          scenario.sendInvitationCalls.push({ ...input });
          scenario.invitations = [
            ...scenario.invitations,
            {
              id: 'invite-2',
              status: 'pending',
              role: input.role,
              invited_user: { email: input.usernameOrEmail || null },
              invited_by: { username: 'owner-user' },
              created_at: '2026-04-24T00:00:00Z',
              expires_at: '2026-05-01T00:00:00Z',
            },
          ];
          return {
            id: 'invite-2',
          };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('invited@example.test');
      });

      changeValue(
        queryRequiredInput(target, '#org-invite-target'),
        'new-maintainer@example.test'
      );
      changeValue(queryRequiredSelect(target, '#org-invite-role'), 'publisher');
      changeValue(queryRequiredInput(target, '#org-invite-expiry'), '14');
      submitForm(queryRequiredForm(target, '#org-invite-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Invitation sent successfully.');
        expect(target.textContent).toContain('new-maintainer@example.test');
      });

      expect(queryRequiredInput(target, '#org-invite-target').value).toBe('');
      expect(queryRequiredInput(target, '#org-invite-expiry').value).toBe('7');
      expect(scenario.sendInvitationCalls).toEqual([
        {
          usernameOrEmail: 'new-maintainer@example.test',
          role: 'publisher',
          expiresInDays: 14,
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('adds members directly and updates existing member roles', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async addMember(_slug, input) {
          scenario.addMemberCalls.push({ ...input });
          const existingMember = scenario.members.find(
            (member) => member.username === input.username
          );
          if (existingMember) {
            existingMember.role = input.role;
          } else {
            scenario.members = [
              ...scenario.members,
              {
                user_id: 'cccccccc-cccc-4ccc-8ccc-cccccccccccc',
                username: input.username,
                display_name: 'Release Bot',
                role: input.role,
                joined_at: '2026-04-24T00:00:00Z',
              },
            ];
          }
          return { members: scenario.members };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('admin-user');
      });

      changeValue(queryRequiredInput(target, '#org-member-username'), 'release-bot');
      changeValue(queryRequiredSelect(target, '#org-member-role'), 'publisher');
      submitForm(queryRequiredForm(target, '#org-member-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Member added successfully.');
        expect(target.textContent).toContain('release-bot');
      });

      changeValue(
        queryRequiredSelect(target, '#member-role-admin-user'),
        'publisher'
      );
      submitForm(queryRequiredForm(target, '#member-role-form-admin-user'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Updated @admin-user to Publisher.');
      });

      expect(scenario.addMemberCalls).toEqual([
        {
          username: 'release-bot',
          role: 'publisher',
        },
        {
          username: 'admin-user',
          role: 'publisher',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('requires explicit confirmation before transferring ownership and then reloads on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferOwnership(_slug, input) {
          scenario.transferOwnershipCalls.push({ ...input });
          return {
            new_owner: {
              username: input.username,
            },
          };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(queryRequiredButton(target, '#org-ownership-transfer-toggle')).toBeDefined();
      });

      changeValue(queryRequiredInput(target, '#org-transfer-owner'), 'admin-user');
      click(queryRequiredButton(target, '#org-ownership-transfer-toggle'));
      submitForm(queryRequiredForm(target, '#org-ownership-transfer-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Please confirm the ownership transfer.'
        );
      });

      setChecked(
        queryRequiredInput(target, '#org-ownership-transfer-confirm'),
        true
      );
      submitForm(queryRequiredForm(target, '#org-ownership-transfer-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Ownership transferred to @admin-user.'
        );
      });

      expect(scenario.transferOwnershipCalls).toEqual([
        {
          username: 'admin-user',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('revokes invitations and removes members after explicit confirmation', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async revokeInvitation(_slug, invitationId) {
          scenario.revokeInvitationCalls.push(invitationId);
          scenario.invitations = scenario.invitations.filter(
            (invitation) => invitation.id !== invitationId
          );
        },
        async removeMember(_slug, username) {
          scenario.removeMemberCalls.push(username);
          scenario.members = scenario.members.filter(
            (member) => member.username !== username
          );
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('invited@example.test');
        expect(target.textContent).toContain('admin-user');
      });

      click(
        queryRequiredButton(target, '#invitation-revoke-toggle-invite-1')
      );
      await waitFor(() => {
        flush();
        expect(
          queryRequiredForm(target, '#invitation-revoke-form-invite-1')
        ).toBeDefined();
      });
      submitForm(queryRequiredForm(target, '#invitation-revoke-form-invite-1'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Please confirm that you want to revoke this invitation immediately.'
        );
      });

      setChecked(
        queryRequiredInput(target, '#invitation-revoke-confirm-invite-1'),
        true
      );
      submitForm(queryRequiredForm(target, '#invitation-revoke-form-invite-1'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Invitation revoked.');
        expect(
          target.querySelector('[data-test="invitation-invite-1"]')
        ).toBeNull();
      });

      click(queryRequiredButton(target, '#member-remove-toggle-admin-user'));
      await waitFor(() => {
        flush();
        expect(
          queryRequiredForm(target, '#member-remove-form-admin-user')
        ).toBeDefined();
      });
      submitForm(queryRequiredForm(target, '#member-remove-form-admin-user'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Please confirm that you want to remove this member from the organization.'
        );
      });

      setChecked(
        queryRequiredInput(target, '#member-remove-confirm-admin-user'),
        true
      );
      submitForm(queryRequiredForm(target, '#member-remove-form-admin-user'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Removed @admin-user from the organization.'
        );
        expect(target.querySelector('[data-test="member-admin-user"]')).toBeNull();
      });

      expect(scenario.revokeInvitationCalls).toEqual(['invite-1']);
      expect(scenario.removeMemberCalls).toEqual(['admin-user']);
    } finally {
      unmount();
    }
  });
});

function createLoadState(scenario: Scenario) {
  return async () => ({
    org: { ...scenario.org },
    members: scenario.members.map((member) => ({ ...member })),
    invitations: scenario.invitations.map((invitation) => ({ ...invitation })),
  });
}

function createMutations(
  overrides: Partial<OrgGovernanceMutations> = {}
): OrgGovernanceMutations {
  return {
    async updateOrg(_slug, updates) {
      return {
        id: 'org-1',
        slug: 'source-org',
        name: updates.name || 'Source Org',
        description: updates.description ?? null,
        website: updates.website ?? null,
        email: updates.email ?? null,
        mfa_required: Boolean(updates.mfaRequired),
        member_directory_is_private: Boolean(updates.memberDirectoryIsPrivate),
      };
    },
    async sendInvitation() {
      return {
        id: 'invite-default',
      };
    },
    async addMember() {
      return { members: [] };
    },
    async transferOwnership(_slug, input) {
      return {
        new_owner: {
          username: input.username,
        },
      };
    },
    async revokeInvitation() {
    },
    async removeMember() {},
    ...overrides,
  };
}

interface Scenario {
  org: OrganizationDetail;
  members: OrgMember[];
  invitations: OrgInvitation[];
  updateOrgCalls: UpdateOrgInput[];
  sendInvitationCalls: Array<Record<string, unknown>>;
  addMemberCalls: Array<Record<string, unknown>>;
  transferOwnershipCalls: Array<Record<string, unknown>>;
  revokeInvitationCalls: string[];
  removeMemberCalls: string[];
}

function createScenario(): Scenario {
  return {
    org: {
      id: 'org-1',
      slug: 'source-org',
      name: 'Source Org',
      description: 'Initial description',
      website: 'https://initial.example.test',
      email: 'initial@example.test',
      mfa_required: false,
      member_directory_is_private: false,
    },
    members: [
      {
        user_id: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
        username: 'admin-user',
        display_name: 'Admin User',
        role: 'admin',
        joined_at: '2026-04-02T00:00:00Z',
      },
    ],
    invitations: [
      {
        id: 'invite-1',
        status: 'pending',
        role: 'viewer',
        invited_user: {
          email: 'invited@example.test',
        },
        invited_by: {
          username: 'owner-user',
        },
        created_at: '2026-04-20T00:00:00Z',
        expires_at: '2026-04-27T00:00:00Z',
      },
    ],
    updateOrgCalls: [],
    sendInvitationCalls: [],
    addMemberCalls: [],
    transferOwnershipCalls: [],
    revokeInvitationCalls: [],
    removeMemberCalls: [],
  };
}

function queryRequiredInput(target: HTMLElement, selector: string): HTMLInputElement {
  const input = target.querySelector(selector);
  expect(input).not.toBeNull();
  return input as HTMLInputElement;
}

function queryRequiredTextArea(
  target: HTMLElement,
  selector: string
): HTMLTextAreaElement {
  const textarea = target.querySelector(selector);
  expect(textarea).not.toBeNull();
  return textarea as HTMLTextAreaElement;
}

function queryRequiredSelect(target: HTMLElement, selector: string): HTMLSelectElement {
  const select = target.querySelector(selector);
  expect(select).not.toBeNull();
  return select as HTMLSelectElement;
}

function queryRequiredForm(target: HTMLElement, selector: string): HTMLFormElement {
  const form = target.querySelector(selector);
  expect(form).not.toBeNull();
  return form as HTMLFormElement;
}

function queryRequiredButton(
  target: HTMLElement,
  selector: string
): HTMLButtonElement {
  const button = target.querySelector(selector);
  expect(button).not.toBeNull();
  return button as HTMLButtonElement;
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
