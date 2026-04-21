<script lang="ts">
  import { page } from '$app/stores';

  import { ApiError } from '../../../../../api/client';
  import type { NamespaceClaim, NamespaceListResponse } from '../../../../../api/namespaces';
  import { listOrgNamespaces } from '../../../../../api/namespaces';
  import type {
    MemberListResponse,
    OrgMember,
    OrgPackageListResponse,
    OrgPackageSummary,
    OrgRepositoryListResponse,
    OrgRepositorySummary,
    Team,
    TeamMember,
    TeamMemberListResponse,
    TeamNamespaceAccessGrant,
    TeamNamespaceAccessListResponse,
    TeamNamespaceAccessMutationResult,
    TeamPackageAccessGrant,
    TeamPackageAccessListResponse,
    TeamRepositoryAccessGrant,
    TeamRepositoryAccessListResponse,
  } from '../../../../../api/orgs';
  import {
    addTeamMember,
    getOrg,
    listMembers,
    listOrgPackages,
    listOrgRepositories,
    listTeamMembers,
    listTeamNamespaceAccess,
    listTeamPackageAccess,
    listTeamRepositoryAccess,
    listTeams,
    removeTeamMember,
    removeTeamNamespaceAccess,
    removeTeamPackageAccess,
    removeTeamRepositoryAccess,
    replaceTeamNamespaceAccess,
    replaceTeamPackageAccess,
    replaceTeamRepositoryAccess,
    updateTeam,
  } from '../../../../../api/orgs';
  import TeamAccessGrantForm from '../../../../../lib/components/TeamAccessGrantForm.svelte';
  import {
    buildOrgMemberPickerOptions,
    resolveOrgMemberPickerInput,
  } from '../../../../../pages/org-member-picker';
  import { sortNamespaceClaims } from '../../../../../pages/personal-namespaces';
  import {
    renderPackageSelectionValue,
    resolveTeamNamespaceAccessSubmission,
    resolveTeamPackageAccessSubmission,
    resolveTeamRepositoryAccessSubmission,
  } from '../../../../../pages/org-workspace-actions';
  import {
    canManageOrgNamespaces,
    canManageOrgRepositories,
    canManageOrgTeams,
  } from '../../../../../pages/org-workspace-access';
  import { ecosystemLabel } from '../../../../../utils/ecosystem';
  import { formatDate, formatNumber } from '../../../../../utils/format';
  import {
    formatRepositoryKindLabel,
    formatRepositoryVisibilityLabel,
  } from '../../../../../utils/repositories';

  const TEAM_PERMISSION_OPTIONS = [
    {
      value: 'admin',
      label: 'Admin',
      description: 'Manage package administration workflows.',
    },
    {
      value: 'publish',
      label: 'Publish',
      description: 'Create releases and publish artifacts.',
    },
    {
      value: 'write_metadata',
      label: 'Write metadata',
      description: 'Update package readmes and metadata.',
    },
    {
      value: 'read_private',
      label: 'Read private',
      description: 'Read non-public package data.',
    },
    {
      value: 'security_review',
      label: 'Security review',
      description: 'Reserved for future security workflows.',
    },
    {
      value: 'transfer_ownership',
      label: 'Transfer ownership',
      description: 'Transfer a package to another owner.',
    },
  ] as const;
  const TEAM_NAMESPACE_PERMISSION_OPTIONS = [
    {
      value: 'admin',
      label: 'Admin',
      description: 'Delete organization-owned namespace claims.',
    },
    {
      value: 'transfer_ownership',
      label: 'Transfer ownership',
      description: 'Transfer a namespace claim into another controlled organization.',
    },
  ] as const;

  let lastLoadKey = '';
  let loading = true;
  let loadError: string | null = null;
  let notice: string | null = null;
  let error: string | null = null;
  let notFound = false;

  let org:
    | Awaited<ReturnType<typeof getOrg>>
    | null = null;
  let team: Team | null = null;
  let orgMembers: OrgMember[] = [];
  let orgPackages: OrgPackageSummary[] = [];
  let orgRepositories: OrgRepositorySummary[] = [];
  let orgNamespaces: NamespaceClaim[] = [];
  let members: TeamMember[] = [];
  let packageAccess: TeamPackageAccessGrant[] = [];
  let repositoryAccess: TeamRepositoryAccessGrant[] = [];
  let namespaceAccess: TeamNamespaceAccessGrant[] = [];
  let orgMembersError: string | null = null;
  let orgPackagesError: string | null = null;
  let orgRepositoriesError: string | null = null;
  let orgNamespacesError: string | null = null;
  let membersError: string | null = null;
  let packageAccessError: string | null = null;
  let repositoryAccessError: string | null = null;
  let namespaceAccessError: string | null = null;

  $: slug = $page.params.slug ?? '';
  $: teamSlug = $page.params.team_slug ?? '';
  $: loadKey = `${slug}|${teamSlug}`;
  $: if (slug && teamSlug && loadKey !== lastLoadKey) {
    lastLoadKey = loadKey;
    void loadTeamWorkspace();
  }

  $: teamName = team?.name?.trim() || team?.slug?.trim() || 'Team';
  $: canViewRepositoryAccess = canManageOrgRepositories(org);
  $: canViewNamespaceAccess = canManageOrgNamespaces(org);
  $: teamWorkspaceAnchor = `/orgs/${encodeURIComponent(slug)}#team-${encodeURIComponent(
    team?.slug || teamSlug
  )}`;
  $: packageGrantOptions = [...orgPackages]
    .sort((left, right) =>
      `${left.ecosystem || ''}:${left.name || ''}`.localeCompare(
        `${right.ecosystem || ''}:${right.name || ''}`
      )
    )
    .map((pkg) => ({
      value: renderPackageSelectionValue(pkg.ecosystem, pkg.name),
      label: `${pkg.ecosystem || ''} · ${pkg.name || ''}`,
    }));
  $: repositoryGrantOptions = [...orgRepositories]
    .sort((left, right) =>
      `${left.name || left.slug || ''}`.localeCompare(
        `${right.name || right.slug || ''}`
      )
    )
    .map((repository) => ({
      value: repository.slug || '',
      label: `${repository.name || repository.slug || ''} · ${formatRepositoryKindLabel(repository.kind)} · ${formatRepositoryVisibilityLabel(repository.visibility)}`,
    }));
  $: namespaceGrantOptions = sortNamespaceClaims(orgNamespaces)
    .filter(
      (claim): claim is NamespaceClaim & { id: string } =>
        typeof claim.id === 'string' && claim.id.trim().length > 0
    )
    .map((claim) => ({
      value: claim.id,
      label: `${claim.namespace || 'Unnamed claim'} · ${ecosystemLabel(claim.ecosystem)}`,
    }));
  $: eligibleTeamMemberOptions = buildOrgMemberPickerOptions(
    orgMembers,
    members.map((member) => member.username?.trim() || '').filter(Boolean)
  );

  async function loadTeamWorkspace(
    options: { notice?: string | null; error?: string | null } = {}
  ): Promise<void> {
    loading = true;
    loadError = null;
    notice = options.notice ?? null;
    error = options.error ?? null;
    notFound = false;
    org = null;
    team = null;
    orgMembers = [];
    orgPackages = [];
    orgRepositories = [];
    orgNamespaces = [];
    members = [];
    packageAccess = [];
    repositoryAccess = [];
    namespaceAccess = [];
    orgMembersError = null;
    orgPackagesError = null;
    orgRepositoriesError = null;
    orgNamespacesError = null;
    membersError = null;
    packageAccessError = null;
    repositoryAccessError = null;
    namespaceAccessError = null;

    try {
      const [loadedOrg, teamData] = await Promise.all([getOrg(slug), listTeams(slug)]);
      org = loadedOrg;
      team =
        (teamData.teams || []).find((candidate) => candidate.slug === teamSlug) || null;

      if (!team) {
        notFound = true;
        return;
      }

      if (!canManageOrgTeams(loadedOrg)) {
        loadError = 'Team workspaces are available to organization administrators.';
        return;
      }

      const [
        memberData,
        packageData,
        repositoryData,
        namespaceData,
        orgMemberData,
        orgPackageData,
        orgRepositoryData,
        orgNamespaceData,
      ] =
        await Promise.all([
          listTeamMembers(slug, teamSlug).catch(
            (caughtError: unknown): TeamMemberListResponse => ({
              members: [],
              load_error: toErrorMessage(
                caughtError,
                `Failed to load members for ${team?.name || teamSlug}.`
              ),
            })
          ),
          listTeamPackageAccess(slug, teamSlug).catch(
            (caughtError: unknown): TeamPackageAccessListResponse => ({
              package_access: [],
              load_error: toErrorMessage(
                caughtError,
                `Failed to load package access for ${team?.name || teamSlug}.`
              ),
            })
          ),
          canManageOrgRepositories(loadedOrg)
            ? listTeamRepositoryAccess(slug, teamSlug).catch(
                (caughtError: unknown): TeamRepositoryAccessListResponse => ({
                  repository_access: [],
                  load_error: toErrorMessage(
                    caughtError,
                    `Failed to load repository access for ${team?.name || teamSlug}.`
                  ),
                })
              )
            : Promise.resolve<TeamRepositoryAccessListResponse>({
                repository_access: [],
                load_error: null,
              }),
          canManageOrgNamespaces(loadedOrg)
            ? listTeamNamespaceAccess(slug, teamSlug).catch(
                (caughtError: unknown): TeamNamespaceAccessListResponse => ({
                  namespace_access: [],
                  load_error: toErrorMessage(
                    caughtError,
                    `Failed to load namespace access for ${team?.name || teamSlug}.`
                  ),
                })
              )
            : Promise.resolve<TeamNamespaceAccessListResponse>({
                namespace_access: [],
                load_error: null,
              }),
          listMembers(slug).catch((caughtError: unknown): MemberListResponse => ({
            members: [],
            load_error: toErrorMessage(
              caughtError,
              'Failed to load organization members.'
            ),
          })),
          listOrgPackages(slug).catch(
            (caughtError: unknown): OrgPackageListResponse => ({
              packages: [],
              load_error: toErrorMessage(
                caughtError,
                'Failed to load organization packages.'
              ),
            })
          ),
          canManageOrgRepositories(loadedOrg)
            ? listOrgRepositories(slug).catch(
                (caughtError: unknown): OrgRepositoryListResponse => ({
                  repositories: [],
                  load_error: toErrorMessage(
                    caughtError,
                    'Failed to load organization repositories.'
                  ),
                })
              )
            : Promise.resolve<OrgRepositoryListResponse>({
                repositories: [],
                load_error: null,
              }),
          canManageOrgNamespaces(loadedOrg)
            ? loadedOrg.id?.trim()
              ? listOrgNamespaces(loadedOrg.id).catch(
                  (caughtError: unknown): NamespaceListResponse => ({
                    namespaces: [],
                    load_error: toErrorMessage(
                      caughtError,
                      'Failed to load organization namespace claims.'
                    ),
                  })
                )
              : Promise.resolve<NamespaceListResponse>({
                  namespaces: [],
                  load_error:
                    'Failed to load namespace claims because the organization id is unavailable.',
                })
            : Promise.resolve<NamespaceListResponse>({
                namespaces: [],
                load_error: null,
              }),
        ]);

      orgMembers = orgMemberData.members || [];
      orgMembersError = orgMemberData.load_error || null;
      orgPackages = orgPackageData.packages || [];
      orgPackagesError = orgPackageData.load_error || null;
      orgRepositories = orgRepositoryData.repositories || [];
      orgRepositoriesError = orgRepositoryData.load_error || null;
      orgNamespaces = orgNamespaceData.namespaces || [];
      orgNamespacesError = orgNamespaceData.load_error || null;
      members = memberData.members || [];
      membersError = memberData.load_error || null;
      packageAccess = packageData.package_access || [];
      packageAccessError = packageData.load_error || null;
      repositoryAccess = repositoryData.repository_access || [];
      repositoryAccessError = repositoryData.load_error || null;
      namespaceAccess = namespaceData.namespace_access || [];
      namespaceAccessError = namespaceData.load_error || null;
    } catch (caughtError: unknown) {
      if (caughtError instanceof ApiError && caughtError.status === 404) {
        notFound = true;
      } else {
        loadError = toErrorMessage(
          caughtError,
          'Failed to load the team workspace.'
        );
      }
    } finally {
      loading = false;
    }
  }

  async function handleUpdateTeam(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await updateTeam(slug, teamSlug, {
        name: formData.get('name')?.toString().trim() || '',
        description: formData.get('description')?.toString().trim() || '',
      });
      await loadTeamWorkspace({
        notice: `Saved changes to ${teamSlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to update team.'),
      });
    }
  }

  async function handleAddTeamMember(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const username = resolveOrgMemberPickerInput(
      formData.get('username')?.toString() || '',
      eligibleTeamMemberOptions
    );

    try {
      await addTeamMember(slug, teamSlug, { username });
      (event.currentTarget as HTMLFormElement).reset();
      await loadTeamWorkspace({
        notice: `Added a member to ${teamSlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to add team member.'),
      });
    }
  }

  async function handleRemoveMember(username: string): Promise<void> {
    try {
      await removeTeamMember(slug, teamSlug, username);
      await loadTeamWorkspace({
        notice: `Removed @${username} from ${teamSlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to remove team member.'),
      });
    }
  }

  async function handleReplacePackageAccess(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const resolution = resolveTeamPackageAccessSubmission(
      new FormData(event.currentTarget as HTMLFormElement)
    );

    if (!resolution.ok) {
      await loadTeamWorkspace({ error: resolution.error });
      return;
    }

    try {
      await replaceTeamPackageAccess(
        slug,
        teamSlug,
        resolution.value.ecosystem,
        resolution.value.name,
        {
          permissions: resolution.value.permissions,
        }
      );
      await loadTeamWorkspace({
        notice: `Saved package access for ${resolution.value.name}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to update package access.'),
      });
    }
  }

  async function handleRemovePackageAccess(
    ecosystem: string,
    packageName: string
  ): Promise<void> {
    try {
      await removeTeamPackageAccess(slug, teamSlug, ecosystem, packageName);
      await loadTeamWorkspace({
        notice: `Revoked package access for ${packageName}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to revoke package access.'),
      });
    }
  }

  async function handleReplaceRepositoryAccess(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const resolution = resolveTeamRepositoryAccessSubmission(
      new FormData(event.currentTarget as HTMLFormElement)
    );

    if (!resolution.ok) {
      await loadTeamWorkspace({ error: resolution.error });
      return;
    }

    try {
      await replaceTeamRepositoryAccess(
        slug,
        teamSlug,
        resolution.value.repositorySlug,
        {
          permissions: resolution.value.permissions,
        }
      );
      await loadTeamWorkspace({
        notice: `Saved repository access for ${resolution.value.repositorySlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to update repository access.'),
      });
    }
  }

  async function handleRemoveRepositoryAccess(repositorySlug: string): Promise<void> {
    try {
      await removeTeamRepositoryAccess(slug, teamSlug, repositorySlug);
      await loadTeamWorkspace({
        notice: `Revoked repository access for ${repositorySlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to revoke repository access.'),
      });
    }
  }

  async function handleReplaceNamespaceAccess(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const resolution = resolveTeamNamespaceAccessSubmission(
      new FormData(event.currentTarget as HTMLFormElement)
    );

    if (!resolution.ok) {
      await loadTeamWorkspace({ error: resolution.error });
      return;
    }

    try {
      const result: TeamNamespaceAccessMutationResult =
        await replaceTeamNamespaceAccess(slug, teamSlug, resolution.value.claimId, {
          permissions: resolution.value.permissions,
        });
      await loadTeamWorkspace({
        notice: `Saved namespace access for ${result.namespace_claim?.namespace || 'the selected claim'}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to update namespace access.'),
      });
    }
  }

  async function handleRemoveNamespaceAccess(
    claimId: string,
    namespace: string
  ): Promise<void> {
    try {
      await removeTeamNamespaceAccess(slug, teamSlug, claimId);
      await loadTeamWorkspace({
        notice: `Revoked namespace access for ${namespace}.`,
      });
    } catch (caughtError: unknown) {
      await loadTeamWorkspace({
        error: toErrorMessage(caughtError, 'Failed to revoke namespace access.'),
      });
    }
  }

  function formatPermission(permission: string): string {
    return permission
      .split('_')
      .filter(Boolean)
      .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
      .join(' ');
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }
</script>

<svelte:head>
  <title>{teamName} team — Publaryn</title>
</svelte:head>

{#if loading}
  <div class="loading"><span class="spinner"></span> Loading…</div>
{:else if notFound}
  <div class="empty-state mt-6">
    <h2>Team not found</h2>
    <p>
      {teamSlug} does not exist in {org?.name || org?.slug || slug} or is not visible to
      you.
    </p>
    <div class="empty-actions">
      <a
        href={`/orgs/${encodeURIComponent(slug)}`}
        class="btn btn-primary"
        data-sveltekit-preload-data="hover">Back to organization</a
      >
    </div>
  </div>
{:else if loadError || !org || !team}
  <div class="alert alert-error mt-6">
    {loadError || 'Failed to load the team workspace.'}
  </div>
{:else}
  <div class="page-shell">
    {#if notice}<div class="alert alert-success">{notice}</div>{/if}
    {#if error}<div class="alert alert-error">{error}</div>{/if}

    <nav class="page-breadcrumbs">
      <a href={`/orgs/${encodeURIComponent(slug)}`} data-sveltekit-preload-data="hover"
        >{org.name || org.slug || slug}</a
      >
      <span>&rsaquo;</span>
      <span>{teamName}</span>
    </nav>

    <section class="page-hero">
      <div class="page-hero__header">
        <div class="page-hero__copy">
          <span class="page-hero__eyebrow">
            <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
            Team governance
          </span>
          <h1 class="page-hero__title">{teamName}</h1>
          <p class="page-hero__subtitle">
            {team.description ||
              'Review delegated package, repository, and namespace responsibilities for this organization team.'}
          </p>
          <div class="page-hero__meta">
            <span class="badge badge-ecosystem">@{team.slug || teamSlug}</span>
            <span>Created {formatDate(team.created_at)}</span>
            <span>{org.name || org.slug || slug}</span>
          </div>
        </div>
        <div class="token-row__actions">
          <a
            href={`/orgs/${encodeURIComponent(slug)}`}
            class="btn btn-secondary"
            data-sveltekit-preload-data="hover">Back to organization</a
          >
          <a
            href={teamWorkspaceAnchor}
            class="btn btn-primary"
            data-sveltekit-preload-data="hover">Manage in org workspace</a
          >
        </div>
      </div>
    </section>

    <div class="page-stats">
      <div class="page-stat">
        <div class="page-stat__label">Team members</div>
        <div class="page-stat__value">{formatNumber(members.length)}</div>
      </div>
      <div class="page-stat">
        <div class="page-stat__label">Package grants</div>
        <div class="page-stat__value">{formatNumber(packageAccess.length)}</div>
      </div>
      <div class="page-stat">
        <div class="page-stat__label">Repository grants</div>
        <div class="page-stat__value">{formatNumber(repositoryAccess.length)}</div>
      </div>
      <div class="page-stat">
        <div class="page-stat__label">Namespace grants</div>
        <div class="page-stat__value">{formatNumber(namespaceAccess.length)}</div>
      </div>
    </div>

    <div class="detail-grid">
      <div class="detail-main">
        <section class="surface-card">
          <div class="surface-card__header">
            <h2 class="surface-card__title">Team settings</h2>
            <p class="surface-card__copy">
              Rename the team or clarify its ownership and responsibilities without leaving this
              workspace.
            </p>
          </div>

          <form
            id="team-settings-form"
            class="surface-card__body"
            on:submit={handleUpdateTeam}
          >
            <div class="grid gap-4 xl:grid-cols-2">
              <div class="form-group">
                <label for="team-name">Team name</label>
                <input
                  id="team-name"
                  name="name"
                  class="form-input"
                  value={team.name || ''}
                  required
                />
              </div>
              <div class="form-group">
                <label for="team-slug">Team slug</label>
                <input
                  id="team-slug"
                  class="form-input"
                  value={team.slug || teamSlug}
                  disabled
                />
              </div>
            </div>
            <div class="form-group">
              <label for="team-description">Description</label>
              <textarea
                id="team-description"
                name="description"
                class="form-input"
                rows="3">{team.description || ''}</textarea
              >
            </div>
            <button type="submit" class="btn btn-primary">Save team details</button>
          </form>
        </section>

        <section class="surface-card">
          <div class="surface-card__header">
            <h2 class="surface-card__title">Members</h2>
            <p class="surface-card__copy">
              Team membership is limited to current organization members and controls who inherits
              the delegated grants shown below.
            </p>
          </div>

          <div class="surface-card__body">
            {#if membersError}
              <div class="alert alert-error">{membersError}</div>
            {:else if members.length === 0}
              <div class="empty-state">
                <p>No members belong to this team yet.</p>
              </div>
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
                          id={`team-member-remove-${encodeURIComponent(member.username)}`}
                          class="btn btn-secondary btn-sm"
                          type="button"
                          on:click={() => handleRemoveMember(member.username || '')}
                          >Remove</button
                        >
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}

            {#if orgMembersError}
              <div class="alert alert-error mt-4">{orgMembersError}</div>
            {:else if eligibleTeamMemberOptions.length === 0}
              <p class="settings-copy mt-4">
                Every current organization member is already part of this team.
              </p>
            {:else}
              <form
                id="team-member-form"
                class="settings-subsection"
                on:submit={handleAddTeamMember}
              >
                <div class="form-group">
                  <label for="team-member-input">Add organization member</label>
                  <input
                    id="team-member-input"
                    name="username"
                    class="form-input"
                    list="team-member-options"
                    placeholder="Search username or paste user id"
                    autocomplete="off"
                    required
                  />
                  <datalist id="team-member-options">
                    {#each eligibleTeamMemberOptions as option}
                      <option value={option.username}>{option.label}</option>
                      <option value={option.userId}>{option.label}</option>
                    {/each}
                  </datalist>
                </div>
                <button type="submit" class="btn btn-primary">Add member</button>
              </form>
            {/if}
          </div>
        </section>

        <section class="surface-card">
          <div class="surface-card__header">
            <h2 class="surface-card__title">Package access</h2>
            <p class="surface-card__copy">
              Package grants delegate publish, metadata, security, and transfer workflows without
              changing package ownership.
            </p>
          </div>

          <div class="surface-card__body">
            {#if packageAccessError}
              <div class="alert alert-error">{packageAccessError}</div>
            {:else if packageAccess.length === 0}
              <div class="empty-state">
                <p>No package grants are assigned to this team yet.</p>
              </div>
            {:else}
              <div class="token-list">
                {#each packageAccess as grant}
                  <div class="token-row">
                    <div class="token-row__main">
                      <div class="token-row__title">
                        <a
                          href={`/packages/${encodeURIComponent(grant.ecosystem || 'unknown')}/${encodeURIComponent(grant.name || '')}`}
                          data-sveltekit-preload-data="hover"
                          >{grant.name || 'Unnamed package'}</a
                        >
                      </div>
                      <div class="token-row__meta">
                        <span>{ecosystemLabel(grant.ecosystem)}</span>
                        <span>Granted {formatDate(grant.granted_at)}</span>
                      </div>
                        <div class="token-row__scopes">
                          {#each grant.permissions || [] as permission}
                            <span class="badge badge-ecosystem">{formatPermission(permission)}</span>
                          {/each}
                        </div>
                      </div>
                      {#if grant.ecosystem && grant.name}
                        <div class="token-row__actions">
                          <button
                            id={`team-package-revoke-${encodeURIComponent(`${grant.ecosystem}-${grant.name}`)}`}
                            class="btn btn-secondary btn-sm"
                            type="button"
                            on:click={() =>
                              handleRemovePackageAccess(
                                grant.ecosystem || '',
                                grant.name || ''
                              )}>Revoke</button
                          >
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}

              <TeamAccessGrantForm
                fieldId="team-package-access"
                selectLabel="Organization package"
                selectName="package_key"
                placeholderLabel="Select a package"
                emptyMessage="Create or transfer a package before delegating access."
                submitLabel="Save package access"
                error={orgPackagesError}
                options={packageGrantOptions}
                permissionOptions={TEAM_PERMISSION_OPTIONS}
                handleSubmit={handleReplacePackageAccess}
              />
            </div>
          </section>

        {#if canViewRepositoryAccess}
          <section class="surface-card">
            <div class="surface-card__header">
              <h2 class="surface-card__title">Repository access</h2>
              <p class="surface-card__copy">
                Repository grants apply to all current and future packages inside the selected
                repository.
              </p>
            </div>

            <div class="surface-card__body">
              {#if repositoryAccessError}
                <div class="alert alert-error">{repositoryAccessError}</div>
              {:else if repositoryAccess.length === 0}
                <div class="empty-state">
                  <p>No repository grants are assigned to this team yet.</p>
                </div>
              {:else}
                <div class="token-list">
                  {#each repositoryAccess as grant}
                    <div class="token-row">
                      <div class="token-row__main">
                        <div class="token-row__title">
                          {#if grant.slug}
                            <a
                              href={`/repositories/${encodeURIComponent(grant.slug)}`}
                              data-sveltekit-preload-data="hover"
                              >{grant.name || grant.slug}</a
                            >
                          {:else}
                            {grant.name || 'Unnamed repository'}
                          {/if}
                        </div>
                        <div class="token-row__meta">
                          <span>@{grant.slug || 'no-slug'}</span>
                          <span>{formatRepositoryKindLabel(grant.kind)}</span>
                          <span>{formatRepositoryVisibilityLabel(grant.visibility)}</span>
                          <span>Granted {formatDate(grant.granted_at)}</span>
                        </div>
                        <div class="token-row__scopes">
                          {#each grant.permissions || [] as permission}
                            <span class="badge badge-ecosystem">{formatPermission(permission)}</span>
                          {/each}
                        </div>
                      </div>
                      {#if grant.slug}
                        <div class="token-row__actions">
                          <button
                            id={`team-repository-revoke-${encodeURIComponent(grant.slug)}`}
                            class="btn btn-secondary btn-sm"
                            type="button"
                            on:click={() =>
                              handleRemoveRepositoryAccess(grant.slug || '')}
                            >Revoke</button
                          >
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}

              <TeamAccessGrantForm
                fieldId="team-repository-access"
                selectLabel="Organization repository"
                selectName="repository_slug"
                placeholderLabel="Select a repository"
                emptyMessage="Create a repository before delegating repository-wide access."
                submitLabel="Save repository access"
                error={orgRepositoriesError}
                options={repositoryGrantOptions}
                permissionOptions={TEAM_PERMISSION_OPTIONS}
                handleSubmit={handleReplaceRepositoryAccess}
              />
            </div>
          </section>
        {/if}

        {#if canViewNamespaceAccess}
          <section class="surface-card">
            <div class="surface-card__header">
              <h2 class="surface-card__title">Namespace access</h2>
              <p class="surface-card__copy">
                Namespace grants delegate claim deletion or transfer without granting broader
                organization administration.
              </p>
            </div>

            <div class="surface-card__body">
              {#if namespaceAccessError}
                <div class="alert alert-error">{namespaceAccessError}</div>
              {:else if namespaceAccess.length === 0}
                <div class="empty-state">
                  <p>No namespace grants are assigned to this team yet.</p>
                </div>
              {:else}
                <div class="token-list">
                  {#each namespaceAccess as grant}
                    <div class="token-row">
                      <div class="token-row__main">
                        <div class="token-row__title">
                          {grant.namespace || 'Unnamed namespace claim'}
                        </div>
                        <div class="token-row__meta">
                          <span>{ecosystemLabel(grant.ecosystem)}</span>
                          <span>{grant.is_verified ? 'Verified' : 'Pending verification'}</span>
                          <span>Granted {formatDate(grant.granted_at)}</span>
                        </div>
                        <div class="token-row__scopes">
                          {#each grant.permissions || [] as permission}
                            <span class="badge badge-ecosystem">{formatPermission(permission)}</span>
                          {/each}
                        </div>
                      </div>
                      {#if grant.namespace_claim_id}
                        <div class="token-row__actions">
                          <button
                            id={`team-namespace-revoke-${encodeURIComponent(grant.namespace_claim_id)}`}
                            class="btn btn-secondary btn-sm"
                            type="button"
                            on:click={() =>
                              handleRemoveNamespaceAccess(
                                grant.namespace_claim_id || '',
                                grant.namespace || 'this namespace claim'
                              )}>Revoke</button
                          >
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}

              <TeamAccessGrantForm
                fieldId="team-namespace-access"
                selectLabel="Organization namespace claim"
                selectName="claim_id"
                placeholderLabel="Select a namespace claim"
                emptyMessage="Create or transfer a namespace claim before delegating access."
                submitLabel="Save namespace access"
                error={orgNamespacesError}
                options={namespaceGrantOptions}
                permissionOptions={TEAM_NAMESPACE_PERMISSION_OPTIONS}
                handleSubmit={handleReplaceNamespaceAccess}
              />
            </div>
          </section>
        {/if}
      </div>

      <aside class="detail-sidebar">
        <section class="detail-summary">
          <div class="detail-summary__header">
            <h2 class="detail-summary__title">At a glance</h2>
          </div>
          <p class="detail-summary__copy">
            Delegated access stays scoped to the organization’s current ownership. Transfers revoke
            stale grants automatically so teams do not retain access after ownership changes.
          </p>
        </section>

        <section class="surface-card">
          <div class="surface-card__header">
            <h2 class="surface-card__title">Workspace guidance</h2>
            <p class="surface-card__copy">
              Use the organization workspace for edits, membership changes, and delegated access
              updates.
            </p>
          </div>
          <div class="surface-card__body stack-sm">
            <div class="metadata-block">
              <div class="metadata-block__title">Capabilities</div>
              <div class="token-row__scopes">
                <span class="badge badge-ecosystem">Team admin</span>
                <span class="badge badge-ecosystem">Package delegation</span>
                {#if canViewRepositoryAccess}
                  <span class="badge badge-ecosystem">Repository delegation</span>
                {/if}
                {#if canViewNamespaceAccess}
                  <span class="badge badge-ecosystem">Namespace delegation</span>
                {/if}
              </div>
            </div>
            <div class="metadata-block">
              <div class="metadata-block__title">Eligible members</div>
              <div class="metadata-block__copy">
                {formatNumber(eligibleTeamMemberOptions.length)} organization members can be added
                without leaving this page.
              </div>
            </div>
            <a
              href={teamWorkspaceAnchor}
              class="btn btn-primary"
              data-sveltekit-preload-data="hover">Open editable team section</a
            >
          </div>
        </section>
      </aside>
    </div>
  </div>
{/if}
