import { getAuthToken } from '../api/client.js';
import {
  addMember,
  getOrg,
  listMembers,
  listMyOrganizations,
  listOrgInvitations,
  listOrgPackages,
  listTeams,
  removeMember,
  revokeInvitation,
  sendInvitation,
} from '../api/orgs.js';
import { escapeHtml, formatDate, formatNumber } from '../utils/format.js';

const ADMIN_ROLES = new Set(['owner', 'admin']);
const ORG_ROLE_OPTIONS = [
  { value: 'admin', label: 'Admin' },
  { value: 'maintainer', label: 'Maintainer' },
  { value: 'publisher', label: 'Publisher' },
  { value: 'security_manager', label: 'Security manager' },
  { value: 'auditor', label: 'Auditor' },
  { value: 'billing_manager', label: 'Billing manager' },
  { value: 'viewer', label: 'Viewer' },
];

export function orgDetailPage({ params }, container) {
  const slug = params.slug;
  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading organization…</div>`;
  loadAndRender(container, slug);
}

async function loadAndRender(
  container,
  slug,
  { notice = null, error = null } = {}
) {
  try {
    const isAuthenticated = Boolean(getAuthToken());
    const [org, memberData, teamData, packageData, myOrganizationsData] =
      await Promise.all([
        getOrg(slug),
        listMembers(slug).catch((err) => ({
          members: [],
          load_error: err?.message || 'Failed to load organization members.',
        })),
        listTeams(slug).catch((err) => ({
          teams: [],
          load_error: err?.message || 'Failed to load teams.',
        })),
        listOrgPackages(slug).catch((err) => ({
          packages: [],
          load_error: err?.message || 'Failed to load packages.',
        })),
        isAuthenticated
          ? listMyOrganizations().catch(() => ({ organizations: [] }))
          : Promise.resolve({ organizations: [] }),
      ]);

    const membership = (myOrganizationsData.organizations || []).find(
      (item) => item.slug === slug
    );
    const canAdminister = ADMIN_ROLES.has(membership?.role || '');

    const invitationData = canAdminister
      ? await listOrgInvitations(slug).catch((err) => ({
          invitations: [],
          load_error: err?.message || 'Failed to load invitations.',
        }))
      : { invitations: [], load_error: null };

    render(container, {
      slug,
      org,
      notice,
      error,
      membership,
      canAdminister,
      members: memberData.members || [],
      membersError: memberData.load_error || null,
      teams: teamData.teams || [],
      teamsError: teamData.load_error || null,
      packages: packageData.packages || [],
      packagesError: packageData.load_error || null,
      invitations: invitationData.invitations || [],
      invitationsError: invitationData.load_error || null,
      isAuthenticated,
    });
  } catch (err) {
    if (err?.status === 404) {
      container.innerHTML = `
        <div class="empty-state mt-6">
          <h2>Organization not found</h2>
          <p>@${escapeHtml(slug)} does not exist or is no longer available.</p>
          <a href="/search" class="btn btn-primary mt-4">Search packages</a>
        </div>
      `;
      return;
    }

    container.innerHTML = `
      <div class="mt-6">
        <div class="alert alert-error">${escapeHtml(err?.message || 'Failed to load organization.')}</div>
      </div>
    `;
  }
}

function render(container, state) {
  const {
    slug,
    org,
    notice,
    error,
    membership,
    canAdminister,
    members,
    membersError,
    teams,
    teamsError,
    packages,
    packagesError,
    invitations,
    invitationsError,
    isAuthenticated,
  } = state;

  container.innerHTML = `
    <div class="mt-6 org-page settings-page">
      ${notice ? `<div class="alert alert-success">${escapeHtml(notice)}</div>` : ''}
      ${error ? `<div class="alert alert-error">${escapeHtml(error)}</div>` : ''}

      <section class="card org-hero">
        <div class="org-hero__header">
          <div class="org-hero__copy">
            <div class="org-hero__eyebrow">Organization workspace</div>
            <div class="pkg-header">
              <h1 class="pkg-header__name">${escapeHtml(org.name || slug)}</h1>
              ${org.is_verified ? '<span class="badge badge-verified">Verified</span>' : ''}
            </div>
            <p class="text-muted">@${escapeHtml(org.slug || slug)}</p>
            ${org.description ? `<p class="settings-copy">${escapeHtml(org.description)}</p>` : '<p class="settings-copy">No organization description yet.</p>'}
          </div>

          <div class="org-hero__meta">
            ${org.website ? `<a href="${escapeHtml(org.website)}" target="_blank" rel="noopener noreferrer">${escapeHtml(org.website)}</a>` : ''}
            ${org.email ? `<a href="mailto:${escapeHtml(org.email)}">${escapeHtml(org.email)}</a>` : ''}
            ${org.created_at ? `<span>Created ${escapeHtml(formatDate(org.created_at))}</span>` : ''}
          </div>
        </div>

        <div class="org-kpi-grid">
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(String(members.length))}</span>
            <span class="org-kpi__label">Members</span>
          </div>
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(String(teams.length))}</span>
            <span class="org-kpi__label">Teams</span>
          </div>
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(String(packages.length))}</span>
            <span class="org-kpi__label">Visible packages</span>
          </div>
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(formatRole(membership?.role || 'public'))}</span>
            <span class="org-kpi__label">Your access</span>
          </div>
        </div>
      </section>

      <div class="settings-grid">
        <section class="card settings-section">
          <h2>Your access</h2>
          ${
            membership
              ? `
                <p class="settings-copy">
                  You are a <strong>${escapeHtml(formatRole(membership.role))}</strong> in this organization.
                  ${canAdminister ? 'You can manage invitations and direct memberships from this page.' : 'You can view organization details here; broader admin actions require an owner or admin role.'}
                </p>
              `
              : isAuthenticated
                ? `
                  <p class="settings-copy">
                    You are signed in but not currently a member of this organization.
                    Public information remains visible, while private package visibility and admin actions stay restricted.
                  </p>
                `
                : `
                  <p class="settings-copy">
                    You are viewing this organization as a public visitor.
                    <a href="/login">Sign in</a> to access private memberships and org-admin actions.
                  </p>
                `
          }
        </section>

        <section class="card settings-section">
          <h2>About this organization</h2>
          <p class="settings-copy">
            Use this page as the canonical workspace for organization ownership, members, teams, and visible packages.
            Team CRUD, delegated package access, and audit/security dashboards land here next.
          </p>
        </section>
      </div>

      ${
        canAdminister
          ? `
            <div class="org-admin-grid">
              <section class="card settings-section">
                <h2>Invite a member</h2>
                <p class="settings-copy">Invitations currently resolve against an existing username or email, then require acceptance by the invited user.</p>
                <form id="org-invite-form">
                  <div class="form-group">
                    <label for="org-invite-target">Username or email</label>
                    <input id="org-invite-target" name="username_or_email" class="form-input" placeholder="alice or alice@example.com" required />
                  </div>
                  <div class="form-group">
                    <label for="org-invite-role">Role</label>
                    <select id="org-invite-role" name="role" class="form-input">
                      ${renderRoleOptions('viewer')}
                    </select>
                  </div>
                  <div class="form-group">
                    <label for="org-invite-expiry">Expires in days</label>
                    <input id="org-invite-expiry" name="expires_in_days" type="number" min="1" max="30" class="form-input" value="7" />
                  </div>
                  <button type="submit" class="btn btn-primary">Send invitation</button>
                </form>
              </section>

              <section class="card settings-section">
                <h2>Add existing member directly</h2>
                <p class="settings-copy">This immediately grants membership to an existing user account. Ownership transfer remains a dedicated later flow.</p>
                <form id="org-member-form">
                  <div class="form-group">
                    <label for="org-member-username">Username</label>
                    <input id="org-member-username" name="username" class="form-input" placeholder="alice" required />
                  </div>
                  <div class="form-group">
                    <label for="org-member-role">Role</label>
                    <select id="org-member-role" name="role" class="form-input">
                      ${renderRoleOptions('viewer')}
                    </select>
                  </div>
                  <button type="submit" class="btn btn-primary">Add member</button>
                </form>
              </section>
            </div>
          `
          : ''
      }

      ${
        canAdminister
          ? `
            <section class="card settings-section">
              <div class="org-section-header">
                <div>
                  <h2>Pending invitations</h2>
                  <p class="settings-copy">Review active invitations and revoke them when plans change.</p>
                </div>
              </div>

              ${
                invitationsError
                  ? `<div class="alert alert-error">${escapeHtml(invitationsError)}</div>`
                  : invitations.length === 0
                    ? `<div class="empty-state"><h3>No active invitations</h3><p>New invitations sent from this page will appear here until they are accepted, declined, revoked, or expired.</p></div>`
                    : `<div class="token-list">
                        ${invitations
                          .map(
                            (invitation) => `
                              <div class="token-row">
                                <div class="token-row__main">
                                  <div class="token-row__title">@${escapeHtml(invitation.invited_user?.username || 'unknown')}</div>
                                  <div class="token-row__meta">
                                    <span>${escapeHtml(invitation.invited_user?.email || 'No email')}</span>
                                    <span>role ${escapeHtml(formatRole(invitation.role || 'viewer'))}</span>
                                    <span>invited ${escapeHtml(formatDate(invitation.created_at))}</span>
                                    <span>expires ${escapeHtml(formatDate(invitation.expires_at))}</span>
                                  </div>
                                </div>
                                <div class="token-row__actions">
                                  <button class="btn btn-secondary btn-sm" data-revoke-invitation="${escapeHtml(invitation.id || '')}" type="button">Revoke</button>
                                </div>
                              </div>
                            `
                          )
                          .join('')}
                      </div>`
              }
            </section>
          `
          : ''
      }

      <section class="card settings-section">
        <div class="org-section-header">
          <div>
            <h2>Members</h2>
            <p class="settings-copy">Public organization memberships and their effective organization roles.</p>
          </div>
        </div>

        ${
          membersError
            ? `<div class="alert alert-error">${escapeHtml(membersError)}</div>`
            : members.length === 0
              ? `<div class="empty-state"><h3>No members yet</h3><p>This organization has not added any members yet.</p></div>`
              : `<div class="token-list">
                  ${members
                    .map(
                      (member) => `
                        <div class="token-row">
                          <div class="token-row__main">
                            <div class="token-row__title">${escapeHtml(member.display_name || member.username || 'Unknown member')}</div>
                            <div class="token-row__meta">
                              <span>@${escapeHtml(member.username || 'unknown')}</span>
                              <span>role ${escapeHtml(formatRole(member.role || 'viewer'))}</span>
                              <span>joined ${escapeHtml(formatDate(member.joined_at))}</span>
                            </div>
                          </div>
                          ${
                            canAdminister && member.role !== 'owner'
                              ? `
                                <div class="token-row__actions">
                                  <button class="btn btn-danger btn-sm" data-remove-member="${escapeHtml(member.username || '')}" type="button">Remove</button>
                                </div>
                              `
                              : ''
                          }
                        </div>
                      `
                    )
                    .join('')}
                </div>`
        }
      </section>

      <div class="settings-grid">
        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Teams</h2>
              <p class="settings-copy">Current team definitions for this organization. Team creation and delegated package access management are the next dedicated UI slice.</p>
            </div>
          </div>

          ${
            teamsError
              ? `<div class="alert alert-error">${escapeHtml(teamsError)}</div>`
              : teams.length === 0
                ? `<div class="empty-state"><h3>No teams yet</h3><p>Team management is backed by the API and will surface here as the workspace expands.</p></div>`
                : `<div class="token-list">
                    ${teams
                      .map(
                        (team) => `
                          <div class="token-row">
                            <div class="token-row__main">
                              <div class="token-row__title">${escapeHtml(team.name || team.slug || 'Team')}</div>
                              <div class="token-row__meta">
                                <span>${escapeHtml(team.slug || 'no-slug')}</span>
                                <span>created ${escapeHtml(formatDate(team.created_at))}</span>
                              </div>
                              ${team.description ? `<p class="settings-copy">${escapeHtml(team.description)}</p>` : ''}
                            </div>
                          </div>
                        `
                      )
                      .join('')}
                  </div>`
          }
        </section>

        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Visible packages</h2>
              <p class="settings-copy">Showing the packages currently visible from this organization. Public visitors see public packages only.</p>
            </div>
          </div>

          ${
            packagesError
              ? `<div class="alert alert-error">${escapeHtml(packagesError)}</div>`
              : packages.length === 0
                ? `<div class="empty-state"><h3>No packages yet</h3><p>No packages are currently visible for this organization.</p></div>`
                : `<div class="token-list">
                    ${packages
                      .map(
                        (pkg) => `
                          <div class="token-row">
                            <div class="token-row__main">
                              <div class="token-row__title"><a href="/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name || '')}">${escapeHtml(pkg.name || 'Unnamed package')}</a></div>
                              <div class="token-row__meta">
                                <span>${escapeHtml(pkg.ecosystem || 'unknown')}</span>
                                <span>${escapeHtml(formatNumber(pkg.download_count))} downloads</span>
                                <span>created ${escapeHtml(formatDate(pkg.created_at))}</span>
                              </div>
                              ${pkg.description ? `<p class="settings-copy">${escapeHtml(pkg.description)}</p>` : ''}
                            </div>
                          </div>
                        `
                      )
                      .join('')}
                  </div>`
          }
        </section>
      </div>
    </div>
  `;

  const inviteForm = container.querySelector('#org-invite-form');
  inviteForm?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const form = event.currentTarget;
    const formData = new FormData(form);
    const submitButton = form.querySelector('button[type="submit"]');
    submitButton.disabled = true;
    submitButton.textContent = 'Sending…';

    try {
      await sendInvitation(slug, {
        usernameOrEmail:
          formData.get('username_or_email')?.toString().trim() || '',
        role: formData.get('role')?.toString() || 'viewer',
        expiresInDays:
          Number(formData.get('expires_in_days')?.toString().trim() || '7') ||
          7,
      });
      await loadAndRender(container, slug, {
        notice: 'Invitation sent successfully.',
      });
    } catch (err) {
      await loadAndRender(container, slug, {
        error: err.message || 'Failed to send invitation.',
      });
    }
  });

  const memberForm = container.querySelector('#org-member-form');
  memberForm?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const form = event.currentTarget;
    const formData = new FormData(form);
    const submitButton = form.querySelector('button[type="submit"]');
    submitButton.disabled = true;
    submitButton.textContent = 'Adding…';

    try {
      await addMember(slug, {
        username: formData.get('username')?.toString().trim() || '',
        role: formData.get('role')?.toString() || 'viewer',
      });
      await loadAndRender(container, slug, {
        notice: 'Member added successfully.',
      });
    } catch (err) {
      await loadAndRender(container, slug, {
        error: err.message || 'Failed to add member.',
      });
    }
  });

  container.querySelectorAll('[data-revoke-invitation]').forEach((button) => {
    button.addEventListener('click', async () => {
      const invitationId = button.getAttribute('data-revoke-invitation');
      if (!invitationId) return;

      button.disabled = true;
      button.textContent = 'Revoking…';

      try {
        await revokeInvitation(slug, invitationId);
        await loadAndRender(container, slug, {
          notice: 'Invitation revoked.',
        });
      } catch (err) {
        await loadAndRender(container, slug, {
          error: err.message || 'Failed to revoke invitation.',
        });
      }
    });
  });

  container.querySelectorAll('[data-remove-member]').forEach((button) => {
    button.addEventListener('click', async () => {
      const username = button.getAttribute('data-remove-member');
      if (!username) return;

      button.disabled = true;
      button.textContent = 'Removing…';

      try {
        await removeMember(slug, username);
        await loadAndRender(container, slug, {
          notice: `Removed @${username} from the organization.`,
        });
      } catch (err) {
        await loadAndRender(container, slug, {
          error: err.message || 'Failed to remove member.',
        });
      }
    });
  });
}

function renderRoleOptions(selectedValue) {
  return ORG_ROLE_OPTIONS.map(
    (role) => `
      <option value="${role.value}" ${role.value === selectedValue ? 'selected' : ''}>
        ${role.label}
      </option>
    `
  ).join('');
}

function formatRole(role) {
  return role
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}
