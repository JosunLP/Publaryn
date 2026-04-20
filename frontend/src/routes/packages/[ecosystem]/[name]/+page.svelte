<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';

  import { ApiError, getAuthToken } from '../../../../api/client';
  import type { OrganizationMembership, Team } from '../../../../api/orgs';
  import {
    listMyOrganizations,
    listTeams,
    removeTeamPackageAccess,
    replaceTeamPackageAccess,
  } from '../../../../api/orgs';
  import type {
    PackageDetail,
    Release,
    SecurityFinding,
    Tag,
    TrustedPublisher,
  } from '../../../../api/packages';
  import {
    createRelease,
    createTrustedPublisher as createTrustedPublisherForPackage,
    deleteTrustedPublisher as deleteTrustedPublisherForPackage,
    getPackage,
    listReleases,
    listSecurityFindings,
    listTags,
    listTrustedPublishers as listTrustedPublishersForPackage,
    severityLevel,
    transferPackageOwnership,
    updatePackage,
    updateSecurityFinding,
  } from '../../../../api/packages';
  import type { PackageDetailTab } from '../../../../pages/package-detail-tabs';
  import {
    buildPackageDetailPath,
    getPackageDetailTabFromQuery,
  } from '../../../../pages/package-detail-tabs';
  import {
    ecosystemIcon,
    ecosystemLabel,
    formatVersionLabel,
    installCommand,
  } from '../../../../utils/ecosystem';
  import {
    copyToClipboard,
    formatDate,
    formatNumber,
  } from '../../../../utils/format';
  import { renderMarkdown } from '../../../../utils/markdown';
  import {
    buildPackageMetadataUpdateInput,
    createPackageMetadataFormValues,
    packageMetadataHasChanges,
  } from '../../../../utils/package-metadata';
  import { selectPackageTransferTargets } from '../../../../utils/package-transfer';
  import {
    normalizeTrustedPublisherInput,
    trustedPublisherBindingFields,
    trustedPublisherHeading,
  } from '../../../../utils/trusted-publishers';

  interface TransferState {
    showTransfer: boolean;
    organizations: OrganizationMembership[];
    loadError: string | null;
  }

  interface TrustedPublisherState {
    publishers: TrustedPublisher[];
    loadError: string | null;
  }

  interface TeamAccessManagementState {
    showManagement: boolean;
    teams: Team[];
    loadError: string | null;
  }

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

  let lastLoadKey = '';
  let loading = true;
  let notFound = false;
  let error: string | null = null;
  let pkg: PackageDetail | null = null;
  let releases: Release[] = [];
  let tags: Tag[] = [];
  let findings: SecurityFinding[] = [];
  let transferState: TransferState = {
    showTransfer: false,
    organizations: [],
    loadError: null,
  };
  let trustedPublisherState: TrustedPublisherState = {
    publishers: [],
    loadError: null,
  };
  let teamAccessManagementState: TeamAccessManagementState = {
    showManagement: false,
    teams: [],
    loadError: null,
  };
  let releaseNotice: string | null = null;
  let releaseError: string | null = null;
  let transferNotice: string | null = null;
  let transferError: string | null = null;
  let teamAccessNotice: string | null = null;
  let teamAccessError: string | null = null;
  let packageSettingsNotice: string | null = null;
  let packageSettingsError: string | null = null;
  let trustedPublisherNotice: string | null = null;
  let trustedPublisherError: string | null = null;
  let includeResolvedFindings = false;
  let activeTab: PackageDetailTab = 'readme';
  let findingsNotice: string | null = null;
  let findingsError: string | null = null;
  let updatingFindingId: string | null = null;
  let findingNotes: Record<string, string> = {};

  let newReleaseVersion = '';
  let newReleaseDescription = '';
  let newReleaseChangelog = '';
  let newReleaseSourceRef = '';
  let newReleaseIsPrerelease = false;
  let creatingRelease = false;

  let targetOrgSlug = '';
  let transferConfirmed = false;
  let transferringPackage = false;

  let packageSettingsDescription = '';
  let packageSettingsReadme = '';
  let packageSettingsHomepage = '';
  let packageSettingsRepositoryUrl = '';
  let packageSettingsLicense = '';
  let packageSettingsKeywords = '';
  let updatingPackageSettings = false;

  let trustedPublisherIssuer = '';
  let trustedPublisherSubject = '';
  let trustedPublisherRepository = '';
  let trustedPublisherWorkflowRef = '';
  let trustedPublisherEnvironment = '';
  let creatingTrustedPublisher = false;
  let deletingTrustedPublisherId: string | null = null;
  let savingTeamAccess = false;
  let revokingTeamSlug: string | null = null;

  $: ecosystem = $page.params.ecosystem ?? '';
  $: name = $page.params.name ?? '';
  $: activeTab = getPackageDetailTabFromQuery($page.url.searchParams);
  $: loadKey = `${ecosystem}|${name}`;
  $: if (ecosystem && name && loadKey !== lastLoadKey) {
    lastLoadKey = loadKey;
    void loadPackagePage();
  }

  async function loadPackagePage(): Promise<void> {
    loading = true;
    notFound = false;
    error = null;
    pkg = null;
    releases = [];
    tags = [];
    findings = [];
    transferState = {
      showTransfer: false,
      organizations: [],
      loadError: null,
    };
    trustedPublisherState = {
      publishers: [],
      loadError: null,
    };
    teamAccessManagementState = {
      showManagement: false,
      teams: [],
      loadError: null,
    };
    releaseNotice = null;
    releaseError = null;
    transferNotice = null;
    transferError = null;
    teamAccessNotice = null;
    teamAccessError = null;
    packageSettingsNotice = null;
    packageSettingsError = null;
    trustedPublisherNotice = null;
    trustedPublisherError = null;
    includeResolvedFindings = false;
    resetReleaseForm();
    resetTransferForm();
    resetPackageSettingsForm();
    resetTrustedPublisherForm();

    try {
      pkg = await getPackage(ecosystem, name);
      resetPackageSettingsForm(pkg);
    } catch (caughtError: unknown) {
      if (caughtError instanceof ApiError && caughtError.status === 404) {
        notFound = true;
      } else {
        error =
          caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Failed to load package.';
      }
      loading = false;
      return;
    }

    const [
      loadedReleases,
      loadedTags,
      loadedFindings,
      loadedTransferState,
      loadedTrustedPublisherState,
      loadedTeamAccessManagementState,
    ] = await Promise.all([
      listReleases(ecosystem, name, { perPage: 20 }).catch(
        () => [] as Release[]
      ),
      listTags(ecosystem, name).catch(() => [] as Tag[]),
      listSecurityFindings(ecosystem, name).catch(
        () => [] as SecurityFinding[]
      ),
      loadTransferState(pkg),
      loadTrustedPublisherState(pkg),
      loadTeamAccessManagementState(pkg),
    ]);

    releases = loadedReleases;
    tags = loadedTags;
    findings = loadedFindings;
    transferState = loadedTransferState;
    trustedPublisherState = loadedTrustedPublisherState;
    teamAccessManagementState = loadedTeamAccessManagementState;
    loading = false;
  }

  async function loadTransferState(
    currentPackage: PackageDetail | null
  ): Promise<TransferState> {
    if (
      !currentPackage ||
      !getAuthToken() ||
      currentPackage.can_transfer !== true
    ) {
      return {
        showTransfer: false,
        organizations: [],
        loadError: null,
      };
    }

    try {
      const response = await listMyOrganizations();
      return {
        showTransfer: true,
        organizations: selectPackageTransferTargets(
          response.organizations || [],
          currentPackage.owner_org_slug
        ),
        loadError: null,
      };
    } catch (caughtError: unknown) {
      return {
        showTransfer: true,
        organizations: [],
        loadError:
          caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Failed to load your organizations for package transfer.',
      };
    }
  }

  async function handleUpdatePackage(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!pkg || pkg.can_manage_metadata !== true) {
      return;
    }

    const input = buildPackageMetadataUpdateInput(pkg, {
      description: packageSettingsDescription,
      readme: packageSettingsReadme,
      homepage: packageSettingsHomepage,
      repositoryUrl: packageSettingsRepositoryUrl,
      license: packageSettingsLicense,
      keywords: packageSettingsKeywords,
    });

    if (Object.keys(input).length === 0) {
      packageSettingsError = 'No metadata changes to save.';
      packageSettingsNotice = null;
      return;
    }

    updatingPackageSettings = true;
    packageSettingsError = null;
    packageSettingsNotice = null;

    try {
      const result = await updatePackage(ecosystem, name, input);
      await loadPackagePage();
      packageSettingsNotice = result.message || 'Package updated';
    } catch (caughtError: unknown) {
      packageSettingsError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to update package settings.';
    } finally {
      updatingPackageSettings = false;
    }
  }

  async function loadTrustedPublisherState(
    currentPackage: PackageDetail | null
  ): Promise<TrustedPublisherState> {
    if (!currentPackage || ecosystem !== 'pypi') {
      return {
        publishers: [],
        loadError: null,
      };
    }

    try {
      return {
        publishers: await listTrustedPublishersForPackage(ecosystem, name),
        loadError: null,
      };
    } catch (caughtError: unknown) {
      return {
        publishers: [],
        loadError:
          caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Failed to load trusted publishers.',
      };
    }
  }

  async function loadTeamAccessManagementState(
    currentPackage: PackageDetail | null
  ): Promise<TeamAccessManagementState> {
    if (
      !currentPackage ||
      !getAuthToken() ||
      !currentPackage.owner_org_slug
    ) {
      return {
        showManagement: false,
        teams: [],
        loadError: null,
      };
    }

    try {
      const organizations = await listMyOrganizations();
      const membership = (organizations.organizations || []).find(
        (org) => org.slug === currentPackage.owner_org_slug
      );

      if (!membership || !isOrgAdminRole(membership.role)) {
        return {
          showManagement: false,
          teams: [],
          loadError: null,
        };
      }

      try {
        const teamResponse = await listTeams(currentPackage.owner_org_slug);
        return {
          showManagement: true,
          teams: (teamResponse.teams || []).filter((team) => Boolean(team.slug)),
          loadError: teamResponse.load_error || null,
        };
      } catch (caughtError: unknown) {
        return {
          showManagement: true,
          teams: [],
          loadError: toErrorMessage(caughtError, 'Failed to load teams.'),
        };
      }
    } catch {
      return {
        showManagement: false,
        teams: [],
        loadError: null,
      };
    }
  }

  async function handleCopyInstall(): Promise<void> {
    if (!pkg) {
      return;
    }

    const latestVersion =
      pkg.latest_version ??
      (releases.length > 0 ? (releases[0]?.version ?? null) : null);
    const command = latestVersion
      ? installCommand(ecosystem, pkg.name, latestVersion)
      : installCommand(ecosystem, pkg.name);
    await copyToClipboard(command);
  }

  async function handlePackageTabChange(tab: PackageDetailTab): Promise<void> {
    if (tab === activeTab) {
      return;
    }

    await goto(
      buildPackageDetailPath(ecosystem, name, { tab }, $page.url.searchParams)
    );
  }

  async function handleResolvedToggleChange(): Promise<void> {
    try {
      findings = await listSecurityFindings(ecosystem, name, {
        includeResolved: includeResolvedFindings,
      });
    } catch {
      findings = [];
    }
  }

  async function handleToggleFindingResolution(
    finding: SecurityFinding
  ): Promise<void> {
    if (updatingFindingId) {
      return;
    }
    const targetIsResolved = !finding.is_resolved;
    updatingFindingId = finding.id;
    findingsError = null;
    findingsNotice = null;
    const rawNote = findingNotes[finding.id] ?? '';
    const trimmedNote = rawNote.trim();
    if (trimmedNote.length > 2000) {
      findingsError = 'Security finding note must be 2000 characters or fewer.';
      updatingFindingId = null;
      return;
    }
    try {
      const updated = await updateSecurityFinding(ecosystem, name, finding.id, {
        isResolved: targetIsResolved,
        note: trimmedNote.length > 0 ? trimmedNote : undefined,
      });
      findings = findings.map((current) =>
        current.id === updated.id ? { ...current, ...updated } : current
      );
      if (!includeResolvedFindings && updated.is_resolved) {
        // Remove newly-resolved finding when resolved findings are hidden.
        findings = findings.filter((current) => !current.is_resolved);
      }
      findingNotes = { ...findingNotes, [finding.id]: '' };
      findingsNotice = targetIsResolved
        ? 'Finding marked as resolved.'
        : 'Finding reopened.';
    } catch (err) {
      findingsError =
        err instanceof ApiError
          ? err.message
          : 'Failed to update the security finding.';
    } finally {
      updatingFindingId = null;
    }
  }

  async function handleCreateRelease(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!newReleaseVersion.trim()) {
      releaseError = 'Enter a version to create a release.';
      releaseNotice = null;
      return;
    }

    creatingRelease = true;
    releaseError = null;
    releaseNotice = null;

    try {
      await createRelease(ecosystem, name, {
        version: newReleaseVersion.trim(),
        description: optional(newReleaseDescription),
        changelog: optional(newReleaseChangelog),
        sourceRef: optional(newReleaseSourceRef),
        isPrerelease: newReleaseIsPrerelease || undefined,
      });

      const notice = encodeURIComponent(
        `Release ${newReleaseVersion.trim()} created in quarantine. Upload at least one artifact before publishing.`
      );
      await goto(
        `/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}/versions/${encodeURIComponent(newReleaseVersion.trim())}?notice=${notice}`
      );
    } catch (caughtError: unknown) {
      releaseError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to create release.';
    } finally {
      creatingRelease = false;
    }
  }

  async function handleReplacePackageTeamAccess(
    event: SubmitEvent
  ): Promise<void> {
    event.preventDefault();

    if (
      !pkg ||
      !pkg.owner_org_slug ||
      !teamAccessManagementState.showManagement ||
      savingTeamAccess
    ) {
      return;
    }

    const form = event.currentTarget as HTMLFormElement;
    const formData = new FormData(form);
    const teamSlug = formData.get('team_slug')?.toString().trim() || '';
    const permissions = formData
      .getAll('permissions')
      .map((value) => value.toString().trim())
      .filter(Boolean);

    if (!teamSlug) {
      teamAccessError = 'Select a team to manage package access.';
      teamAccessNotice = null;
      return;
    }

    if (permissions.length === 0) {
      teamAccessError = 'Select at least one delegated package permission.';
      teamAccessNotice = null;
      return;
    }

    savingTeamAccess = true;
    teamAccessError = null;
    teamAccessNotice = null;

    try {
      await replaceTeamPackageAccess(pkg.owner_org_slug, teamSlug, ecosystem, name, {
        permissions,
      });
      const teamLabel =
        teamAccessManagementState.teams.find((team) => team.slug === teamSlug)
          ?.name ||
        teamSlug;
      await loadPackagePage();
      teamAccessNotice = `Saved package access for ${teamLabel}.`;
      form.reset();
    } catch (caughtError: unknown) {
      teamAccessError = toErrorMessage(
        caughtError,
        'Failed to update package access.'
      );
    } finally {
      savingTeamAccess = false;
    }
  }

  async function handleRemovePackageTeamAccess(teamSlug: string): Promise<void> {
    if (
      !pkg ||
      !pkg.owner_org_slug ||
      !teamAccessManagementState.showManagement ||
      revokingTeamSlug
    ) {
      return;
    }

    revokingTeamSlug = teamSlug;
    teamAccessError = null;
    teamAccessNotice = null;

    try {
      await removeTeamPackageAccess(pkg.owner_org_slug, teamSlug, ecosystem, name);
      const teamLabel =
        pkg.team_access?.find((grant) => grant.team_slug === teamSlug)?.team_name ||
        teamSlug;
      await loadPackagePage();
      teamAccessNotice = `Revoked package access for ${teamLabel}.`;
    } catch (caughtError: unknown) {
      teamAccessError = toErrorMessage(
        caughtError,
        'Failed to revoke package access.'
      );
    } finally {
      revokingTeamSlug = null;
    }
  }

  async function handleTransferPackage(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!targetOrgSlug.trim()) {
      transferError = 'Select an organization to receive this package.';
      transferNotice = null;
      return;
    }

    if (!transferConfirmed) {
      transferError =
        'Please confirm that you understand this transfer is immediate and revokes existing team grants.';
      transferNotice = null;
      return;
    }

    transferringPackage = true;
    transferError = null;
    transferNotice = null;

    try {
      const result = await transferPackageOwnership(ecosystem, name, {
        targetOrgSlug: targetOrgSlug.trim(),
      });
      transferNotice = `Package ownership transferred to ${result.owner?.name || result.owner?.slug || targetOrgSlug.trim()}.`;
      await loadPackagePage();
    } catch (caughtError: unknown) {
      transferError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to transfer package ownership.';
    } finally {
      transferringPackage = false;
    }
  }

  async function handleCreateTrustedPublisher(
    event: SubmitEvent
  ): Promise<void> {
    event.preventDefault();

    let input;
    try {
      input = normalizeTrustedPublisherInput({
        issuer: trustedPublisherIssuer,
        subject: trustedPublisherSubject,
        repository: trustedPublisherRepository,
        workflowRef: trustedPublisherWorkflowRef,
        environment: trustedPublisherEnvironment,
      });
    } catch (caughtError: unknown) {
      trustedPublisherError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Issuer and subject are required.';
      trustedPublisherNotice = null;
      return;
    }

    creatingTrustedPublisher = true;
    trustedPublisherError = null;
    trustedPublisherNotice = null;

    try {
      await createTrustedPublisherForPackage(ecosystem, name, input);
      trustedPublisherState = await loadTrustedPublisherState(pkg);
      trustedPublisherNotice = 'Trusted publisher added.';
      resetTrustedPublisherForm();
    } catch (caughtError: unknown) {
      trustedPublisherError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to add trusted publisher.';
    } finally {
      creatingTrustedPublisher = false;
    }
  }

  async function handleDeleteTrustedPublisher(
    publisher: TrustedPublisher
  ): Promise<void> {
    if (!publisher.id) {
      return;
    }

    const confirmed = window.confirm(
      `Remove trusted publisher ${trustedPublisherHeading(publisher)}?`
    );
    if (!confirmed) {
      return;
    }

    deletingTrustedPublisherId = publisher.id;
    trustedPublisherError = null;
    trustedPublisherNotice = null;

    try {
      await deleteTrustedPublisherForPackage(ecosystem, name, publisher.id);
      trustedPublisherState = await loadTrustedPublisherState(pkg);
      trustedPublisherNotice = 'Trusted publisher removed.';
    } catch (caughtError: unknown) {
      trustedPublisherError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to remove trusted publisher.';
    } finally {
      deletingTrustedPublisherId = null;
    }
  }

  function resetReleaseForm(): void {
    newReleaseVersion = '';
    newReleaseDescription = '';
    newReleaseChangelog = '';
    newReleaseSourceRef = '';
    newReleaseIsPrerelease = false;
    creatingRelease = false;
  }

  function resetTransferForm(): void {
    targetOrgSlug = '';
    transferConfirmed = false;
    transferringPackage = false;
  }

  function resetPackageSettingsForm(
    currentPackage: PackageDetail | null = null
  ): void {
    const values = createPackageMetadataFormValues(currentPackage);
    packageSettingsDescription = values.description;
    packageSettingsReadme = values.readme;
    packageSettingsHomepage = values.homepage;
    packageSettingsRepositoryUrl = values.repositoryUrl;
    packageSettingsLicense = values.license;
    packageSettingsKeywords = values.keywords;
    updatingPackageSettings = false;
  }

  function resetTrustedPublisherForm(): void {
    trustedPublisherIssuer = '';
    trustedPublisherSubject = '';
    trustedPublisherRepository = '';
    trustedPublisherWorkflowRef = '';
    trustedPublisherEnvironment = '';
    creatingTrustedPublisher = false;
    deletingTrustedPublisherId = null;
  }

  function optional(value: string): string | undefined {
    const trimmed = value.trim();
    return trimmed ? trimmed : undefined;
  }

  function latestVersionForPackage(
    currentPackage: PackageDetail
  ): string | null {
    return (
      currentPackage.latest_version ??
      (releases.length > 0 ? (releases[0]?.version ?? null) : null)
    );
  }

  function worstSeverity(openFindings: SecurityFinding[]): string {
    if (openFindings.length === 0) {
      return 'info';
    }

    let worst = 'info';
    let worstLevel = -1;
    for (const finding of openFindings) {
      const level = severityLevel(finding.severity);
      if (level > worstLevel) {
        worstLevel = level;
        worst = finding.severity.toLowerCase();
      }
    }

    return worst;
  }

  function formatFindingKind(kind: string): string {
    return kind
      .replace(/_/g, ' ')
      .replace(/\b\w/g, (char) => char.toUpperCase());
  }

  function formatIdentifierLabel(value: string): string {
    return value
      .split('_')
      .filter(Boolean)
      .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
      .join(' ');
  }

  function formatPermission(permission: string): string {
    return formatIdentifierLabel(permission);
  }

  function isOrgAdminRole(role: string | null | undefined): boolean {
    return role === 'owner' || role === 'admin';
  }

  function formatTeamOption(team: Team): string {
    const name = team.name?.trim();
    const slug = team.slug?.trim();

    if (name && slug && name !== slug) {
      return `${name} (${slug})`;
    }

    return name || slug || 'Unnamed team';
  }

  function toErrorMessage(error: unknown, fallback: string): string {
    return error instanceof Error && error.message ? error.message : fallback;
  }

  $: packageMetadata = pkg?.ecosystem_metadata ?? null;
  $: canonicalPackageEcosystem = pkg?.ecosystem ?? ecosystem;
  $: showsRegistryFamily = Boolean(
    pkg?.ecosystem && pkg.ecosystem.toLowerCase() !== ecosystem.toLowerCase()
  );
  $: npmPackageMetadata =
    packageMetadata?.kind === 'npm' || packageMetadata?.kind === 'bun'
      ? packageMetadata.details
      : null;
  $: pypiPackageMetadata =
    packageMetadata?.kind === 'pypi' ? packageMetadata.details : null;
  $: cargoPackageMetadata =
    packageMetadata?.kind === 'cargo' ? packageMetadata.details : null;
  $: nugetPackageMetadata =
    packageMetadata?.kind === 'nuget' ? packageMetadata.details : null;
  $: rubygemsPackageMetadata =
    packageMetadata?.kind === 'rubygems' ? packageMetadata.details : null;
  $: composerPackageMetadata =
    packageMetadata?.kind === 'composer' ? packageMetadata.details : null;
  $: mavenPackageMetadata =
    packageMetadata?.kind === 'maven' ? packageMetadata.details : null;
  $: ociPackageMetadata =
    packageMetadata?.kind === 'oci' ? packageMetadata.details : null;
  $: openFindings = findings.filter((finding) => !finding.is_resolved);
  $: packageSettingsHasChanges = pkg
    ? packageMetadataHasChanges(pkg, {
        description: packageSettingsDescription,
        readme: packageSettingsReadme,
        homepage: packageSettingsHomepage,
        repositoryUrl: packageSettingsRepositoryUrl,
        license: packageSettingsLicense,
        keywords: packageSettingsKeywords,
      })
    : false;
  $: readmeHtml = renderMarkdown(pkg?.readme);
