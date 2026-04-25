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
    deletePackage as archivePackage,
    createRelease,
    createTrustedPublisher as createTrustedPublisherForPackage,
    deleteTag as deletePackageTag,
    deleteTrustedPublisher as deleteTrustedPublisherForPackage,
    deprecateRelease,
    getPackage,
    listReleases,
    listSecurityFindings,
    listTags,
    listTrustedPublishers as listTrustedPublishersForPackage,
    publishRelease,
    severityLevel,
    transferPackageOwnership,
    undeprecateRelease,
    unyankRelease,
    updatePackage,
    updateSecurityFinding,
    upsertTag as upsertPackageTag,
    yankRelease,
  } from '../../../../api/packages';
  import type { PackageDetailTab } from '../../../../pages/package-detail-tabs';
  import {
    buildPackageDetailPath,
    getPackageDetailTabFromQuery,
  } from '../../../../pages/package-detail-tabs';
  import {
    buildPackageSecurityEmptyStateMessage,
    buildPackageSecurityFilterSummary,
    countPackageSecurityFindingsBySeverity,
    filterPackageSecurityFindings,
    normalizePackageSecuritySearchQuery,
    type PackageSecurityFocusMode,
  } from '../../../../pages/package-security';
  import { getPackageSecurityViewFromQuery } from '../../../../pages/pkg-security-url';
  import {
    TEAM_PERMISSION_OPTIONS,
    formatTeamPermission,
  } from '../../../../pages/team-management';
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
    buildBundleAnalysisHighlights,
    buildBundleAnalysisQuickFacts,
    buildBundleAnalysisStats,
    bundleAnalysisNotes,
  } from '../../../../utils/package-analysis';
  import {
    buildPackageMetadataUpdateInput,
    createPackageMetadataFormValues,
    packageMetadataHasChanges,
  } from '../../../../utils/package-metadata';
  import { selectPackageTransferTargets } from '../../../../utils/package-transfer';
  import {
    getReleaseActionAvailability,
    getRestoreReleaseLabel,
  } from '../../../../utils/releases';
  import {
    REPOSITORY_VISIBILITY_OPTIONS,
    formatRepositoryVisibilityLabel,
  } from '../../../../utils/repositories';
  import { SECURITY_SEVERITIES } from '../../../../utils/security';
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

  type PackageTeamAccessGrant = NonNullable<
    PackageDetail['team_access']
  >[number];

  const SECURITY_REVIEW_PERMISSIONS = new Set(['admin', 'security_review']);

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
  let releaseActionNotice: string | null = null;
  let releaseActionError: string | null = null;
  let activeReleaseActionVersion: string | null = null;
  let transferNotice: string | null = null;
  let transferError: string | null = null;
  let teamAccessNotice: string | null = null;
  let teamAccessError: string | null = null;
  let packageSettingsNotice: string | null = null;
  let packageSettingsError: string | null = null;
  let archiveNotice: string | null = null;
  let archiveError: string | null = null;
  let trustedPublisherNotice: string | null = null;
  let trustedPublisherError: string | null = null;
  let includeResolvedFindings = false;
  let findingSearchQuery = '';
  let findingSeverityFilters: string[] = [];
  let findingFocusMode: PackageSecurityFocusMode = 'triage';
  let requestedTab: PackageDetailTab = 'readme';
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
  let packageSettingsVisibility = '';
  let updatingPackageSettings = false;
  let archiveConfirmed = false;
  let archivingPackage = false;
  let tagName = '';
  let tagVersion = '';
  let savingTag = false;
  let deletingTagName: string | null = null;
  let tagNotice: string | null = null;
  let tagError: string | null = null;

  let trustedPublisherIssuer = '';
  let trustedPublisherSubject = '';
  let trustedPublisherRepository = '';
  let trustedPublisherWorkflowRef = '';
  let trustedPublisherEnvironment = '';
  let creatingTrustedPublisher = false;
  let deletingTrustedPublisherId: string | null = null;
  let savingTeamAccess = false;
  let revokingTeamSlug: string | null = null;
  let lastPackageSecurityQuerySyncKey = '';

  $: ecosystem = $page.params.ecosystem ?? '';
  $: name = $page.params.name ?? '';
  $: requestedTab = getPackageDetailTabFromQuery($page.url.searchParams);
  $: activeTab =
    requestedTab === 'settings' && pkg?.can_manage_metadata !== true
      ? 'readme'
      : requestedTab;
  $: packageSecurityView = getPackageSecurityViewFromQuery(
    $page.url.searchParams
  );
  $: {
    const nextSyncKey = [
      packageSecurityView.focusMode,
      packageSecurityView.includeResolved ? '1' : '0',
      packageSecurityView.searchQuery,
      packageSecurityView.severities.join(','),
    ].join('|');

    if (nextSyncKey !== lastPackageSecurityQuerySyncKey) {
      lastPackageSecurityQuerySyncKey = nextSyncKey;
      includeResolvedFindings = packageSecurityView.includeResolved;
      findingSearchQuery = packageSecurityView.searchQuery;
      findingSeverityFilters = [...packageSecurityView.severities];
      findingFocusMode = packageSecurityView.focusMode;
    }
  }
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
    releaseActionNotice = null;
    releaseActionError = null;
    activeReleaseActionVersion = null;
    transferNotice = null;
    transferError = null;
    teamAccessNotice = null;
    teamAccessError = null;
    packageSettingsNotice = null;
    packageSettingsError = null;
    archiveNotice = null;
    archiveError = null;
    tagNotice = null;
    tagError = null;
    trustedPublisherNotice = null;
    trustedPublisherError = null;
    includeResolvedFindings = packageSecurityView.includeResolved;
    findingSearchQuery = packageSecurityView.searchQuery;
    findingSeverityFilters = [...packageSecurityView.severities];
    findingFocusMode = packageSecurityView.focusMode;
    resetReleaseForm();
    resetTransferForm();
    resetPackageSettingsForm();
    resetArchiveForm();
    resetTagForm();
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
      listSecurityFindings(ecosystem, name, {
        includeResolved: includeResolvedFindings,
      }).catch(() => [] as SecurityFinding[]),
      loadTransferState(pkg),
      loadTrustedPublisherState(pkg),
      loadTeamAccessManagementState(pkg),
    ]);

    releases = loadedReleases;
    tags = sortTags(loadedTags);
    findings = loadedFindings;
    transferState = loadedTransferState;
    trustedPublisherState = loadedTrustedPublisherState;
    teamAccessManagementState = loadedTeamAccessManagementState;
    resetTagForm(latestVersionForPackage(pkg));
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

    const form = event.currentTarget as HTMLFormElement;
    if (pkg.can_manage_visibility) {
      const submittedVisibility = form
        ? new FormData(form).get('visibility')?.toString()
        : null;
      packageSettingsVisibility =
        submittedVisibility || packageSettingsVisibility;
    }

    const input = buildPackageMetadataUpdateInput(pkg, {
      description: packageSettingsDescription,
      readme: packageSettingsReadme,
      homepage: packageSettingsHomepage,
      repositoryUrl: packageSettingsRepositoryUrl,
      license: packageSettingsLicense,
      keywords: packageSettingsKeywords,
      visibility: packageSettingsVisibility,
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

  async function handleArchivePackage(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!pkg || pkg.can_manage_metadata !== true || archivingPackage) {
      return;
    }

    if (pkg.is_archived) {
      archiveError = null;
      archiveNotice = 'This package is already archived.';
      return;
    }

    if (!archiveConfirmed) {
      archiveError =
        'Please confirm that you understand archiving marks this package as archived.';
      archiveNotice = null;
      return;
    }

    archivingPackage = true;
    archiveError = null;
    archiveNotice = null;

    try {
      const result = await archivePackage(ecosystem, name);
      await loadPackagePage();
      archiveNotice = result.message || 'Package archived';
    } catch (caughtError: unknown) {
      archiveError = toErrorMessage(caughtError, 'Failed to archive package.');
    } finally {
      archivingPackage = false;
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
    if (!currentPackage || !getAuthToken() || !currentPackage.owner_org_slug) {
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
          teams: (teamResponse.teams || []).filter((team) =>
            Boolean(team.slug)
          ),
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
    if (tab === requestedTab) {
      return;
    }

    await goto(
      buildPackageDetailPath(ecosystem, name, { tab }, $page.url.searchParams)
    );
  }

  async function syncPackageSecurityQueryState(
    overrides: {
      focusMode?: PackageSecurityFocusMode;
      includeResolved?: boolean;
      searchQuery?: string;
      severities?: string[];
    } = {}
  ): Promise<void> {
    await goto(
      buildPackageDetailPath(
        ecosystem,
        name,
        {
          tab: 'security',
          securityView: {
            focusMode: overrides.focusMode ?? findingFocusMode,
            includeResolved:
              overrides.includeResolved ?? includeResolvedFindings,
            searchQuery: overrides.searchQuery ?? findingSearchQuery,
            severities: overrides.severities ?? findingSeverityFilters,
          },
        },
        $page.url.searchParams
      ),
      {
        replaceState: true,
        noScroll: true,
        keepFocus: true,
      }
    );
  }

  async function handleResolvedToggleChange(): Promise<void> {
    await syncPackageSecurityQueryState({
      includeResolved: includeResolvedFindings,
    });

    try {
      findings = await listSecurityFindings(ecosystem, name, {
        includeResolved: includeResolvedFindings,
      });
    } catch {
      findings = [];
    }
  }

  async function handleFindingSearchInput(): Promise<void> {
    await syncPackageSecurityQueryState({
      searchQuery: findingSearchQuery,
    });
  }

  async function handleFindingFocusModeChange(): Promise<void> {
    await syncPackageSecurityQueryState({
      focusMode: findingFocusMode,
    });
  }

  async function handleFindingSeverityFilterChange(): Promise<void> {
    await syncPackageSecurityQueryState({
      severities: findingSeverityFilters,
    });
  }

  async function clearFindingFilters(): Promise<void> {
    findingSearchQuery = '';
    findingSeverityFilters = [];
    findingFocusMode = 'triage';
    await syncPackageSecurityQueryState({
      searchQuery: '',
      severities: [],
      focusMode: 'triage',
    });
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

  async function handleReleaseListPublish(release: Release): Promise<void> {
    activeReleaseActionVersion = release.version;
    releaseActionError = null;
    releaseActionNotice = null;

    try {
      const result = await publishRelease(ecosystem, name, release.version);
      await loadPackagePage();
      releaseActionNotice =
        result.message || `Submitted ${release.version} for scanning.`;
    } catch (caughtError: unknown) {
      releaseActionError = toErrorMessage(
        caughtError,
        'Failed to publish release.'
      );
    } finally {
      activeReleaseActionVersion = null;
    }
  }

  async function handleReleaseListYank(release: Release): Promise<void> {
    const reason = window.prompt(
      `Optional yank reason for ${release.version}`,
      release.yank_reason || ''
    );

    if (reason === null) {
      return;
    }

    activeReleaseActionVersion = release.version;
    releaseActionError = null;
    releaseActionNotice = null;

    try {
      const result = await yankRelease(ecosystem, name, release.version, {
        reason: reason.trim() || undefined,
      });
      await loadPackagePage();
      releaseActionNotice =
        result.message || `Yanked ${release.version} successfully.`;
    } catch (caughtError: unknown) {
      releaseActionError = toErrorMessage(
        caughtError,
        'Failed to yank release.'
      );
    } finally {
      activeReleaseActionVersion = null;
    }
  }

  async function handleReleaseListRestore(release: Release): Promise<void> {
    activeReleaseActionVersion = release.version;
    releaseActionError = null;
    releaseActionNotice = null;

    try {
      const result = await unyankRelease(ecosystem, name, release.version);
      await loadPackagePage();
      releaseActionNotice =
        result.message || `Restored ${release.version} successfully.`;
    } catch (caughtError: unknown) {
      releaseActionError = toErrorMessage(
        caughtError,
        'Failed to restore release.'
      );
    } finally {
      activeReleaseActionVersion = null;
    }
  }

  async function handleReleaseListDeprecate(release: Release): Promise<void> {
    const message = window.prompt(
      `Optional deprecation message for ${release.version}`,
      release.deprecation_message || ''
    );

    if (message === null) {
      return;
    }

    activeReleaseActionVersion = release.version;
    releaseActionError = null;
    releaseActionNotice = null;

    try {
      const result = await deprecateRelease(ecosystem, name, release.version, {
        message: message.trim() || undefined,
      });
      await loadPackagePage();
      releaseActionNotice =
        result.message || `Deprecated ${release.version} successfully.`;
    } catch (caughtError: unknown) {
      releaseActionError = toErrorMessage(
        caughtError,
        'Failed to deprecate release.'
      );
    } finally {
      activeReleaseActionVersion = null;
    }
  }

  async function handleReleaseListUndeprecate(release: Release): Promise<void> {
    activeReleaseActionVersion = release.version;
    releaseActionError = null;
    releaseActionNotice = null;

    try {
      const result = await undeprecateRelease(ecosystem, name, release.version);
      await loadPackagePage();
      releaseActionNotice =
        result.message || `Removed deprecation from ${release.version}.`;
    } catch (caughtError: unknown) {
      releaseActionError = toErrorMessage(
        caughtError,
        'Failed to remove release deprecation.'
      );
    } finally {
      activeReleaseActionVersion = null;
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
      await replaceTeamPackageAccess(
        pkg.owner_org_slug,
        teamSlug,
        ecosystem,
        name,
        {
          permissions,
        }
      );
      const teamLabel =
        teamAccessManagementState.teams.find((team) => team.slug === teamSlug)
          ?.name || teamSlug;
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

  async function handleRemovePackageTeamAccess(
    teamSlug: string
  ): Promise<void> {
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
      await removeTeamPackageAccess(
        pkg.owner_org_slug,
        teamSlug,
        ecosystem,
        name
      );
      const teamLabel =
        pkg.team_access?.find((grant) => grant.team_slug === teamSlug)
          ?.team_name || teamSlug;
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

  async function handleSaveTag(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    const resolvedTag = tagName.trim();
    const resolvedVersion = tagVersion.trim();

    if (!resolvedTag) {
      tagError = 'Enter a tag name.';
      tagNotice = null;
      return;
    }

    if (!resolvedVersion) {
      tagError = 'Select a release version to tag.';
      tagNotice = null;
      return;
    }

    savingTag = true;
    tagError = null;
    tagNotice = null;

    try {
      const result = await upsertPackageTag(ecosystem, name, resolvedTag, {
        version: resolvedVersion,
      });
      tags = sortTags((await listTags(ecosystem, name)) || []);
      tagNotice =
        result.message ||
        `Tag ${resolvedTag} now points to ${resolvedVersion}.`;
      resetTagForm(resolvedVersion);
    } catch (caughtError: unknown) {
      tagError = toErrorMessage(caughtError, 'Failed to save tag.');
    } finally {
      savingTag = false;
    }
  }

  async function handleDeleteTag(tag: Tag): Promise<void> {
    const resolvedTag = tag.tag?.trim() || tag.name?.trim() || '';
    if (!resolvedTag || deletingTagName) {
      return;
    }

    const confirmed = window.confirm(`Delete tag ${resolvedTag}?`);
    if (!confirmed) {
      return;
    }

    deletingTagName = resolvedTag;
    tagError = null;
    tagNotice = null;

    try {
      const result = await deletePackageTag(ecosystem, name, resolvedTag);
      tags = tags.filter(
        (currentTag) =>
          (currentTag.tag?.trim() || currentTag.name?.trim() || '') !==
          resolvedTag
      );
      tagNotice = result.message || `Deleted tag ${resolvedTag}.`;
      if (!tagName.trim()) {
        resetTagForm(latestVersionForPackage(pkg));
      }
    } catch (caughtError: unknown) {
      tagError = toErrorMessage(caughtError, 'Failed to delete tag.');
    } finally {
      deletingTagName = null;
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
    packageSettingsVisibility = values.visibility;
    updatingPackageSettings = false;
  }

  function resetArchiveForm(): void {
    archiveConfirmed = false;
    archivingPackage = false;
  }

  function resetTagForm(preferredVersion?: string | null): void {
    tagName = '';
    tagVersion =
      preferredVersion?.trim() ||
      latestVersionForPackage(pkg) ||
      releases[0]?.version ||
      '';
    savingTag = false;
    deletingTagName = null;
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
    currentPackage: PackageDetail | null
  ): string | null {
    return (
      currentPackage?.latest_version ??
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
    return formatTeamPermission(permission);
  }

  function isOrgAdminRole(role: string | null | undefined): boolean {
    return role === 'owner' || role === 'admin';
  }

  function canGrantReviewSecurity(
    grant: Pick<PackageTeamAccessGrant, 'permissions'> | null | undefined
  ): boolean {
    return (grant?.permissions || []).some((permission) =>
      SECURITY_REVIEW_PERMISSIONS.has(permission)
    );
  }

  function securityReviewPermissions(
    grant: Pick<PackageTeamAccessGrant, 'permissions'> | null | undefined
  ): string[] {
    return (grant?.permissions || []).filter((permission) =>
      SECURITY_REVIEW_PERMISSIONS.has(permission)
    );
  }

  function formatTeamOption(team: Team): string {
    const name = team.name?.trim();
    const slug = team.slug?.trim();

    if (name && slug && name !== slug) {
      return `${name} (${slug})`;
    }

    return name || slug || 'Unnamed team';
  }

  function sortTags(entries: Tag[]): Tag[] {
    return [...entries].sort((left, right) =>
      (left.tag || left.name || '').localeCompare(right.tag || right.name || '')
    );
  }

  function toErrorMessage(error: unknown, fallback: string): string {
    return error instanceof Error && error.message ? error.message : fallback;
  }

  $: packageMetadata = pkg?.ecosystem_metadata ?? null;
  $: packageBundleAnalysis = pkg?.bundle_analysis ?? null;
  $: canonicalPackageEcosystem = pkg?.ecosystem ?? ecosystem;
  $: normalizedFindingSearchQuery =
    normalizePackageSecuritySearchQuery(findingSearchQuery);
  $: filteredFindings = filterPackageSecurityFindings(findings, {
    searchQuery: normalizedFindingSearchQuery,
    severities: findingSeverityFilters,
    focusMode: findingFocusMode,
  });
  $: filteredFindingSeverityCounts =
    countPackageSecurityFindingsBySeverity(filteredFindings);
  $: findingFiltersSummary = buildPackageSecurityFilterSummary({
    totalLoadedCount: findings.length,
    visibleCount: filteredFindings.length,
    includeResolvedFindings,
    filters: {
      searchQuery: normalizedFindingSearchQuery,
      severities: findingSeverityFilters,
      focusMode: findingFocusMode,
    },
  });
  $: findingFiltersEmptyMessage = buildPackageSecurityEmptyStateMessage({
    totalLoadedCount: findings.length,
    includeResolvedFindings,
    filters: {
      searchQuery: normalizedFindingSearchQuery,
      severities: findingSeverityFilters,
      focusMode: findingFocusMode,
    },
  });
  $: hasActiveFindingFilters =
    normalizedFindingSearchQuery.length > 0 ||
    findingSeverityFilters.length > 0 ||
    findingFocusMode !== 'triage';
  $: showsRegistryFamily = Boolean(
    pkg?.ecosystem && pkg.ecosystem.toLowerCase() !== ecosystem.toLowerCase()
  );
  $: sortedTeamOptions = [...teamAccessManagementState.teams]
    .map((team) => ({
      team,
      label: formatTeamOption(team),
    }))
    .sort((left, right) => left.label.localeCompare(right.label));
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
        visibility: packageSettingsVisibility,
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

  <div class="page-shell">
    <section class="page-hero">
      <div class="page-hero__header">
        <div class="page-hero__copy">
          <span class="page-hero__eyebrow">
            <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
            Package
          </span>
          <h1 class="page-hero__title">{pkg.display_name || pkg.name}</h1>
          <p class="page-hero__subtitle">
            {pkg.description ||
              'Unified package details, release history, security posture, and delegated access.'}
          </p>
          <div class="page-hero__meta">
            <span class="badge badge-ecosystem"
              >{ecosystemIcon(ecosystem)} {ecosystemLabel(ecosystem)}</span
            >
            {#if latestVersion}
              <span class="badge badge-ecosystem"
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
        </div>
      </div>
    </section>

    <div class="detail-grid">
      <div class="detail-main">
        <section class="detail-summary">
          <div class="detail-summary__header">
            <div>
              <div class="detail-summary__title">Install</div>
              <p class="detail-summary__copy">
                Copy the native client command for the latest visible release.
              </p>
            </div>
          </div>
          <div class="code-block">
            <code>{install}</code>
            <button class="copy-btn" type="button" on:click={handleCopyInstall}
              >Copy</button
            >
          </div>
        </section>

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
                >{openFindings.length}</span
              >
            {/if}
          </button>
          {#if pkg.can_manage_metadata}
            <button
              class:active={activeTab === 'settings'}
              class="tab"
              type="button"
              on:click={() => handlePackageTabChange('settings')}
              >Settings</button
            >
          {/if}
        </div>

        {#if activeTab === 'readme'}
          {#if readmeHtml}
            <div class="readme-content">{@html readmeHtml}</div>
          {:else}
            <div class="empty-state surface-card">
              <p>No README available for this package.</p>
            </div>
          {/if}
        {/if}

        {#if activeTab === 'versions'}
          {#if releaseActionNotice}
            <div class="alert alert-success mb-4">{releaseActionNotice}</div>
          {/if}
          {#if releaseActionError}
            <div class="alert alert-error mb-4">{releaseActionError}</div>
          {/if}
          {#if releases.length === 0}
            <div class="empty-state surface-card"><p>No releases yet.</p></div>
          {:else}
            {#each releases as release}
              {@const releaseActions = getReleaseActionAvailability(release, 0)}
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
                  {#if release.status && release.status !== 'deprecated'}
                    <span class="badge badge-ecosystem">{release.status}</span>
                  {/if}
                  {#if release.bundle_analysis}
                    <div class="token-row__scopes" style="margin-top:0.5rem;">
                      {#each buildBundleAnalysisQuickFacts(release.bundle_analysis) as fact}
                        <span class="badge badge-ecosystem">{fact}</span>
                      {/each}
                    </div>
                  {/if}
                </div>
                <div class="release-row__meta">
                  {formatDate(release.published_at || release.created_at)}
                </div>
                {#if pkg.can_manage_releases && (releaseActions.canUploadArtifact || releaseActions.canYank || releaseActions.canRestore || releaseActions.canDeprecate || releaseActions.canUndeprecate)}
                  <div class="release-row__actions">
                    {#if releaseActions.canUploadArtifact}
                      <a
                        href={`/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}/versions/${encodeURIComponent(release.version)}`}
                        class="btn btn-secondary btn-sm"
                        data-sveltekit-preload-data="hover"
                        >Manage publish flow</a
                      >
                    {/if}
                    {#if releaseActions.canYank}
                      <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        disabled={activeReleaseActionVersion ===
                          release.version}
                        on:click={() => handleReleaseListYank(release)}
                      >
                        {activeReleaseActionVersion === release.version
                          ? 'Working…'
                          : 'Yank'}
                      </button>
                    {/if}
                    {#if releaseActions.canRestore}
                      <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        disabled={activeReleaseActionVersion ===
                          release.version}
                        on:click={() => handleReleaseListRestore(release)}
                      >
                        {activeReleaseActionVersion === release.version
                          ? 'Working…'
                          : getRestoreReleaseLabel(release)}
                      </button>
                    {/if}
                    {#if releaseActions.canDeprecate}
                      <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        disabled={activeReleaseActionVersion ===
                          release.version}
                        on:click={() => handleReleaseListDeprecate(release)}
                      >
                        {activeReleaseActionVersion === release.version
                          ? 'Working…'
                          : 'Deprecate'}
                      </button>
                    {/if}
                    {#if releaseActions.canUndeprecate}
                      <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        data-release-undeprecate={release.version}
                        disabled={activeReleaseActionVersion ===
                          release.version}
                        on:click={() => handleReleaseListUndeprecate(release)}
                      >
                        {activeReleaseActionVersion === release.version
                          ? 'Working…'
                          : 'Remove deprecation'}
                      </button>
                    {/if}
                    {#if release.status?.toLowerCase() === 'quarantine'}
                      <button
                        type="button"
                        class="btn btn-primary btn-sm"
                        disabled={activeReleaseActionVersion ===
                          release.version}
                        on:click={() => handleReleaseListPublish(release)}
                      >
                        {activeReleaseActionVersion === release.version
                          ? 'Working…'
                          : 'Publish'}
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
            {/each}
          {/if}
        {/if}

        {#if activeTab === 'security'}
          {@const securityReviewerTeams = (pkg?.team_access || []).filter(
            (grant) => canGrantReviewSecurity(grant)
          )}
          {#if pkg?.owner_org_slug && (pkg.can_manage_security || securityReviewerTeams.length > 0)}
            <div class="surface-card" style="margin-bottom:1rem;">
              <div class="surface-card__body">
                <h3>Security review access</h3>
                <p class="settings-copy" style="margin-bottom:0.75rem;">
                  {#if securityReviewerTeams.length > 0}
                    Teams with <strong>Security review</strong> or
                    <strong>Admin</strong> package grants can resolve and reopen
                    findings for this package.
                  {:else}
                    Security findings on this package can be triaged with your
                    current package access.
                  {/if}
                </p>
                <div class="token-row__scopes" style="margin-bottom:0.75rem;">
                  {#if pkg.can_manage_security}
                    <span class="badge badge-verified"
                      >You can triage findings</span
                    >
                  {/if}
                  {#if securityReviewerTeams.length > 0}
                    <span class="badge badge-ecosystem"
                      >{securityReviewerTeams.length} review team{securityReviewerTeams.length ===
                      1
                        ? ''
                        : 's'}</span
                    >
                  {/if}
                </div>
                {#if securityReviewerTeams.length > 0}
                  <div class="token-list">
                    {#each securityReviewerTeams as grant}
                      <div class="token-row">
                        <div class="token-row__main">
                          <div class="token-row__title">
                            {grant.team_name ||
                              grant.team_slug ||
                              'Unnamed team'}
                          </div>
                          <div class="token-row__meta">
                            {#if grant.team_slug}
                              <span>{grant.team_slug}</span>
                            {/if}
                            {#if grant.granted_at}
                              <span
                                >latest grant {formatDate(
                                  grant.granted_at
                                )}</span
                              >
                            {/if}
                          </div>
                          <div class="token-row__scopes">
                            {#each securityReviewPermissions(grant) as permission}
                              <span class="badge badge-ecosystem"
                                >{formatPermission(permission)}</span
                              >
                            {/each}
                          </div>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            </div>
          {/if}
          <div class="surface-card" style="margin-bottom:1rem;">
            <div class="surface-card__body">
              <h3>Filter findings</h3>
              <div
                class="flex flex-wrap items-end gap-4"
                style="margin-bottom:0.75rem;"
              >
                <div
                  class="form-group"
                  style="margin-bottom:0; min-width:220px;"
                >
                  <label for="package-security-focus">Focus</label>
                  <select
                    id="package-security-focus"
                    class="form-input"
                    bind:value={findingFocusMode}
                    on:change={handleFindingFocusModeChange}
                  >
                    <option value="triage">Unresolved triage queue</option>
                    <option value="all">All loaded findings</option>
                    <option value="resolved">Resolved history</option>
                  </select>
                </div>
                <div
                  class="form-group"
                  style="margin-bottom:0; min-width:260px; flex:1;"
                >
                  <label for="package-security-search">Search findings</label>
                  <input
                    id="package-security-search"
                    class="form-input"
                    bind:value={findingSearchQuery}
                    on:input={handleFindingSearchInput}
                    placeholder="Match title, advisory, version, or artifact"
                    autocomplete="off"
                  />
                </div>
                <label
                  class="badge badge-ecosystem"
                  for="package-security-include-resolved"
                >
                  <input
                    id="package-security-include-resolved"
                    type="checkbox"
                    bind:checked={includeResolvedFindings}
                    on:change={handleResolvedToggleChange}
                    style="margin-right:0.35rem;"
                  />
                  Load resolved findings
                </label>
                {#if hasActiveFindingFilters}
                  <button
                    type="button"
                    class="btn btn-secondary"
                    on:click={() => void clearFindingFilters()}
                    >Clear filters</button
                  >
                {/if}
              </div>
              <fieldset class="form-group" style="margin-bottom:0.75rem;">
                <legend>Severity</legend>
                <div class="token-row__scopes">
                  {#each SECURITY_SEVERITIES as severity}
                    <label class="badge badge-ecosystem">
                      <input
                        type="checkbox"
                        bind:group={findingSeverityFilters}
                        value={severity}
                        on:change={handleFindingSeverityFilterChange}
                        style="margin-right:0.35rem;"
                      />
                      {formatIdentifierLabel(severity)}
                    </label>
                  {/each}
                </div>
              </fieldset>
              <p class="settings-copy" style="margin:0;" aria-live="polite">
                {findingFiltersSummary}
              </p>
            </div>
          </div>

          {#if findingsNotice}
            <div class="notice notice--success">{findingsNotice}</div>
          {/if}
          {#if findingsError}
            <div class="notice notice--error">{findingsError}</div>
          {/if}

          {#if filteredFindings.length === 0}
            <div class="empty-state surface-card">
              <h3>
                {findings.length === 0
                  ? 'No security findings'
                  : 'No findings match current filters'}
              </h3>
              <p>{findingFiltersEmptyMessage}</p>
              {#if hasActiveFindingFilters}
                <button
                  type="button"
                  class="btn btn-secondary"
                  on:click={() => void clearFindingFilters()}
                  >Clear filters</button
                >
              {/if}
            </div>
          {:else}
            <div class="token-row__scopes" style="margin-bottom:1rem;">
              {#each SECURITY_SEVERITIES.filter((severity) => filteredFindingSeverityCounts[severity] > 0) as severity}
                <span class={`badge badge-severity-${severity}`}
                  >{formatNumber(filteredFindingSeverityCounts[severity])}
                  {severity}</span
                >
              {/each}
            </div>
            {#each filteredFindings as finding}
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

        {#if activeTab === 'settings' && pkg.can_manage_metadata}
          <section class="surface-card settings-section">
            <div class="surface-card__header">
              <h2 class="surface-card__title">Package settings</h2>
              <p class="surface-card__copy">
                Update package metadata that appears on the detail page and in
                search. Leave a field blank to clear its stored value.
              </p>
            </div>

            <div class="surface-card__body">
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

                {#if pkg.can_manage_visibility}
                  <div class="form-group" style="margin-bottom:12px;">
                    <label for="package-settings-visibility">Visibility</label>
                    <select
                      bind:value={packageSettingsVisibility}
                      id="package-settings-visibility"
                      name="visibility"
                      class="form-input"
                    >
                      <option value="" disabled>Select visibility</option>
                      {#each REPOSITORY_VISIBILITY_OPTIONS as option}
                        <option value={option.value}>{option.label}</option>
                      {/each}
                    </select>
                    <p
                      class="settings-copy"
                      style="margin-top:6px; margin-bottom:0;"
                    >
                      Package visibility cannot be broader than the enclosing
                      repository visibility. Publaryn validates that boundary
                      when settings are saved.
                    </p>
                  </div>
                {:else if pkg.visibility}
                  <div class="form-group" style="margin-bottom:12px;">
                    <label for="package-settings-visibility-readonly"
                      >Visibility</label
                    >
                    <input
                      id="package-settings-visibility-readonly"
                      class="form-input"
                      value={formatRepositoryVisibilityLabel(pkg.visibility)}
                      disabled
                    />
                    <p
                      class="settings-copy"
                      style="margin-top:6px; margin-bottom:0;"
                    >
                      Package administrators can change package visibility.
                    </p>
                  </div>
                {/if}

                <div class="grid gap-4 xl:grid-cols-2">
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
                </div>

                <div class="grid gap-4 xl:grid-cols-2">
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
                </div>

                <div class="form-group" style="margin-bottom:12px;">
                  <label for="package-settings-readme">README</label>
                  <textarea
                    bind:value={packageSettingsReadme}
                    id="package-settings-readme"
                    name="readme"
                    class="form-input"
                    rows="12"
                    placeholder="# Demo Widget"
                  ></textarea>
                </div>

                <div style="display:flex; gap:8px;">
                  <button
                    type="submit"
                    class="btn btn-primary"
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
          </section>

          <section class="surface-card settings-section">
            <div class="surface-card__header">
              <h2 class="surface-card__title">Archive package</h2>
              <p class="surface-card__copy">
                Archive keeps package history and audit records intact while
                marking the package as archived in Publaryn.
              </p>
            </div>

            <div class="surface-card__body">
              {#if archiveNotice}
                <div class="alert alert-success" style="margin-bottom:12px;">
                  {archiveNotice}
                </div>
              {/if}
              {#if archiveError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {archiveError}
                </div>
              {/if}

              <div class="alert alert-warning" style="margin-bottom:12px;">
                Archiving hides this package from normal maintenance workflows,
                but it does not remove stored releases, artifacts, or audit
                history.
              </div>

              {#if pkg.is_archived}
                <p class="settings-copy" style="margin-bottom:0;">
                  This package is already archived. Authorized maintainers can
                  still inspect package details and audit history.
                </p>
              {:else}
                <form
                  id="package-archive-form"
                  on:submit={handleArchivePackage}
                >
                  <div class="form-group" style="margin-bottom:12px;">
                    <label class="flex items-start gap-2">
                      <input
                        bind:checked={archiveConfirmed}
                        type="checkbox"
                        id="package-archive-confirm"
                        name="confirm"
                        required
                        disabled={archivingPackage}
                      />
                      <span
                        >I understand archiving marks this package as archived
                        and keeps its existing releases and audit history.</span
                      >
                    </label>
                  </div>

                  <button
                    type="submit"
                    class="btn btn-danger"
                    disabled={archivingPackage}
                  >
                    {archivingPackage ? 'Archiving…' : 'Archive package'}
                  </button>
                </form>
              {/if}
            </div>
          </section>
        {/if}
      </div>

      <div class="detail-sidebar">
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

        {#if packageBundleAnalysis}
          <div class="card">
            <div class="sidebar-section">
              <h3>Bundle analysis</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Bundlephobia-inspired metadata derived from the latest visible
                release{packageBundleAnalysis.source_version
                  ? ` (${packageBundleAnalysis.source_version})`
                  : ''}.
              </p>
              {#each buildBundleAnalysisStats(packageBundleAnalysis) as stat}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">{stat.label}</span>
                  <span class="sidebar-row__value">{stat.value}</span>
                </div>
              {/each}
              {#if buildBundleAnalysisHighlights(packageBundleAnalysis).length > 0}
                <div
                  class="token-row__scopes"
                  style="margin-top:12px; margin-bottom:12px;"
                >
                  {#each buildBundleAnalysisHighlights(packageBundleAnalysis) as highlight}
                    <span class="badge badge-ecosystem">{highlight}</span>
                  {/each}
                </div>
              {/if}
              {#if bundleAnalysisNotes(packageBundleAnalysis).length > 0}
                <div
                  class="settings-copy"
                  style="display:grid; gap:6px; margin:0;"
                >
                  {#each bundleAnalysisNotes(packageBundleAnalysis) as note}
                    <span>{note}</span>
                  {/each}
                </div>
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
                  class="sidebar-row__value"
                  >{formatRepositoryVisibilityLabel(pkg.visibility)}</span
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
                            <span
                              >latest grant {formatDate(grant.granted_at)}</span
                            >
                          {/if}
                        </div>
                        <div class="token-row__scopes">
                          {#each grant.permissions || [] as permission}
                            <span class="badge badge-ecosystem"
                              >{formatPermission(permission)}</span
                            >
                          {/each}
                          {#if canGrantReviewSecurity(grant)}
                            <span class="badge badge-verified"
                              >Can triage findings</span
                            >
                          {/if}
                        </div>
                      </div>
                      {#if teamAccessManagementState.showManagement && grant.team_slug}
                        <div class="token-row__actions">
                          <button
                            type="button"
                            class="btn btn-secondary btn-sm"
                            disabled={Boolean(revokingTeamSlug)}
                            on:click={() =>
                              handleRemovePackageTeamAccess(
                                grant.team_slug || ''
                              )}
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
                    Saving replaces the selected team's permissions for this
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
                          disabled={savingTeamAccess ||
                            Boolean(revokingTeamSlug)}
                        >
                          <option value="">Select a team</option>
                          {#each sortedTeamOptions as option}
                            <option value={option.team.slug || ''}>
                              {option.label}
                            </option>
                          {/each}
                        </select>
                      </div>
                      <fieldset
                        style="margin:0 0 12px; padding:0; border:none;"
                      >
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

        {#if tags.length > 0 || pkg.can_manage_releases}
          <div class="card">
            <div class="sidebar-section">
              <h3>Tags</h3>
              {#if tagNotice}
                <div class="alert alert-success" style="margin-bottom:12px;">
                  {tagNotice}
                </div>
              {/if}
              {#if tagError}
                <div class="alert alert-error" style="margin-bottom:12px;">
                  {tagError}
                </div>
              {/if}
              {#if tags.length > 0}
                {#each tags as tag}
                  {@const resolvedTag = tag.tag || tag.name || ''}
                  <div class="sidebar-row" style="align-items:center; gap:8px;">
                    <div style="min-width:0; flex:1;">
                      <div class="sidebar-row__label">{resolvedTag}</div>
                      <div class="sidebar-row__value">{tag.version}</div>
                    </div>
                    {#if pkg.can_manage_releases && resolvedTag}
                      <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        data-tag-delete={resolvedTag}
                        on:click={() => handleDeleteTag(tag)}
                        disabled={savingTag || deletingTagName === resolvedTag}
                      >
                        {deletingTagName === resolvedTag
                          ? 'Removing…'
                          : 'Delete'}
                      </button>
                    {/if}
                  </div>
                {/each}
              {:else}
                <p class="settings-copy" style="margin-bottom:12px;">
                  No tags published yet.
                </p>
              {/if}

              {#if pkg.can_manage_releases}
                <div
                  style="padding-top:12px; margin-top:12px; border-top:1px solid var(--color-border);"
                >
                  <h4
                    style="font-size:0.875rem; font-weight:600; margin-bottom:12px;"
                  >
                    Create or retarget a tag
                  </h4>
                  <p class="settings-copy" style="margin-bottom:12px;">
                    Point a mutable channel tag at a published release version
                    for installs and automation.
                  </p>
                  <form id="package-tag-form" on:submit={handleSaveTag}>
                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="package-tag-name">Tag name</label>
                      <input
                        bind:value={tagName}
                        id="package-tag-name"
                        name="tag"
                        class="form-input"
                        placeholder="latest"
                        required
                      />
                    </div>
                    <div class="form-group" style="margin-bottom:12px;">
                      <label for="package-tag-version">Release version</label>
                      <select
                        bind:value={tagVersion}
                        id="package-tag-version"
                        name="version"
                        class="form-input"
                        required
                        disabled={releases.length === 0}
                      >
                        <option value="" disabled>
                          Select a release version
                        </option>
                        {#each releases as release}
                          <option value={release.version}
                            >{release.version}</option
                          >
                        {/each}
                      </select>
                    </div>
                    <div style="display:flex; gap:8px;">
                      <button
                        type="submit"
                        class="btn btn-primary"
                        style="flex:1; justify-content:center;"
                        disabled={savingTag || releases.length === 0}
                      >
                        {savingTag ? 'Saving…' : 'Save tag'}
                      </button>
                      <button
                        type="button"
                        class="btn btn-secondary"
                        disabled={savingTag}
                        on:click={() => resetTagForm()}
                      >
                        Reset
                      </button>
                    </div>
                  </form>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
