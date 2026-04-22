<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';

  import { ApiError } from '../../../../../api/client';
  import type { NamespaceClaim, NamespaceListResponse } from '../../../../../api/namespaces';
  import type {
    OrgMember,
    OrgPackageSummary,
    OrgRepositorySummary,
    Team,
    TeamMember,
    TeamNamespaceAccessGrant,
    TeamPackageAccessGrant,
    TeamRepositoryAccessGrant,
  } from '../../../../../api/orgs';
  import {
    deleteTeam,
    getOrg,
    listTeams,
  } from '../../../../../api/orgs';
  import TeamMembersEditor from '../../../../../lib/components/TeamMembersEditor.svelte';
  import TeamNamespaceAccessEditor from '../../../../../lib/components/TeamNamespaceAccessEditor.svelte';
  import TeamPackageAccessEditor from '../../../../../lib/components/TeamPackageAccessEditor.svelte';
  import TeamRepositoryAccessEditor from '../../../../../lib/components/TeamRepositoryAccessEditor.svelte';
  import TeamSettingsEditor from '../../../../../lib/components/TeamSettingsEditor.svelte';
  import {
    buildEligibleTeamMemberOptions,
    buildNamespaceGrantOptions,
    buildPackageGrantOptions,
    buildRepositoryGrantOptions,
    createTeamManagementController,
    loadOrgTeamReferenceData,
    loadSingleTeamManagementState,
    TEAM_DELETE_CONFIRMATION_MESSAGE,
    TEAM_NAMESPACE_PERMISSION_OPTIONS,
    TEAM_PERMISSION_OPTIONS,
  } from '../../../../../pages/team-management';
  import {
    canManageOrgNamespaces,
    canManageOrgRepositories,
    canManageOrgTeams,
  } from '../../../../../pages/org-workspace-access';
  import { formatDate, formatNumber } from '../../../../../utils/format';

  let lastLoadKey = '';
  let loading = true;
  let loadError: string | null = null;
  let notice: string | null = null;
  let error: string | null = null;
  let notFound = false;
  let deleteConfirmed = false;
  let deletingTeam = false;

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
  $: packageGrantOptions = buildPackageGrantOptions(orgPackages);
  $: repositoryGrantOptions = buildRepositoryGrantOptions(orgRepositories);
  $: namespaceGrantOptions = buildNamespaceGrantOptions(orgNamespaces);
  $: eligibleTeamMemberOptions = buildEligibleTeamMemberOptions(orgMembers, members);

  async function loadTeamWorkspace(
    options: { notice?: string | null; error?: string | null } = {}
  ): Promise<void> {
    loading = true;
    loadError = null;
    notice = options.notice ?? null;
    error = options.error ?? null;
    notFound = false;
    deleteConfirmed = false;
    deletingTeam = false;
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

      const [singleTeamState, orgReferenceData] =
        await Promise.all([
          loadSingleTeamManagementState(slug, team, {
            includeRepositoryAccess: canManageOrgRepositories(loadedOrg),
            includeNamespaceAccess: canManageOrgNamespaces(loadedOrg),
            toErrorMessage,
          }),
          loadOrgTeamReferenceData(slug, {
            orgId: loadedOrg.id,
            includeMembers: true,
            includePackages: true,
            includeRepositories: canManageOrgRepositories(loadedOrg),
            includeNamespaces: canManageOrgNamespaces(loadedOrg),
            memberErrorMessage: 'Failed to load organization members.',
            packageErrorMessage: 'Failed to load organization packages.',
            repositoryErrorMessage: 'Failed to load organization repositories.',
            namespaceErrorMessage: 'Failed to load organization namespace claims.',
            missingNamespaceOrgIdMessage:
              'Failed to load namespace claims because the organization id is unavailable.',
            toErrorMessage,
          }),
        ]);

      orgMembers = orgReferenceData.members;
      orgMembersError = orgReferenceData.membersError;
      orgPackages = orgReferenceData.packages;
      orgPackagesError = orgReferenceData.packagesError;
      orgRepositories = orgReferenceData.repositories;
      orgRepositoriesError = orgReferenceData.repositoriesError;
      orgNamespaces = orgReferenceData.namespaces;
      orgNamespacesError = orgReferenceData.namespacesError;
      members = singleTeamState.members;
      membersError = singleTeamState.membersError;
      packageAccess = singleTeamState.packageAccess;
      packageAccessError = singleTeamState.packageAccessError;
      repositoryAccess = singleTeamState.repositoryAccess;
      repositoryAccessError = singleTeamState.repositoryAccessError;
      namespaceAccess = singleTeamState.namespaceAccess;
      namespaceAccessError = singleTeamState.namespaceAccessError;
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

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }

  async function handleDeleteTeam(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!team) {
      return;
    }

    if (!deleteConfirmed) {
      notice = null;
      error = TEAM_DELETE_CONFIRMATION_MESSAGE;
      return;
    }

    deletingTeam = true;
    notice = null;
    error = null;

    const resolvedTeamSlug = team.slug?.trim() || teamSlug;

    try {
      await deleteTeam(slug, resolvedTeamSlug);
      const params = new URLSearchParams({
        notice: `Deleted team ${resolvedTeamSlug}.`,
      });
      await goto(`/orgs/${encodeURIComponent(slug)}?${params.toString()}`);
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to delete team.');
      deletingTeam = false;
    }
  }

  const teamManagement = createTeamManagementController({
    getOrgSlug: () => slug,
    reload: loadTeamWorkspace,
    resolveEligibleTeamMemberOptions: () => eligibleTeamMemberOptions,
    toErrorMessage,
  });
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

          <TeamSettingsEditor
            {team}
            {teamSlug}
            formId="team-settings-form"
            formClass="surface-card__body"
            showSlugField={true}
              handleSubmit={(event) => teamManagement.updateTeam(teamSlug, event)}
          />
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
            <TeamMembersEditor
              {members}
              membersError={membersError}
              eligibleOptions={eligibleTeamMemberOptions}
              eligibleOptionsError={orgMembersError}
              formId="team-member-form"
              inputId="team-member-input"
              datalistId="team-member-options"
              handleSubmit={(event) => teamManagement.addTeamMember(teamSlug, event)}
              handleRemoveMember={(username) =>
                teamManagement.removeTeamMember(teamSlug, username)}
            />
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
            <TeamPackageAccessEditor
              grants={packageAccess}
              grantsError={packageAccessError}
              optionsError={orgPackagesError}
              options={packageGrantOptions}
              permissionOptions={TEAM_PERMISSION_OPTIONS}
              fieldId="team-package-access"
              handleSubmit={(event) =>
                teamManagement.replaceTeamPackageAccess(teamSlug, event)}
              handleRevoke={(ecosystem, packageName) =>
                teamManagement.removeTeamPackageAccess(
                  teamSlug,
                  ecosystem,
                  packageName
                )}
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
              <TeamRepositoryAccessEditor
                grants={repositoryAccess}
                grantsError={repositoryAccessError}
                optionsError={orgRepositoriesError}
                options={repositoryGrantOptions}
                permissionOptions={TEAM_PERMISSION_OPTIONS}
                fieldId="team-repository-access"
                handleSubmit={(event) =>
                  teamManagement.replaceTeamRepositoryAccess(teamSlug, event)}
                handleRevoke={(repositorySlug) =>
                  teamManagement.removeTeamRepositoryAccess(
                    teamSlug,
                    repositorySlug
                  )}
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
              <TeamNamespaceAccessEditor
                grants={namespaceAccess}
                grantsError={namespaceAccessError}
                optionsError={orgNamespacesError}
                options={namespaceGrantOptions}
                permissionOptions={TEAM_NAMESPACE_PERMISSION_OPTIONS}
                fieldId="team-namespace-access"
                handleSubmit={(event) =>
                  teamManagement.replaceTeamNamespaceAccess(teamSlug, event)}
                handleRevoke={(claimId, namespace) =>
                  teamManagement.removeTeamNamespaceAccess(
                    teamSlug,
                    claimId,
                    namespace
                  )}
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
              Use this page for focused team edits. For broader org-level context, go back to the
              organization workspace.
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
              data-sveltekit-preload-data="hover">Manage in org workspace</a
            >
          </div>
        </section>

        <section class="surface-card">
          <div class="surface-card__header">
            <h2 class="surface-card__title">Danger zone</h2>
            <p class="surface-card__copy">
              Deleting a team removes its memberships and all delegated package, repository, and
              namespace grants immediately.
            </p>
          </div>
          <form class="surface-card__body stack-sm" id="team-delete-form" on:submit={handleDeleteTeam}>
            <label class="flex items-start gap-2" for="team-delete-confirm">
              <input
                id="team-delete-confirm"
                bind:checked={deleteConfirmed}
                type="checkbox"
                name="confirm_delete"
                disabled={deletingTeam}
              />
              <span>
                I understand deleting this team revokes its delegated access and cannot be undone.
              </span>
            </label>
            <button
              id="team-delete-submit"
              type="submit"
              class="btn btn-danger"
              disabled={deletingTeam}
            >
              {deletingTeam ? 'Deleting…' : 'Delete team'}
            </button>
          </form>
        </section>
      </aside>
    </div>
  </div>
{/if}