</script>

<svelte:head>
  <title>Package details — Publaryn</title>
</svelte:head>

{#if loading}
  <div class="loading"><span class="spinner"></span> Loading…</div>
{:else if notFound}
  <div class="empty-state mt-6">
    <h2>Package not found</h2>
    <p>{ecosystem}/{name} does not exist or is not public.</p>
    <a
      href="/search"
      class="btn btn-primary mt-4"
      data-sveltekit-preload-data="hover">Search packages</a
    >
  </div>
{:else if error || !pkg}
  <div class="alert alert-error mt-6">
    Failed to load package: {error || 'Unknown error.'}
  </div>
{:else}
  {@const latestVersion = latestVersionForPackage(pkg)}
  {@const install = latestVersion
    ? installCommand(ecosystem, pkg.name, latestVersion)
    : installCommand(ecosystem, pkg.name)}

  <div class="mt-6">
    <div class="pkg-header">
      <h1 class="pkg-header__name">{pkg.display_name || pkg.name}</h1>
      <span class="badge badge-ecosystem"
        >{ecosystemIcon(ecosystem)} {ecosystemLabel(ecosystem)}</span
      >
      {#if latestVersion}
        <span class="pkg-header__version"
          >{formatVersionLabel(ecosystem, latestVersion)}</span
        >
      {/if}
      {#if pkg.is_deprecated}
        <span class="badge badge-deprecated">deprecated</span>
      {/if}
      {#if pkg.is_archived}
        <span class="badge badge-yanked">archived</span>
      {/if}
    </div>

    {#if pkg.description}
      <p class="text-muted mt-4" style="font-size:1.05rem;">
        {pkg.description}
      </p>
    {/if}

    <div class="pkg-detail">
      <div class="pkg-detail__main">
        <div class="card mb-4">
          <h3
            style="font-size:0.8125rem; font-weight:600; color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0.05em; margin-bottom:8px;"
          >
            Install
          </h3>
          <div class="code-block">
            <code>{install}</code>
            <button class="copy-btn" type="button" on:click={handleCopyInstall}
              >Copy</button
            >
          </div>
        </div>

        <div class="tabs">
          <button
            class:active={activeTab === 'readme'}
            class="tab"
            type="button"
            on:click={() => handlePackageTabChange('readme')}>Readme</button
          >
          <button
            class:active={activeTab === 'versions'}
            class="tab"
            type="button"
            on:click={() => handlePackageTabChange('versions')}
            >Versions ({releases.length})</button
          >
          <button
            class:active={activeTab === 'security'}
            class="tab"
            type="button"
            on:click={() => handlePackageTabChange('security')}
          >
            Security
            {#if openFindings.length > 0}
              <span
                class={`badge badge-severity-${worstSeverity(openFindings)}`}
                style="margin-left:4px;">{openFindings.length}</span
              >
            {/if}
          </button>
        </div>

        {#if activeTab === 'readme'}
          {#if readmeHtml}
            <div class="readme-content">{@html readmeHtml}</div>
          {:else}
            <div class="empty-state">
              <p>No README available for this package.</p>
            </div>
          {/if}
        {/if}

        {#if activeTab === 'versions'}
          {#if releases.length === 0}
            <div class="empty-state"><p>No releases yet.</p></div>
          {:else}
            {#each releases as release}
              <div class="release-row">
                <div>
                  <a
                    href={`/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}/versions/${encodeURIComponent(release.version)}`}
                    class="release-row__version"
                    data-sveltekit-preload-data="hover"
                  >
                    {release.version}
                  </a>
                  {#if release.is_yanked}
                    <span class="badge badge-yanked">yanked</span>
                  {/if}
                  {#if release.status === 'deprecated'}
                    <span class="badge badge-deprecated">deprecated</span>
                  {/if}
                </div>
                <div class="release-row__meta">
                  {formatDate(release.published_at || release.created_at)}
                </div>
              </div>
            {/each}
          {/if}
        {/if}

        {#if activeTab === 'security'}
          <div class="findings-toggle">
            <label>
              <input
                type="checkbox"
                bind:checked={includeResolvedFindings}
                on:change={handleResolvedToggleChange}
              />
              Show resolved findings
            </label>
          </div>

          {#if findingsNotice}
            <div class="notice notice--success">{findingsNotice}</div>
          {/if}
          {#if findingsError}
            <div class="notice notice--error">{findingsError}</div>
          {/if}

          {#if findings.length === 0}
            <div class="empty-state"><p>No security findings.</p></div>
          {:else}
            {#each [...findings].sort((left, right) => severityLevel(right.severity) - severityLevel(left.severity)) as finding}
              {@const severity = finding.severity?.toLowerCase() || 'info'}
              <div
                class={`finding-row ${finding.is_resolved ? 'finding-resolved' : ''}`}
              >
                <div class="finding-row__header">
                  <span class={`badge badge-severity-${severity}`}
                    >{severity}</span
                  >
                  <span class="badge badge-ecosystem"
                    >{formatFindingKind(finding.kind)}</span
                  >
                  <span class="finding-row__title">{finding.title}</span>
                  {#if finding.advisory_id}
                    {#if finding.advisory_id.startsWith('CVE-')}
                      <a
                        href={`https://nvd.nist.gov/vuln/detail/${encodeURIComponent(finding.advisory_id)}`}
                        target="_blank"
                        rel="noopener noreferrer">{finding.advisory_id}</a
                      >
                    {:else}
                      <span>{finding.advisory_id}</span>
                    {/if}
                  {/if}
                </div>
                <div class="finding-row__meta">
                  {#if finding.release_version}<span
                      >{formatVersionLabel(
                        ecosystem,
                        finding.release_version
                      )}</span
                    >{/if}
                  {#if finding.artifact_filename}<span
                      >{finding.artifact_filename}</span
                    >{/if}
                  <span>{formatDate(finding.detected_at)}</span>
                  {#if finding.is_resolved}
                    <span
                      >Resolved{finding.resolved_at
                        ? ` ${formatDate(finding.resolved_at)}`
                        : ''}</span
                    >
                  {/if}
                </div>
                {#if finding.description}
                  <div class="finding-row__description">
                    {finding.description}
                  </div>
                {/if}
                {#if pkg.can_manage_security}
                  <div class="finding-row__actions">
                    <label class="finding-row__note">
                      <span class="sr-only"
                        >Security finding note for {finding.title}</span
                      >
                      <textarea
                        rows="2"
                        maxlength="2000"
                        placeholder="Optional note (recorded in audit log)"
                        bind:value={findingNotes[finding.id]}
                      ></textarea>
                    </label>
                    <button
                      type="button"
                      class="btn btn-sm btn-secondary"
                      disabled={updatingFindingId !== null}
                      on:click={() => handleToggleFindingResolution(finding)}
                    >
                      {#if updatingFindingId === finding.id}
                        {finding.is_resolved ? 'Reopening…' : 'Resolving…'}
                      {:else}
                        {finding.is_resolved
                          ? 'Reopen finding'
                          : 'Mark resolved'}
                      {/if}
                    </button>
                  </div>
                {/if}
              </div>
            {/each}
          {/if}
        {/if}
      </div>

      <div class="pkg-detail__sidebar">
        {#if packageMetadata}
          <div class="card">
            <div class="sidebar-section">
              <h3>Ecosystem details</h3>

              {#if showsRegistryFamily}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Registry family</span>
                  <span class="sidebar-row__value"
                    >{ecosystemLabel(canonicalPackageEcosystem)}</span
                  >
                </div>
              {/if}

              {#if npmPackageMetadata}
                {#if npmPackageMetadata.scope}
                  <div class="sidebar-row">
                    <span class="sidebar-row__label">Scope</span>
                    <span class="sidebar-row__value"
                      ><code>{npmPackageMetadata.scope}</code></span
                    >
                  </div>
                {/if}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Package</span>
                  <span class="sidebar-row__value"
                    ><code>{npmPackageMetadata.unscoped_name}</code></span
                  >
                </div>
              {:else if pypiPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Project</span>
                  <span class="sidebar-row__value"
                    ><code>{pypiPackageMetadata.project_name}</code></span
                  >
                </div>
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Normalized</span>
                  <span class="sidebar-row__value"
                    ><code>{pypiPackageMetadata.normalized_name}</code></span
                  >
                </div>
              {:else if cargoPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Crate</span>
                  <span class="sidebar-row__value"
                    ><code>{cargoPackageMetadata.crate_name}</code></span
                  >
                </div>
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Normalized</span>
                  <span class="sidebar-row__value"
                    ><code>{cargoPackageMetadata.normalized_name}</code></span
                  >
                </div>
              {:else if nugetPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Package ID</span>
                  <span class="sidebar-row__value"
                    ><code>{nugetPackageMetadata.package_id}</code></span
                  >
                </div>
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Normalized</span>
                  <span class="sidebar-row__value"
                    ><code>{nugetPackageMetadata.normalized_id}</code></span
                  >
                </div>
              {:else if rubygemsPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Gem</span>
                  <span class="sidebar-row__value"
                    ><code>{rubygemsPackageMetadata.gem_name}</code></span
                  >
                </div>
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Normalized</span>
                  <span class="sidebar-row__value"
                    ><code>{rubygemsPackageMetadata.normalized_name}</code
                    ></span
                  >
                </div>
              {:else if composerPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Vendor</span>
                  <span class="sidebar-row__value"
                    ><code>{composerPackageMetadata.vendor}</code></span
                  >
                </div>
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Package</span>
                  <span class="sidebar-row__value"
                    ><code>{composerPackageMetadata.package}</code></span
                  >
                </div>
              {:else if mavenPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Group ID</span>
                  <span class="sidebar-row__value"
                    ><code>{mavenPackageMetadata.group_id}</code></span
                  >
                </div>
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Artifact ID</span>
                  <span class="sidebar-row__value"
                    ><code>{mavenPackageMetadata.artifact_id}</code></span
                  >
                </div>
              {:else if ociPackageMetadata}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">Repository</span>
                  <span class="sidebar-row__value"
                    ><code>{ociPackageMetadata.repository}</code></span
                  >
                </div>
                {#if ociPackageMetadata.segments.length > 0}
                  <div style="margin-top:8px;">
                    <div
                      class="sidebar-row__label"
                      style="margin-bottom:6px; display:block;"
                    >
                      Path segments
                    </div>
                    <div style="display:flex; flex-wrap:wrap; gap:6px;">
                      {#each ociPackageMetadata.segments as segment}
                        <span class="badge badge-ecosystem">{segment}</span>
                      {/each}
                    </div>
                  </div>
                {/if}
              {/if}
            </div>
          </div>
        {/if}

        <div class="card">
          <div class="sidebar-section">
            <h3>Package info</h3>
            {#if pkg.license}<div class="sidebar-row">
                <span class="sidebar-row__label">License</span><span
                  class="sidebar-row__value">{pkg.license}</span
                >
              </div>{/if}
            {#if pkg.visibility}<div class="sidebar-row">
                <span class="sidebar-row__label">Visibility</span><span
                  class="sidebar-row__value">{pkg.visibility}</span
                >
              </div>{/if}
            {#if pkg.download_count != null}<div class="sidebar-row">
                <span class="sidebar-row__label">Downloads</span><span
                  class="sidebar-row__value"
                  >{formatNumber(pkg.download_count)}</span
                >
              </div>{/if}
            {#if pkg.created_at}<div class="sidebar-row">
                <span class="sidebar-row__label">Created</span><span
                  class="sidebar-row__value">{formatDate(pkg.created_at)}</span
                >
              </div>{/if}
            {#if pkg.updated_at}<div class="sidebar-row">
                <span class="sidebar-row__label">Updated</span><span
                  class="sidebar-row__value">{formatDate(pkg.updated_at)}</span
                >
              </div>{/if}
          </div>
        </div>

        {#if pkg.owner_username || pkg.owner_org_slug}
          <div class="card">
            <div class="sidebar-section">
              <h3>Owner</h3>
              {#if pkg.owner_org_slug}
                <a
                  href={`/orgs/${encodeURIComponent(pkg.owner_org_slug)}`}
                  data-sveltekit-preload-data="hover">{pkg.owner_org_slug}</a
                >
              {:else if pkg.owner_username}
                <a
                  href={`/search?q=${encodeURIComponent(pkg.owner_username)}`}
                  data-sveltekit-preload-data="hover">{pkg.owner_username}</a
                >
              {/if}
            </div>
          </div>
        {/if}

        {#if pkg.owner_org_slug && pkg.team_access}
          <div class="card">
            <div class="sidebar-section">
              <h3>Delegated team access</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Team grants for this organization-owned package.
              </p>
              {#if teamAccessNotice}
                <div class="alert alert-success" style="margin-bottom:12px;">
                  {teamAccessNotice}
                </div>
              {/if}
              {#if teamAccessError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {teamAccessError}
                </div>
              {/if}
              {#if teamAccessManagementState.loadError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {teamAccessManagementState.loadError}
                </div>
              {/if}
              {#if pkg.team_access.length === 0}
                <p class="settings-copy">No team grants assigned yet.</p>
              {:else}
                <div class="token-list">
                  {#each pkg.team_access as grant}
                    <div class="token-row">
                      <div class="token-row__main">
                        <div class="token-row__title">
                          {grant.team_name || grant.team_slug || 'Unnamed team'}
                        </div>
                        <div class="token-row__meta">
                          {#if grant.team_slug}
                            <span>{grant.team_slug}</span>
                          {/if}
                          {#if grant.granted_at}
                            <span>latest grant {formatDate(grant.granted_at)}</span>
                          {/if}
                        </div>
                        <div class="token-row__scopes">
                          {#each grant.permissions || [] as permission}
                            <span class="badge badge-ecosystem"
                              >{formatPermission(permission)}</span
                            >
                          {/each}
                        </div>
                      </div>
                      {#if teamAccessManagementState.showManagement && grant.team_slug}
                        <div class="token-row__actions">
                          <button
                            type="button"
                            class="btn btn-secondary btn-sm"
                            disabled={Boolean(revokingTeamSlug)}
                            on:click={() =>
                              handleRemovePackageTeamAccess(grant.team_slug || '')}
                          >
                            {#if revokingTeamSlug === grant.team_slug}
                              Revoking…
                            {:else}
                              Revoke
                            {/if}
                          </button>
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
              {#if teamAccessManagementState.showManagement}
                <div
                  class="settings-subsection"
                  style="margin-top:16px; padding-top:16px; border-top:1px solid var(--color-border);"
                >
                  <h4 style="margin-bottom:12px;">Manage package access</h4>
                  <p class="settings-copy" style="margin-bottom:12px;">
                    Saving replaces the selected team&apos;s permissions for this
                    package.
                  </p>
                  {#if teamAccessManagementState.teams.length === 0}
                    <p class="settings-copy">
                      Create a team in the organization workspace before
                      delegating package access.
                    </p>
                  {:else}
                    <form on:submit={handleReplacePackageTeamAccess}>
                      <div class="form-group" style="margin-bottom:12px;">
                        <label for="package-team-access-team">Team</label>
                        <select
                          id="package-team-access-team"
                          name="team_slug"
                          class="form-input"
                          required
                          disabled={savingTeamAccess || Boolean(revokingTeamSlug)}
                        >
                          <option value="">Select a team</option>
                          {#each [...teamAccessManagementState.teams].sort((left, right) => formatTeamOption(left).localeCompare(formatTeamOption(right))) as team}
                            <option value={team.slug || ''}>
                              {formatTeamOption(team)}
                            </option>
                          {/each}
                        </select>
                      </div>
                      <fieldset style="margin:0 0 12px; padding:0; border:none;">
                        <legend
                          style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                        >
                          Permissions
                        </legend>
                        <div class="grid gap-3">
                          {#each TEAM_PERMISSION_OPTIONS as permission}
                            <label
                              class="rounded-lg border border-neutral-200 p-3 text-sm"
                            >
                              <span class="flex items-start gap-3">
                                <input
                                  type="checkbox"
                                  name="permissions"
                                  value={permission.value}
                                  disabled={savingTeamAccess ||
                                    Boolean(revokingTeamSlug)}
                                />
                                <span>
                                  <span class="block font-medium"
                                    >{permission.label}</span
                                  >
                                  <span class="mt-1 block text-muted"
                                    >{permission.description}</span
                                  >
                                </span>
                              </span>
                            </label>
                          {/each}
                        </div>
                      </fieldset>
                      <button
                        type="submit"
                        class="btn btn-primary"
                        disabled={savingTeamAccess || Boolean(revokingTeamSlug)}
                      >
                        {#if savingTeamAccess}
                          Saving…
                        {:else}
                          Save package access
                        {/if}
                      </button>
                    </form>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        {/if}

        {#if pkg.can_manage_metadata}
          <div class="card">
            <div class="sidebar-section">
              <h3>Package settings</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Update package metadata that appears on the detail page and in
                search. Leave a field blank to clear its stored value.
              </p>

              {#if packageSettingsNotice}
                <div class="alert alert-success" style="margin-bottom:12px;">
                  {packageSettingsNotice}
                </div>
              {/if}
              {#if packageSettingsError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {packageSettingsError}
                </div>
              {/if}

              <form id="package-settings-form" on:submit={handleUpdatePackage}>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-description">Description</label>
                  <textarea
                    bind:value={packageSettingsDescription}
                    id="package-settings-description"
                    name="description"
                    class="form-input"
                    rows="3"
                    placeholder="Short package summary"
                  ></textarea>
                </div>

                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-homepage">Homepage</label>
                  <input
                    bind:value={packageSettingsHomepage}
                    id="package-settings-homepage"
                    name="homepage"
                    class="form-input"
                    placeholder="https://example.test/project"
                  />
                </div>

                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-repository-url"
                    >Repository URL</label
                  >
                  <input
                    bind:value={packageSettingsRepositoryUrl}
                    id="package-settings-repository-url"
                    name="repository_url"
                    class="form-input"
                    placeholder="https://github.com/acme/demo-widget"
                  />
                </div>

                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-license">License</label>
                  <input
                    bind:value={packageSettingsLicense}
                    id="package-settings-license"
                    name="license"
                    class="form-input"
                    placeholder="MIT"
                  />
                </div>

                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-keywords">Keywords</label>
                  <input
                    bind:value={packageSettingsKeywords}
                    id="package-settings-keywords"
                    name="keywords"
                    class="form-input"
                    placeholder="docs, cli, api"
                  />
                </div>

                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-readme">README</label>
                  <textarea
                    bind:value={packageSettingsReadme}
                    id="package-settings-readme"
                    name="readme"
                    class="form-input"
                    rows="8"
                    placeholder="# Demo Widget"
                  ></textarea>
                </div>

                <div style="display:flex; gap:8px;">
                  <button
                    type="submit"
                    class="btn btn-primary"
                    style="flex:1; justify-content:center;"
                    disabled={!packageSettingsHasChanges ||
                      updatingPackageSettings}
                  >
                    {updatingPackageSettings ? 'Saving…' : 'Save settings'}
                  </button>
                  <button
                    type="button"
                    class="btn btn-secondary"
                    on:click={() => resetPackageSettingsForm(pkg)}
                    disabled={!packageSettingsHasChanges ||
                      updatingPackageSettings}
                  >
                    Reset
                  </button>
                </div>
              </form>
            </div>
          </div>
        {/if}

        {#if ecosystem === 'pypi'}
          <div class="card">
            <div class="sidebar-section">
              <h3>PyPI trusted publishing</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Publaryn can mint a short-lived PyPI upload token for CI when
                the incoming OIDC claims match exactly one trusted publisher
                configured for this package.
              </p>

              {#if trustedPublisherNotice}
                <div class="alert alert-success" style="margin-bottom:12px;">
                  {trustedPublisherNotice}
                </div>
              {/if}
              {#if trustedPublisherError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {trustedPublisherError}
                </div>
              {/if}
              {#if trustedPublisherState.loadError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {trustedPublisherState.loadError}
                </div>
              {/if}

              {#if trustedPublisherState.publishers.length === 0}
                <p class="settings-copy" style="margin-bottom:12px;">
                  No trusted publishers configured yet.
                </p>
              {:else}
                <div
                  style="display:flex; flex-direction:column; gap:12px; margin-bottom:12px;"
                >
                  {#each trustedPublisherState.publishers as publisher}
                    {@const publisherTitle = trustedPublisherHeading(publisher)}
                    {@const bindingFields = trustedPublisherBindingFields(
                      publisher
                    ).filter(
                      (field) =>
                        field.label !== 'Repository' ||
                        field.value !== publisherTitle
                    )}
                    <div
                      style="border:1px solid var(--color-border); border-radius:12px; padding:12px;"
                    >
                      <div
                        style="display:flex; gap:12px; justify-content:space-between; align-items:flex-start;"
                      >
                        <div style="min-width:0; flex:1;">
                          <div style="font-weight:600; overflow-wrap:anywhere;">
                            {publisherTitle}
                          </div>
                          {#if publisher.subject}
                            <div
                              class="settings-copy"
                              style="margin-top:4px; overflow-wrap:anywhere;"
                            >
                              Subject: {publisher.subject}
                            </div>
                          {/if}
                          {#if publisher.issuer}
                            <div
                              class="settings-copy"
                              style="margin-top:4px; overflow-wrap:anywhere;"
                            >
                              Issuer: {publisher.issuer}
                            </div>
                          {/if}
                          {#each bindingFields as field}
                            <div
                              class="settings-copy"
                              style="margin-top:4px; overflow-wrap:anywhere;"
                            >
                              {field.label}: {field.value}
                            </div>
                          {/each}
                          {#if publisher.created_at}
                            <div class="settings-copy" style="margin-top:4px;">
                              Added {formatDate(publisher.created_at)}
                            </div>
                          {/if}
                        </div>

                        {#if pkg.can_manage_trusted_publishers && publisher.id}
                          <button
                            type="button"
                            class="btn btn-secondary"
                            style="flex-shrink:0;"
                            on:click={() =>
                              handleDeleteTrustedPublisher(publisher)}
                            disabled={deletingTrustedPublisherId ===
                              publisher.id}
                          >
                            {deletingTrustedPublisherId === publisher.id
                              ? 'Removing…'
                              : 'Remove'}
                          </button>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}

              {#if pkg.can_manage_trusted_publishers}
                <div
                  style="padding-top:12px; border-top:1px solid var(--color-border);"
                >
                  <h4
                    style="font-size:0.875rem; font-weight:600; margin-bottom:12px;"
                  >
                    Add trusted publisher
                  </h4>
                  <p class="settings-copy" style="margin-bottom:12px;">
                    Use the issuer and subject claims from your CI provider.
                    Repository, workflow, and environment are optional
                    additional match constraints.
                  </p>

                  <form
                    id="trusted-publisher-form"
                    on:submit={handleCreateTrustedPublisher}
                  >
                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="trusted-publisher-issuer">Issuer</label>
                      <input
                        bind:value={trustedPublisherIssuer}
                        id="trusted-publisher-issuer"
                        name="issuer"
                        class="form-input"
                        placeholder="https://token.actions.githubusercontent.com"
                        required
                      />
                    </div>

                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="trusted-publisher-subject">Subject</label>
                      <input
                        bind:value={trustedPublisherSubject}
                        id="trusted-publisher-subject"
                        name="subject"
                        class="form-input"
                        placeholder="repo:acme/demo-widget:ref:refs/heads/main"
                        required
                      />
                    </div>

                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="trusted-publisher-repository"
                        >Repository (optional)</label
                      >
                      <input
                        bind:value={trustedPublisherRepository}
                        id="trusted-publisher-repository"
                        name="repository"
                        class="form-input"
                        placeholder="acme/demo-widget"
                      />
                    </div>

                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="trusted-publisher-workflow"
                        >Workflow ref (optional)</label
                      >
                      <input
                        bind:value={trustedPublisherWorkflowRef}
                        id="trusted-publisher-workflow"
                        name="workflow_ref"
                        class="form-input"
                        placeholder=".github/workflows/publish.yml@refs/heads/main"
                      />
                    </div>

                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="trusted-publisher-environment"
                        >Environment (optional)</label
                      >
                      <input
                        bind:value={trustedPublisherEnvironment}
                        id="trusted-publisher-environment"
                        name="environment"
                        class="form-input"
                        placeholder="production"
                      />
                    </div>

                    <button
                      type="submit"
                      class="btn btn-primary"
                      style="width:100%; justify-content:center;"
                      disabled={creatingTrustedPublisher}
                    >
                      {creatingTrustedPublisher
                        ? 'Adding…'
                        : 'Add trusted publisher'}
                    </button>
                  </form>
                </div>
              {:else}
                <p class="settings-copy" style="margin-bottom:0;">
                  Package administrators can add or remove trusted publishers
                  for this PyPI package.
                </p>
              {/if}
            </div>
          </div>
        {/if}

        {#if openFindings.length > 0}
          <div class="card">
            <div class="sidebar-section">
              <h3>Security</h3>
              <p style="margin-bottom:8px; font-size:0.875rem;">
                {openFindings.length} open finding{openFindings.length === 1
                  ? ''
                  : 's'}
              </p>
              <div>
                {#each ['critical', 'high', 'medium', 'low', 'info'] as severity}
                  {@const count = openFindings.filter(
                    (finding) => finding.severity?.toLowerCase() === severity
                  ).length}
                  {#if count > 0}
                    <span
                      class={`badge badge-severity-${severity}`}
                      style="margin-right:4px;">{count} {severity}</span
                    >
                  {/if}
                {/each}
              </div>
            </div>
          </div>
        {/if}

        {#if pkg.can_manage_releases}
          <div class="card">
            <div class="sidebar-section">
              <h3>Create release</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Start a new release in quarantine, then upload immutable
                artifacts and publish when it is ready.
              </p>
              {#if releaseNotice}<div
                  class="alert alert-success"
                  style="margin-bottom:12px;"
                >
                  {releaseNotice}
                </div>{/if}
              {#if releaseError}<div
                  class="alert alert-error"
                  style="margin-bottom:12px;"
                >
                  {releaseError}
                </div>{/if}
              <form
                id="package-create-release-form"
                on:submit={handleCreateRelease}
              >
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="release-version">Version</label>
                  <input
                    bind:value={newReleaseVersion}
                    id="release-version"
                    name="version"
                    class="form-input"
                    placeholder="1.0.0"
                    required
                  />
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="release-description">Description</label>
                  <textarea
                    bind:value={newReleaseDescription}
                    id="release-description"
                    name="description"
                    class="form-input"
                    rows="2"
                    placeholder="Optional release summary"
                  ></textarea>
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="release-changelog">Changelog</label>
                  <textarea
                    bind:value={newReleaseChangelog}
                    id="release-changelog"
                    name="changelog"
                    class="form-input"
                    rows="4"
                    placeholder="Optional changelog notes"
                  ></textarea>
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="release-source-ref">Source ref</label>
                  <input
                    bind:value={newReleaseSourceRef}
                    id="release-source-ref"
                    name="source_ref"
                    class="form-input"
                    placeholder="refs/tags/v1.0.0"
                  />
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label class="flex items-start gap-2">
                    <input
                      bind:checked={newReleaseIsPrerelease}
                      type="checkbox"
                      name="is_prerelease"
                    />
                    <span
                      >Mark this version as a pre-release instead of relying on
                      version suffix detection.</span
                    >
                  </label>
                </div>
                <button
                  type="submit"
                  class="btn btn-primary"
                  style="width:100%; justify-content:center;"
                  disabled={creatingRelease}
                >
                  {creatingRelease ? 'Creating…' : 'Create release'}
                </button>
              </form>
            </div>
          </div>
        {/if}

        {#if transferState.showTransfer}
          <div class="card">
            <div class="sidebar-section">
              <h3>Transfer ownership</h3>
              <div class="alert alert-warning" style="margin-bottom:12px;">
                This transfer is immediate and revokes existing team grants on
                the package.
              </div>
              {#if transferNotice}<div
                  class="alert alert-success"
                  style="margin-bottom:12px;"
                >
                  {transferNotice}
                </div>{/if}
              {#if transferError}<div
                  class="alert alert-error"
                  style="margin-bottom:12px;"
                >
                  {transferError}
                </div>{/if}
              {#if transferState.loadError}<div
                  class="alert alert-error"
                  style="margin-bottom:12px;"
                >
                  {transferState.loadError}
                </div>{/if}
              <p class="settings-copy" style="margin-bottom:12px;">
                Move this package away from {pkg.owner_org_slug ||
                  pkg.owner_username ||
                  'the current owner'} into an organization you already administer.
              </p>
              {#if transferState.organizations.length === 0}
                <p class="settings-copy" style="margin-bottom:0;">
                  You can transfer this package, but you do not currently
                  administer another organization that can receive it.
                </p>
              {:else}
                <form
                  id="package-transfer-form"
                  on:submit={handleTransferPackage}
                >
                  <div class="form-group" style="margin-bottom:12px;">
                    <label for="package-transfer-target"
                      >Target organization</label
                    >
                    <select
                      bind:value={targetOrgSlug}
                      id="package-transfer-target"
                      name="target_org_slug"
                      class="form-input"
                      required
                    >
                      <option value="">Select an organization</option>
                      {#each transferState.organizations as organization}
                        <option value={organization.slug || ''}
                          >{organization.name ||
                            organization.slug ||
                            'Unnamed organization'}</option
                        >
                      {/each}
                    </select>
                  </div>
                  <div class="form-group" style="margin-bottom:12px;">
                    <label class="flex items-start gap-2">
                      <input
                        bind:checked={transferConfirmed}
                        type="checkbox"
                        id="package-transfer-confirm"
                        name="confirm"
                        required
                      />
                      <span
                        >I understand this package transfer is immediate and
                        existing team grants will be removed.</span
                      >
                    </label>
                  </div>
                  <button
                    type="submit"
                    class="btn btn-danger"
                    style="width:100%; justify-content:center;"
                    disabled={transferringPackage}
                  >
                    {transferringPackage ? 'Transferring…' : 'Transfer package'}
                  </button>
                </form>
              {/if}
            </div>
          </div>
        {/if}

        {#if pkg.homepage || pkg.repository_url}
          <div class="card">
            <div class="sidebar-section">
              <h3>Links</h3>
              {#if pkg.homepage}<div style="margin-bottom:4px;">
                  <a
                    href={pkg.homepage}
                    target="_blank"
                    rel="noopener noreferrer">Homepage</a
                  >
                </div>{/if}
              {#if pkg.repository_url}<div style="margin-bottom:4px;">
                  <a
                    href={pkg.repository_url}
                    target="_blank"
                    rel="noopener noreferrer">Repository</a
                  >
                </div>{/if}
            </div>
          </div>
        {/if}

        {#if pkg.keywords && pkg.keywords.length > 0}
          <div class="card">
            <div class="sidebar-section">
              <h3>Keywords</h3>
              <div>
                {#each pkg.keywords as keyword}
                  <a
                    href={`/search?q=${encodeURIComponent(keyword)}`}
                    class="badge badge-ecosystem"
                    style="margin:2px;"
                    data-sveltekit-preload-data="hover">{keyword}</a
                  >
                {/each}
              </div>
            </div>
          </div>
        {/if}

        {#if tags.length > 0}
          <div class="card">
            <div class="sidebar-section">
              <h3>Tags</h3>
              {#each tags as tag}
                <div class="sidebar-row">
                  <span class="sidebar-row__label"
                    >{tag.tag || tag.name || ''}</span
                  >
                  <span class="sidebar-row__value">{tag.version}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
