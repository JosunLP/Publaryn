<script lang="ts">
  import { page } from '$app/stores';

  import { ApiError } from '../../../../../api/client';
  import type {
    Team,
    TeamMember,
    TeamMemberListResponse,
    TeamNamespaceAccessGrant,
    TeamNamespaceAccessListResponse,
    TeamPackageAccessGrant,
    TeamPackageAccessListResponse,
    TeamRepositoryAccessGrant,
    TeamRepositoryAccessListResponse,
  } from '../../../../../api/orgs';
  import {
    getOrg,
    listTeamMembers,
    listTeamNamespaceAccess,
    listTeamPackageAccess,
    listTeamRepositoryAccess,
    listTeams,
  } from '../../../../../api/orgs';
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

  let lastLoadKey = '';
  let loading = true;
  let loadError: string | null = null;
  let notFound = false;

  let org:
    | Awaited<ReturnType<typeof getOrg>>
    | null = null;
  let team: Team | null = null;
  let members: TeamMember[] = [];
  let packageAccess: TeamPackageAccessGrant[] = [];
  let repositoryAccess: TeamRepositoryAccessGrant[] = [];
  let namespaceAccess: TeamNamespaceAccessGrant[] = [];
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
  $: canManageTeamWorkspace = canManageOrgTeams(org);
  $: canViewRepositoryAccess = canManageOrgRepositories(org);
  $: canViewNamespaceAccess = canManageOrgNamespaces(org);

  async function loadTeamWorkspace(): Promise<void> {
    loading = true;
    loadError = null;
    notFound = false;
    org = null;
    team = null;
    members = [];
    packageAccess = [];
    repositoryAccess = [];
    namespaceAccess = [];
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

      const [memberData, packageData, repositoryData, namespaceData] =
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
        ]);

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
            href={`/orgs/${encodeURIComponent(slug)}#team-${encodeURIComponent(team.slug || teamSlug)}`}
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
                  </div>
                {/each}
              </div>
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
                        <span>{grant.ecosystem || 'unknown'}</span>
                        <span>Granted {formatDate(grant.granted_at)}</span>
                      </div>
                      <div class="token-row__scopes">
                        {#each grant.permissions || [] as permission}
                          <span class="badge badge-ecosystem">{formatPermission(permission)}</span>
                        {/each}
                      </div>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
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
                    </div>
                  {/each}
                </div>
              {/if}
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
                    </div>
                  {/each}
                </div>
              {/if}
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
            <a
              href={`/orgs/${encodeURIComponent(slug)}#team-${encodeURIComponent(team.slug || teamSlug)}`}
              class="btn btn-primary"
              data-sveltekit-preload-data="hover">Open editable team section</a
            >
          </div>
        </section>
      </aside>
    </div>
  </div>
{/if}
