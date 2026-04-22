<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';

  import { ApiError, getAuthToken } from '../../../api/client';
  import type {
    NamespaceClaim,
    NamespaceTransferOwnershipResult,
  } from '../../../api/namespaces';
  import {
    createNamespaceClaim,
    deleteNamespaceClaim,
    transferNamespaceClaim,
  } from '../../../api/namespaces';
  import type {
    OrgAuditListResponse,
    OrgAuditLog,
    OrgInvitation,
    OrgMember,
    OrgPackageSummary,
    OrgRepositoryPackageCoverageResponse,
    OrgRepositorySummary,
    OrgSecurityFindingsResponse,
    OrgSecurityPackageSummary,
    OrgSecurityQuery,
    OrgSecuritySummary,
    OrgWorkspaceBootstrapResponse,
    OrganizationDetail,
    OrganizationListResponse,
    OrganizationMembership,
    Team,
    TeamMember,
    TeamNamespaceAccessGrant,
    TeamNamespaceAccessListResponse,
    TeamPackageAccessGrant,
    TeamPackageAccessListResponse,
    TeamNamespaceAccessMutationResult,
    TeamRepositoryAccessGrant,
    TeamRepositoryAccessListResponse,
    TransferOwnershipResult,
  } from '../../../api/orgs';
  import {
    addMember,
    addTeamMember,
    createTeam,
    deleteTeam,
    exportOrgAuditLogsCsv,
    exportOrgSecurityFindingsCsv,
    getOrgWorkspaceBootstrap,
    listMyOrganizations,
    listOrgAuditLogs,
    listOrgSecurityFindings,
    listTeamNamespaceAccess,
    listTeamMembers,
    listTeamPackageAccess,
    listTeamRepositoryAccess,
    removeMember,
    removeTeamMember,
    removeTeamNamespaceAccess,
    removeTeamPackageAccess,
    removeTeamRepositoryAccess,
    replaceTeamNamespaceAccess,
    replaceTeamPackageAccess,
    replaceTeamRepositoryAccess,
    revokeInvitation,
    searchOrgMembers,
    sendInvitation,
    transferOwnership,
    updateOrg,
    updateTeam,
  } from '../../../api/orgs';
  import {
    createPackage,
    listSecurityFindings,
    transferPackageOwnership,
    updateSecurityFinding,
  } from '../../../api/packages';
  import type { SecurityFinding } from '../../../api/packages';
  import type { RepositoryPackageSummary } from '../../../api/repositories';
  import {
    createRepository,
    transferRepositoryOwnership,
    updateRepository,
  } from '../../../api/repositories';
  import OrgAuditFilterControls from '../../../lib/components/OrgAuditFilterControls.svelte';
  import OrgSecurityFindingTriageControls from '../../../lib/components/OrgSecurityFindingTriageControls.svelte';
  import OrgSecurityFilterControls from '../../../lib/components/OrgSecurityFilterControls.svelte';
  import TeamMembersEditor from '../../../lib/components/TeamMembersEditor.svelte';
  import TeamNamespaceAccessEditor from '../../../lib/components/TeamNamespaceAccessEditor.svelte';
  import TeamPackageAccessEditor from '../../../lib/components/TeamPackageAccessEditor.svelte';
  import TeamRepositoryAccessEditor from '../../../lib/components/TeamRepositoryAccessEditor.svelte';
  import TeamSettingsEditor from '../../../lib/components/TeamSettingsEditor.svelte';
  import type { OrgAuditActorOption } from '../../../pages/org-audit-actors';
  import {
    buildAuditActorOptions,
    buildRemoteAuditActorOptions,
    nextAuditActorInputState,
  } from '../../../pages/org-audit-actors';
  import {
    formatAuditActionLabel,
    formatAuditSummary,
    formatAuditTarget,
  } from '../../../pages/org-audit-format';
  import {
    ORG_AUDIT_ACTION_VALUES,
    buildOrgAuditExportFilename,
    buildOrgAuditPath,
    formatAuditActorQueryLabel,
    getAuditViewFromQuery,
    normalizeAuditAction,
    normalizeAuditActorUserId,
    normalizeAuditActorUsername,
  } from '../../../pages/org-audit-query';
  import {
    countOrgInvitationStatuses,
    describeOrgInvitationEvent,
    formatOrgInvitationInvitee,
    formatOrgInvitationStatusLabel,
    partitionOrgInvitations,
  } from '../../../pages/org-invitation-history';
  import {
    selectNamespaceTransferTargets,
    sortNamespaceClaims,
  } from '../../../pages/personal-namespaces';
  import type { OrgMemberPickerOption } from '../../../pages/org-member-picker';
  import {
    buildOrgMemberPickerOptions,
    resolveOrgMemberPickerInput,
  } from '../../../pages/org-member-picker';
  import {
    buildOrgSecurityExportFilename,
    buildOrgSecurityPath,
    getOrgSecurityViewFromQuery,
  } from '../../../pages/org-security-query';
  import {
    buildPackageDetailsPath,
    buildPackageSecurityFindingPath,
    buildPackageSecurityPath,
  } from '../../../pages/package-security-links';
  import {
    buildAuditExportQuery,
    buildSecurityExportQuery,
    decodePackageSelection,
    renderPackageSelectionValue,
    resolveAuditFilterSubmission,
    resolveSecurityFilterSubmission,
    resolveTeamNamespaceAccessSubmission,
    resolveTeamPackageAccessSubmission,
    resolveTeamRepositoryAccessSubmission,
  } from '../../../pages/org-workspace-actions';
  import {
    buildTeamManagementStateMapsFromBootstrap,
    buildEligibleTeamMemberOptions,
    buildNamespaceGrantOptions,
    buildPackageGrantOptions,
    buildRepositoryGrantOptions,
    createTeamManagementController,
    loadOrgMembersState,
    TEAM_DELETE_CONFIRMATION_MESSAGE,
    type TeamMemberState,
    type TeamNamespaceAccessState,
    type TeamPackageAccessState,
    type TeamRepositoryAccessState,
    TEAM_NAMESPACE_PERMISSION_OPTIONS,
    TEAM_PERMISSION_OPTIONS,
  } from '../../../pages/team-management';
  import {
    buildOrgSecurityPackageKey,
    mergeUpdatedOrgSecurityFinding,
    sortOrgSecurityFindings,
  } from '../../../pages/org-security-triage';
  import {
    canManageOrgInvitations,
    canManageOrgMembers,
    canManageOrgNamespaces,
    canManageOrgRepositories,
    canManageOrgTeams,
    canManageOrgWorkspace,
    canTransferOrgOwnership,
    canViewOrgAuditWorkspace,
    canViewOrgPeopleWorkspace,
  } from '../../../pages/org-workspace-access';
  import { ECOSYSTEMS, ecosystemLabel } from '../../../utils/ecosystem';
  import { formatDate, formatNumber } from '../../../utils/format';
  import {
    formatPackageCreationRepositoryLabel,
    getAllowedPackageVisibilityOptions,
    selectCreatableRepositories,
  } from '../../../utils/package-creation';
  import {
    selectPackageTransferTargets,
    selectTransferablePackages,
  } from '../../../utils/package-transfer';
  import {
    REPOSITORY_KIND_OPTIONS,
    REPOSITORY_VISIBILITY_OPTIONS,
    formatRepositoryKindLabel,
    formatRepositoryPackageCoverageLabel,
    formatRepositoryVisibilityLabel,
    selectRepositoryTransferTargets,
    selectTransferableRepositories,
  } from '../../../utils/repositories';
  import {
    SECURITY_SEVERITIES,
    normalizeSecuritySeverity,
    normalizeSecuritySeverityCounts,
    securitySeverityRank,
    totalSecuritySeverityCounts,
    worstSecuritySeverityFromCounts,
  } from '../../../utils/security';

  const ORG_AUDIT_PAGE_SIZE = 20;
  const DEFAULT_NAMESPACE_ECOSYSTEM = 'npm';
  const DEFAULT_PACKAGE_ECOSYSTEM = 'npm';
  const OWNERSHIP_TRANSFER_CONFIRMATION_MESSAGE =
    'Please confirm the ownership transfer.';
  const REPOSITORY_TRANSFER_CONFIRMATION_MESSAGE =
    'Please confirm the repository transfer.';
  const NAMESPACE_TRANSFER_CONFIRMATION_MESSAGE =
    'Please confirm the namespace transfer.';
  const PACKAGE_TRANSFER_CONFIRMATION_MESSAGE =
    'Please confirm the package transfer.';
  const INVITATION_REVOKE_CONFIRMATION_MESSAGE =
    'Please confirm that you want to revoke this invitation immediately.';
  const MEMBER_REMOVE_CONFIRMATION_MESSAGE =
    'Please confirm that you want to remove this member from the organization.';
  const NAMESPACE_DELETE_CONFIRMATION_MESSAGE =
    'Please confirm that you understand deleting this namespace claim is immediate and cannot be undone.';
  const REVIEW_TEAM_FALLBACK_LABEL = 'Team (no name)';
  const SECURITY_FINDING_NOTE_PLACEHOLDER =
    'Optional note (recorded in audit log)';
  const ORG_ROLE_OPTIONS = [
    { value: 'admin', label: 'Admin' },
    { value: 'maintainer', label: 'Maintainer' },
    { value: 'publisher', label: 'Publisher' },
    { value: 'security_manager', label: 'Security manager' },
    { value: 'auditor', label: 'Auditor' },
    { value: 'billing_manager', label: 'Billing manager' },
    { value: 'viewer', label: 'Viewer' },
  ] as const;
  let transferableNamespaceClaims: NamespaceClaim[] = [];
  const SECURITY_FILTER_ECOSYSTEM_OPTIONS = [
    { value: 'npm', label: 'npm / Bun' },
    { value: 'pypi', label: 'PyPI' },
    { value: 'cargo', label: 'Cargo' },
    { value: 'nuget', label: 'NuGet' },
    { value: 'rubygems', label: 'RubyGems' },
    { value: 'maven', label: 'Maven' },
    { value: 'composer', label: 'Composer' },
    { value: 'oci', label: 'OCI / Docker' },
  ] as const;
  const ORG_AUDIT_ACTION_OPTIONS = ORG_AUDIT_ACTION_VALUES.map((action) => ({
    value: action,
    label: formatAuditActionLabel(action),
  }));
  const SECURITY_FILTER_SEVERITY_OPTIONS = SECURITY_SEVERITIES.map((severity) => ({
    value: severity,
    label: formatIdentifierLabel(severity),
  }));
  let securityPackageOptions: Array<{ value: string; label: string }> = [];

  $: repositoryGrantOptions = buildRepositoryGrantOptions(repositories);
  $: packageGrantOptions = buildPackageGrantOptions(packages);
  $: namespaceGrantOptions = buildNamespaceGrantOptions(namespaceClaims);
  $: transferableNamespaceClaims = sortNamespaceClaims(
    namespaceClaims.filter((claim) => claim.can_transfer)
  );
  $: securityPackageOptions = [...packages]
    .sort((left, right) =>
      `${left.ecosystem || ''}:${left.name || ''}`.localeCompare(
        `${right.ecosystem || ''}:${right.name || ''}`
      )
    )
    .map((pkg) => ({
      value: pkg.name || '',
      label: `${pkg.ecosystem || ''} · ${pkg.name || ''}`,
    }));

  interface RepositoryPackageState {
    packages: RepositoryPackageSummary[];
    load_error: string | null;
  }

  interface OrgSecurityFindingState {
    findings: SecurityFinding[];
    load_error: string | null;
    loading: boolean;
    expanded: boolean;
    updatingFindingId: string | null;
    notice: string | null;
    error: string | null;
    findingNotes: Record<string, string>;
  }

  type CreatableRepository = OrgRepositorySummary & { slug: string };

  let lastLoadKey = '';
  let loading = true;
  let notFound = false;
  let loadError: string | null = null;
  let notice: string | null = null;
  let error: string | null = null;

  let org: OrganizationDetail | null = null;
  let membership: OrganizationMembership | undefined;
  let isAuthenticated = false;
  let canAdminister = false;
  let canManageInvitations = false;
  let canManageMembers = false;
  let canManageTeams = false;
  let canManageRepositories = false;
  let canManageNamespaces = false;
  let canViewAudit = false;
  let canViewPeopleWorkspace = false;
  let canTransferOwnership = false;

  let members: OrgMember[] = [];
  let membersError: string | null = null;
  let invitations: OrgInvitation[] = [];
  let invitationsError: string | null = null;
  let activeInvitations: OrgInvitation[] = [];
  let historicalInvitations: OrgInvitation[] = [];
  let showInvitationHistory = false;
  let teams: Team[] = [];
  let teamsError: string | null = null;
  let teamMembersBySlug: Record<string, TeamMemberState> = {};
  let teamPackageAccessBySlug: Record<string, TeamPackageAccessState> = {};
  let teamRepositoryAccessBySlug: Record<string, TeamRepositoryAccessState> =
    {};
  let teamNamespaceAccessBySlug: Record<string, TeamNamespaceAccessState> = {};
  let repositories: OrgRepositorySummary[] = [];
  let repositoriesError: string | null = null;
  let repositoryPackagesBySlug: Record<string, RepositoryPackageState> = {};
  let namespaceClaims: NamespaceClaim[] = [];
  let namespaceError: string | null = null;
  let packages: OrgPackageSummary[] = [];
  let packagesError: string | null = null;
  let namespaceTransferTargets: OrganizationMembership[] = [];
  let packageTransferTargets: OrganizationMembership[] = [];
  let repositoryTransferTargets: OrganizationMembership[] = [];
  let securitySummary: OrgSecuritySummary | null = null;
  let securityPackages: OrgSecurityPackageSummary[] = [];
  let securityError: string | null = null;
  let securityFindingsByPackageKey: Record<string, OrgSecurityFindingState> = {};
  let exportingSecurity = false;
  let auditLogs: OrgAuditLog[] = [];
  let auditError: string | null = null;
  let auditHasNext = false;
  let exportingAudit = false;
  let auditActorOptions: OrgAuditActorOption[] = [];
  let auditActorRemoteOptions: OrgAuditActorOption[] = [];
  let auditActorInput = '';
  let auditActorInputSyncKey = '';
  let auditActorSearchInFlight = false;
  let auditActorSearchRequest = 0;
  let creatableRepositories: CreatableRepository[] = [];
  let selectedPackageCreationRepository: CreatableRepository | null = null;
  let packageVisibilityOptions: Array<{ value: string; label: string }> = [];
  let explicitPackageVisibilityOptions: Array<{
    value: string;
    label: string;
  }> = [];
  let repositoryDefaultPackageVisibility = '';
  let ownershipMemberOptions: OrgMemberPickerOption[] = [];
  let invitationStatusCounts = countOrgInvitationStatuses([]);

  let newPackageRepositorySlug = '';
  let newPackageEcosystem = DEFAULT_PACKAGE_ECOSYSTEM;
  let newPackageName = '';
  let newPackageVisibility = '';
  let newPackageDisplayName = '';
  let newPackageDescription = '';
  let creatingPackage = false;
  let ownershipTransferConfirmationOpen = false;
  let ownershipTransferConfirmed = false;
  let transferringOwnership = false;
  let repositoryTransferConfirmationOpen = false;
  let repositoryTransferConfirmed = false;
  let transferringRepositoryOwnership = false;
  let namespaceTransferConfirmationOpen = false;
  let namespaceTransferConfirmed = false;
  let transferringNamespaceOwnership = false;
  let packageTransferConfirmationOpen = false;
  let packageTransferConfirmed = false;
  let transferringPackageOwnershipFlow = false;
  let invitationRevokeTargetId: string | null = null;
  let invitationRevokeConfirmed = false;
  let revokingInvitationId: string | null = null;
  let memberRemoveTargetUsername: string | null = null;
  let memberRemoveConfirmed = false;
  let removingMemberUsername: string | null = null;
  let teamDeleteTargetSlug: string | null = null;
  let teamDeleteConfirmed = false;
  let deletingTeamSlug: string | null = null;
  let namespaceDeleteTargetId: string | null = null;
  let namespaceDeleteConfirmed = false;
  let deletingNamespaceClaimId: string | null = null;

  $: slug = $page.params.slug ?? '';
  $: pageNotice = $page.url.searchParams.get('notice')?.trim() || null;
  $: transferCandidates = members.filter(
    (member) =>
      member.role !== 'owner' &&
      typeof member.user_id === 'string' &&
      member.user_id.trim().length > 0 &&
      typeof member.username === 'string' &&
      member.username.trim().length > 0
  );
  $: ownershipMemberOptions = buildOrgMemberPickerOptions(transferCandidates);
  $: auditView = getAuditViewFromQuery($page.url.searchParams);
  $: securityView = getOrgSecurityViewFromQuery($page.url.searchParams);
  $: loadKey = `${slug}|${$page.url.search}`;
  $: if (slug && loadKey !== lastLoadKey) {
    lastLoadKey = loadKey;
    void loadOrganizationPage();
  }

  $: auditActorOptions = buildAuditActorOptions(
    members,
    auditActorRemoteOptions
  );
  $: {
    const nextInputState = nextAuditActorInputState(
      auditActorInputSyncKey,
      auditActorInput,
      auditView.actorUserId,
      auditView.actorUsername
    );

    if (
      nextInputState.syncKey !== auditActorInputSyncKey ||
      nextInputState.input !== auditActorInput
    ) {
      auditActorInputSyncKey = nextInputState.syncKey;
      auditActorInput = nextInputState.input;
    }
  }

  $: {
    const trimmedAuditActorInput = auditActorInput.trim();

    if (trimmedAuditActorInput.length >= 2 && canViewAudit) {
      void searchAuditActors(trimmedAuditActorInput);
    }
  }

  $: {
    const trimmedAuditActorInput = auditActorInput.trim();

    if (!trimmedAuditActorInput && auditActorRemoteOptions.length > 0) {
      auditActorRemoteOptions = [];
    }
  }

  $: transferablePackages = selectTransferablePackages(packages);
  $: transferableRepositories = selectTransferableRepositories(repositories);
  $: {
    const { active, history } = partitionOrgInvitations(invitations);
    activeInvitations = active;
    historicalInvitations = history;
    invitationStatusCounts = countOrgInvitationStatuses(invitations);

    if (historicalInvitations.length === 0 && showInvitationHistory) {
      showInvitationHistory = false;
    }
  }
  $: creatableRepositories = selectCreatableRepositories(repositories);
  $: if (creatableRepositories.length === 0) {
    if (newPackageRepositorySlug !== '') {
      newPackageRepositorySlug = '';
    }
  } else if (
    !creatableRepositories.some(
      (repository) => repository.slug === newPackageRepositorySlug
    )
  ) {
    newPackageRepositorySlug = creatableRepositories[0]?.slug || '';
  }
  $: selectedPackageCreationRepository =
    creatableRepositories.find(
      (repository) => repository.slug === newPackageRepositorySlug
    ) || null;
  $: packageVisibilityOptions = getAllowedPackageVisibilityOptions(
    selectedPackageCreationRepository?.visibility,
    { repositoryIsOrgOwned: true }
  );
  $: repositoryDefaultPackageVisibility =
    selectedPackageCreationRepository?.visibility
      ?.trim()
      .toLowerCase()
      .replace(/-/g, '_') || '';
  $: explicitPackageVisibilityOptions = repositoryDefaultPackageVisibility
    ? packageVisibilityOptions.filter(
        (option) => option.value !== repositoryDefaultPackageVisibility
      )
    : packageVisibilityOptions;
  $: if (
    newPackageVisibility &&
    !packageVisibilityOptions.some(
      (option) => option.value === newPackageVisibility
    )
  ) {
    newPackageVisibility = '';
  }
  $: severityCounts = normalizeSecuritySeverityCounts(
    securitySummary?.severities
  );
  $: openFindingCount =
    typeof securitySummary?.open_findings === 'number' &&
    Number.isFinite(securitySummary.open_findings)
      ? Math.max(0, Math.trunc(securitySummary.open_findings))
      : totalSecuritySeverityCounts(severityCounts);
  $: affectedPackageCount =
    typeof securitySummary?.affected_packages === 'number' &&
    Number.isFinite(securitySummary.affected_packages)
      ? Math.max(0, Math.trunc(securitySummary.affected_packages))
      : securityPackages.length;
  $: hasSecurityFilters =
    securityView.severities.length > 0 ||
    Boolean(securityView.ecosystem) ||
    Boolean(securityView.packageQuery);
  $: sortedSecurityPackages = [...securityPackages].sort((left, right) => {
    const leftSeverity = left.worst_severity
      ? normalizeSecuritySeverity(left.worst_severity)
      : worstSecuritySeverityFromCounts(left.severities);
    const rightSeverity = right.worst_severity
      ? normalizeSecuritySeverity(right.worst_severity)
      : worstSecuritySeverityFromCounts(right.severities);
    const severityDelta =
      securitySeverityRank(rightSeverity) - securitySeverityRank(leftSeverity);
    if (severityDelta !== 0) {
      return severityDelta;
    }

    const leftFindings =
      typeof left.open_findings === 'number' &&
      Number.isFinite(left.open_findings)
        ? Math.max(0, Math.trunc(left.open_findings))
        : totalSecuritySeverityCounts(left.severities);
    const rightFindings =
      typeof right.open_findings === 'number' &&
      Number.isFinite(right.open_findings)
        ? Math.max(0, Math.trunc(right.open_findings))
        : totalSecuritySeverityCounts(right.severities);

    if (rightFindings !== leftFindings) {
      return rightFindings - leftFindings;
    }

    return `${left.ecosystem || ''}:${left.name || ''}`.localeCompare(
      `${right.ecosystem || ''}:${right.name || ''}`
    );
  });

  function createOrgSecurityFindingState(
    overrides: Partial<OrgSecurityFindingState> = {}
  ): OrgSecurityFindingState {
    return {
      findings: [],
      load_error: null,
      loading: false,
      expanded: false,
      updatingFindingId: null,
      notice: null,
      error: null,
      findingNotes: {},
      ...overrides,
    };
  }

  function getSecurityPackageKey(
    securityPackage: Pick<OrgSecurityPackageSummary, 'ecosystem' | 'name'>
  ): string {
    return buildOrgSecurityPackageKey(
      securityPackage.ecosystem,
      securityPackage.name
    );
  }

  function getOrgSecurityFindingState(
    securityPackage: Pick<OrgSecurityPackageSummary, 'ecosystem' | 'name'>
  ): OrgSecurityFindingState {
    return (
      securityFindingsByPackageKey[getSecurityPackageKey(securityPackage)] ||
      createOrgSecurityFindingState()
    );
  }

  function updateOrgSecurityFindingState(
    packageKey: string,
    updates: Partial<OrgSecurityFindingState>
  ): void {
    securityFindingsByPackageKey = {
      ...securityFindingsByPackageKey,
      [packageKey]: {
        ...(securityFindingsByPackageKey[packageKey] ||
          createOrgSecurityFindingState()),
        ...updates,
      },
    };
  }

  function updateOrgSecurityFindingNote(
    packageKey: string,
    findingId: string,
    value: string
  ): void {
    const currentState =
      securityFindingsByPackageKey[packageKey] || createOrgSecurityFindingState();
    updateOrgSecurityFindingState(packageKey, {
      findingNotes: {
        ...currentState.findingNotes,
        [findingId]: value,
      },
    });
  }

  function resolveLoadNotice(
    options: { notice?: string | null },
    fallbackNotice: string | null
  ): string | null {
    const explicitNoticeProvided = Object.hasOwn(options, 'notice');
    return explicitNoticeProvided ? options.notice ?? null : fallbackNotice;
  }

  async function loadOrganizationPage(
    options: { notice?: string | null; error?: string | null } = {}
  ): Promise<void> {
    loading = true;
    notFound = false;
    loadError = null;
    notice = resolveLoadNotice(options, pageNotice);
    error = options.error ?? null;
    canViewPeopleWorkspace = false;
    securityFindingsByPackageKey = {};
    ownershipTransferConfirmationOpen = false;
    ownershipTransferConfirmed = false;
    transferringOwnership = false;
    repositoryTransferConfirmationOpen = false;
    repositoryTransferConfirmed = false;
    transferringRepositoryOwnership = false;
    namespaceTransferConfirmationOpen = false;
    namespaceTransferConfirmed = false;
    transferringNamespaceOwnership = false;
    packageTransferConfirmationOpen = false;
    packageTransferConfirmed = false;
    transferringPackageOwnershipFlow = false;
    invitationRevokeTargetId = null;
    invitationRevokeConfirmed = false;
    revokingInvitationId = null;
    memberRemoveTargetUsername = null;
    memberRemoveConfirmed = false;
    removingMemberUsername = null;
    teamDeleteTargetSlug = null;
    teamDeleteConfirmed = false;
    deletingTeamSlug = null;
    namespaceDeleteTargetId = null;
    namespaceDeleteConfirmed = false;
    deletingNamespaceClaimId = null;

    isAuthenticated = Boolean(getAuthToken());

    const resolvedAuditAction = normalizeAuditAction(auditView.action);
    const resolvedAuditActorUserId = normalizeAuditActorUserId(
      auditView.actorUserId
    );
    const securityQuery: OrgSecurityQuery = {
      severities:
        securityView.severities.length > 0
          ? securityView.severities
          : undefined,
      ecosystem: securityView.ecosystem || undefined,
      package: securityView.packageQuery || undefined,
    };

    try {
      const [bootstrapData, myOrganizationsData] = await Promise.all([
        getOrgWorkspaceBootstrap(slug, securityQuery),
        isAuthenticated
          ? listMyOrganizations().catch(
              (): OrganizationListResponse => ({ organizations: [] })
            )
          : Promise.resolve<OrganizationListResponse>({ organizations: [] }),
      ]);

      org = bootstrapData.org || null;
      membership = myOrganizationsData.organizations.find(
        (item) => item.slug === slug
      );
      canViewPeopleWorkspace = canViewOrgPeopleWorkspace(org);
      canAdminister = canManageOrgWorkspace(org);
      canManageInvitations = canManageOrgInvitations(org);
      canManageMembers = canManageOrgMembers(org);
      canManageTeams = canManageOrgTeams(org);
      canManageRepositories = canManageOrgRepositories(org);
      canManageNamespaces = canManageOrgNamespaces(org);
      canViewAudit = canViewOrgAuditWorkspace(org);
      canTransferOwnership = canTransferOrgOwnership(org);

      repositories = bootstrapData.repositories || [];
      repositoriesError = null;
      packages = bootstrapData.packages || [];
      packagesError = null;
      securitySummary = bootstrapData.security?.summary || null;
      securityPackages = bootstrapData.security?.packages || [];
      securityError = null;
      teams = bootstrapData.teams || [];
      teamsError = null;
      invitations = bootstrapData.invitations || [];
      invitationsError = null;
      namespaceClaims = bootstrapData.namespaces || [];
      namespaceError = null;
      repositoryPackagesBySlug = buildRepositoryPackagesBySlug(
        repositories,
        bootstrapData.repository_package_coverage || []
      );
      packageTransferTargets = selectPackageTransferTargets(
        myOrganizationsData.organizations,
        slug
      );
      namespaceTransferTargets = selectNamespaceTransferTargets(
        myOrganizationsData.organizations,
        slug
      );
      repositoryTransferTargets = selectRepositoryTransferTargets(
        myOrganizationsData.organizations,
        slug
      );

      const [
        memberState,
        auditData,
      ] = await Promise.all([
        loadOrgMembersState(slug, {
          include: canViewPeopleWorkspace,
          errorMessage: 'Failed to load members.',
          toErrorMessage,
        }),
        canViewAudit
          ? listOrgAuditLogs(slug, {
              action: resolvedAuditAction || undefined,
              actorUserId: resolvedAuditActorUserId || undefined,
              occurredFrom: auditView.occurredFrom || undefined,
              occurredUntil: auditView.occurredUntil || undefined,
              page: auditView.page,
              perPage: ORG_AUDIT_PAGE_SIZE,
            }).catch(
              (caughtError: unknown): OrgAuditListResponse => ({
                page: auditView.page,
                per_page: ORG_AUDIT_PAGE_SIZE,
                has_next: false,
                logs: [],
                load_error: toErrorMessage(
                  caughtError,
                  'Failed to load activity log.'
                ),
              })
            )
          : Promise.resolve<OrgAuditListResponse>({
              page: auditView.page,
              per_page: ORG_AUDIT_PAGE_SIZE,
              has_next: false,
              logs: [],
              load_error: null,
            }),
      ]);

      members = memberState.members;
      membersError = memberState.load_error;
      const teamManagementStateMaps = buildTeamManagementStateMapsFromBootstrap(
        bootstrapData.team_management
      );
      teamMembersBySlug = teamManagementStateMaps.teamMembersBySlug;
      teamPackageAccessBySlug = teamManagementStateMaps.teamPackageAccessBySlug;
      teamRepositoryAccessBySlug = teamManagementStateMaps.teamRepositoryAccessBySlug;
      teamNamespaceAccessBySlug = teamManagementStateMaps.teamNamespaceAccessBySlug;
      auditLogs = auditData.logs || [];
      auditError = auditData.load_error || null;
      auditHasNext = auditData.has_next === true;
    } catch (caughtError: unknown) {
      if (caughtError instanceof ApiError && caughtError.status === 404) {
        notFound = true;
      } else {
        loadError = toErrorMessage(caughtError, 'Failed to load organization.');
      }
    } finally {
      loading = false;
    }
  }

  async function reloadSecurityOverview(): Promise<void> {
    const securityQuery: OrgSecurityQuery = {
      severities:
        securityView.severities.length > 0 ? securityView.severities : undefined,
      ecosystem: securityView.ecosystem || undefined,
      package: securityView.packageQuery || undefined,
    };

    try {
      const securityData = await listOrgSecurityFindings(slug, securityQuery);
      securitySummary = securityData.summary || null;
      securityPackages = securityData.packages || [];
      securityError = securityData.load_error || null;
    } catch (caughtError: unknown) {
      securitySummary = null;
      securityPackages = [];
      securityError = toErrorMessage(
        caughtError,
        'Failed to load security overview.'
      );
    }
  }

  function buildRepositoryPackagesBySlug(
    repositoryList: OrgRepositorySummary[],
    repositoryCoverage: OrgRepositoryPackageCoverageResponse['repositories']
  ): Record<string, RepositoryPackageState> {
    const repositoriesWithSlug = repositoryList.filter(hasRepositorySlug);
    const packagesByRepositorySlug = new Map(
      (repositoryCoverage || [])
        .filter(hasRepositoryCoverageSlug)
        .map((entry) => [entry.repository_slug, entry.packages || []] as const)
    );

    return Object.fromEntries(
      repositoriesWithSlug.map((repository) => [
        repository.slug,
        {
          packages: packagesByRepositorySlug.get(repository.slug) || [],
          load_error: null,
        },
      ])
    );
  }

  async function handleAuditFilterSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const resolution = resolveAuditFilterSubmission(
      new FormData(event.currentTarget as HTMLFormElement),
      auditActorOptions
    );

    if (!resolution.ok) {
      await loadOrganizationPage({
        error: resolution.error,
      });
      return;
    }

    await goto(
      buildOrgAuditPath(slug, resolution.value, $page.url.searchParams)
    );
  }

  async function goToAuditPage(nextPage: number): Promise<void> {
    await goto(
      buildOrgAuditPath(
        slug,
        {
          action: auditView.action,
          actorUserId: auditView.actorUserId,
          actorUsername: auditView.actorUsername,
          occurredFrom: auditView.occurredFrom,
          occurredUntil: auditView.occurredUntil,
          page: nextPage,
        },
        $page.url.searchParams
      )
    );
  }

  async function clearAuditActionFilter(): Promise<void> {
    await goto(
      buildOrgAuditPath(
        slug,
        {
          action: '',
          actorUserId: auditView.actorUserId,
          actorUsername: auditView.actorUsername,
          occurredFrom: auditView.occurredFrom,
          occurredUntil: auditView.occurredUntil,
          page: 1,
        },
        $page.url.searchParams
      )
    );
  }

  async function clearAuditActorFilter(): Promise<void> {
    await goto(
      buildOrgAuditPath(
        slug,
        {
          action: auditView.action,
          actorUserId: '',
          actorUsername: '',
          occurredFrom: auditView.occurredFrom,
          occurredUntil: auditView.occurredUntil,
          page: 1,
        },
        $page.url.searchParams
      )
    );
  }

  async function clearAuditDateFilter(): Promise<void> {
    await goto(
      buildOrgAuditPath(
        slug,
        {
          action: auditView.action,
          actorUserId: auditView.actorUserId,
          actorUsername: auditView.actorUsername,
          occurredFrom: '',
          occurredUntil: '',
          page: 1,
        },
        $page.url.searchParams
      )
    );
  }

  async function focusAuditActor(
    actorUserId: string,
    actorUsername: string
  ): Promise<void> {
    if (!actorUserId) {
      return;
    }

    await goto(
      buildOrgAuditPath(
        slug,
        {
          action: auditView.action,
          actorUserId,
          actorUsername,
          occurredFrom: auditView.occurredFrom,
          occurredUntil: auditView.occurredUntil,
          page: 1,
        },
        $page.url.searchParams
      )
    );
  }

  async function handleExportAudit(): Promise<void> {
    exportingAudit = true;

    try {
      const csv = await exportOrgAuditLogsCsv(
        slug,
        buildAuditExportQuery(auditView)
      );

      downloadTextFile(
        buildOrgAuditExportFilename(
          slug,
          {
            action: auditView.action,
            actorUsername: auditView.actorUsername,
            occurredFrom: auditView.occurredFrom,
            occurredUntil: auditView.occurredUntil,
          },
          new Date()
        ),
        csv,
        'text/csv;charset=utf-8'
      );
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to export activity log.'),
      });
    } finally {
      exportingAudit = false;
    }
  }

  async function handleSecurityFilterSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const nextView = resolveSecurityFilterSubmission(
      new FormData(event.currentTarget as HTMLFormElement)
    );

    await goto(
      buildOrgSecurityPath(slug, nextView, $page.url.searchParams)
    );
  }

  async function clearSecuritySeverityFilter(): Promise<void> {
    await goto(
      buildOrgSecurityPath(
        slug,
        {
          severities: [],
          ecosystem: securityView.ecosystem,
          packageQuery: securityView.packageQuery,
        },
        $page.url.searchParams
      )
    );
  }

  async function clearSecurityEcosystemFilter(): Promise<void> {
    await goto(
      buildOrgSecurityPath(
        slug,
        {
          severities: securityView.severities,
          ecosystem: '',
          packageQuery: securityView.packageQuery,
        },
        $page.url.searchParams
      )
    );
  }

  async function clearSecurityPackageFilter(): Promise<void> {
    await goto(
      buildOrgSecurityPath(
        slug,
        {
          severities: securityView.severities,
          ecosystem: securityView.ecosystem,
          packageQuery: '',
        },
        $page.url.searchParams
      )
    );
  }

  async function handleExportSecurity(): Promise<void> {
    exportingSecurity = true;

    try {
      const csv = await exportOrgSecurityFindingsCsv(
        slug,
        buildSecurityExportQuery(securityView)
      );

      downloadTextFile(
        buildOrgSecurityExportFilename(
          slug,
          {
            severities: securityView.severities,
            ecosystem: securityView.ecosystem,
            packageQuery: securityView.packageQuery,
          },
          new Date()
        ),
        csv,
        'text/csv;charset=utf-8'
      );
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(
          caughtError,
          'Failed to export security findings.'
        ),
      });
    } finally {
      exportingSecurity = false;
    }
  }

  async function handleProfileUpdate(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!org) {
      return;
    }

    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await updateOrg(slug, {
        description: normalizeFormOptionalText(formData.get('description')),
        website: normalizeFormOptionalText(formData.get('website')),
        email: normalizeFormOptionalText(formData.get('email')),
        mfaRequired: formData.has('mfa_required'),
      });
      await loadOrganizationPage({ notice: 'Organization profile updated.' });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(
          caughtError,
          'Failed to update organization profile.'
        ),
      });
    }
  }

  async function handleInviteMember(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await sendInvitation(slug, {
        usernameOrEmail:
          formData.get('username_or_email')?.toString().trim() || '',
        role: formData.get('role')?.toString() || 'viewer',
        expiresInDays:
          Number(formData.get('expires_in_days')?.toString() || '7') || 7,
      });

      (event.currentTarget as HTMLFormElement).reset();
      await loadOrganizationPage({ notice: 'Invitation sent successfully.' });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to send invitation.'),
      });
    }
  }

  async function handleAddMember(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await addMember(slug, {
        username: formData.get('username')?.toString().trim() || '',
        role: formData.get('role')?.toString() || 'viewer',
      });

      (event.currentTarget as HTMLFormElement).reset();
      await loadOrganizationPage({ notice: 'Member added successfully.' });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to add member.'),
      });
    }
  }

  async function handleTransferOwnership(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const username = resolveOrgMemberPickerInput(
      formData.get('username')?.toString() || '',
      ownershipMemberOptions
    );

    if (!ownershipTransferConfirmed) {
      notice = null;
      error = OWNERSHIP_TRANSFER_CONFIRMATION_MESSAGE;
      return;
    }

    transferringOwnership = true;
    notice = null;
    error = null;

    try {
      const result: TransferOwnershipResult = await transferOwnership(slug, {
        username,
      });

      await loadOrganizationPage({
        notice: `Ownership transferred to @${result.new_owner?.username || 'the selected user'}.`,
      });
    } catch (caughtError: unknown) {
      error = toErrorMessage(
        caughtError,
        'Failed to transfer organization ownership.'
      );
      transferringOwnership = false;
    }
  }

  function openOwnershipTransferConfirmation(): void {
    ownershipTransferConfirmationOpen = true;
    ownershipTransferConfirmed = false;
    transferringOwnership = false;
    notice = null;
    error = null;
  }

  function cancelOwnershipTransferConfirmation(): void {
    ownershipTransferConfirmationOpen = false;
    ownershipTransferConfirmed = false;
    transferringOwnership = false;
    error = null;
  }

  function openInvitationRevokeConfirmation(invitationId: string): void {
    invitationRevokeTargetId = invitationId;
    invitationRevokeConfirmed = false;
    revokingInvitationId = null;
    notice = null;
    error = null;
  }

  function cancelInvitationRevokeConfirmation(): void {
    invitationRevokeTargetId = null;
    invitationRevokeConfirmed = false;
    revokingInvitationId = null;
    error = null;
  }

  async function handleRevokeInvitation(
    event: SubmitEvent,
    invitationId: string
  ): Promise<void> {
    event.preventDefault();

    if (!invitationRevokeConfirmed) {
      notice = null;
      error = INVITATION_REVOKE_CONFIRMATION_MESSAGE;
      return;
    }

    revokingInvitationId = invitationId;
    notice = null;
    error = null;

    try {
      await revokeInvitation(slug, invitationId);
      await loadOrganizationPage({ notice: 'Invitation revoked.' });
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to revoke invitation.');
      revokingInvitationId = null;
    }
  }

  async function handleUpdateMemberRole(
    event: SubmitEvent,
    username: string,
    currentRole: string
  ): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const role = formData.get('role')?.toString().trim() || 'viewer';

    if (role === currentRole) {
      await loadOrganizationPage({
        notice: `@${username} already has the ${formatRole(role)} role.`,
      });
      return;
    }

    try {
      await addMember(slug, { username, role });
      await loadOrganizationPage({
        notice: `Updated @${username} to ${formatRole(role)}.`,
      });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to update member role.'),
      });
    }
  }

  function openMemberRemoveConfirmation(username: string): void {
    memberRemoveTargetUsername = username;
    memberRemoveConfirmed = false;
    removingMemberUsername = null;
    notice = null;
    error = null;
  }

  function cancelMemberRemoveConfirmation(): void {
    memberRemoveTargetUsername = null;
    memberRemoveConfirmed = false;
    removingMemberUsername = null;
    error = null;
  }

  async function handleRemoveMember(
    event: SubmitEvent,
    username: string
  ): Promise<void> {
    event.preventDefault();

    if (!memberRemoveConfirmed) {
      notice = null;
      error = MEMBER_REMOVE_CONFIRMATION_MESSAGE;
      return;
    }

    removingMemberUsername = username;
    notice = null;
    error = null;

    try {
      await removeMember(slug, username);
      await loadOrganizationPage({
        notice: `Removed @${username} from the organization.`,
      });
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to remove member.');
      removingMemberUsername = null;
    }
  }

  async function handleCreateTeam(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await createTeam(slug, {
        name: formData.get('name')?.toString().trim() || '',
        slug: formData.get('team_slug')?.toString().trim() || '',
        description:
          normalizeFormOptionalText(formData.get('description')) || undefined,
      });

      (event.currentTarget as HTMLFormElement).reset();
      await loadOrganizationPage({ notice: 'Team created successfully.' });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to create team.'),
      });
    }
  }

  function openTeamDeleteConfirmation(teamSlug: string): void {
    teamDeleteTargetSlug = teamSlug;
    teamDeleteConfirmed = false;
    deletingTeamSlug = null;
    notice = null;
    error = null;
  }

  function cancelTeamDeleteConfirmation(): void {
    teamDeleteTargetSlug = null;
    teamDeleteConfirmed = false;
    deletingTeamSlug = null;
    error = null;
  }

  async function handleDeleteTeam(event: SubmitEvent, teamSlug: string): Promise<void> {
    event.preventDefault();

    if (!teamDeleteConfirmed) {
      notice = null;
      error = TEAM_DELETE_CONFIRMATION_MESSAGE;
      return;
    }

    deletingTeamSlug = teamSlug;
    notice = null;
    error = null;

    try {
      await deleteTeam(slug, teamSlug);
      await loadOrganizationPage({ notice: `Deleted team ${teamSlug}.` });
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to delete team.');
      deletingTeamSlug = null;
    }
  }

  async function handleCreateNamespace(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!org?.id?.trim()) {
      await loadOrganizationPage({
        error:
          'Failed to create the namespace claim because the organization id is unavailable.',
      });
      return;
    }

    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const ecosystem =
      formData.get('ecosystem')?.toString().trim().toLowerCase() || '';
    const namespace = formData.get('namespace')?.toString().trim() || '';

    if (!ecosystem || !namespace) {
      await loadOrganizationPage({
        error: 'Select an ecosystem and namespace first.',
      });
      return;
    }

    try {
      await createNamespaceClaim({ ecosystem, namespace, ownerOrgId: org.id });
      (event.currentTarget as HTMLFormElement).reset();
      await loadOrganizationPage({
        notice: `Created the ${ecosystemLabel(ecosystem)} namespace claim ${namespace}.`,
      });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to create namespace claim.'),
      });
    }
  }

  function openNamespaceDeleteConfirmation(claimId: string): void {
    namespaceDeleteTargetId = claimId;
    namespaceDeleteConfirmed = false;
    deletingNamespaceClaimId = null;
    notice = null;
    error = null;
  }

  function cancelNamespaceDeleteConfirmation(): void {
    namespaceDeleteTargetId = null;
    namespaceDeleteConfirmed = false;
    deletingNamespaceClaimId = null;
    error = null;
  }

  async function handleDeleteNamespace(
    event: SubmitEvent,
    claimId: string | null | undefined,
    namespace: string
  ): Promise<void> {
    event.preventDefault();

    if (!claimId) {
      await loadOrganizationPage({
        error: 'Failed to delete namespace claim because the claim id is unavailable.',
      });
      return;
    }

    if (!namespaceDeleteConfirmed) {
      notice = null;
      error = NAMESPACE_DELETE_CONFIRMATION_MESSAGE;
      return;
    }

    deletingNamespaceClaimId = claimId;
    notice = null;
    error = null;

    try {
      await deleteNamespaceClaim(claimId);
      await loadOrganizationPage({
        notice: `Deleted namespace claim ${namespace}.`,
      });
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to delete namespace claim.');
      deletingNamespaceClaimId = null;
    }
  }

  async function handleNamespaceTransfer(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const claimId = formData.get('claim_id')?.toString().trim() || '';
    const targetOrgSlug =
      formData.get('target_org_slug')?.toString().trim() || '';

    if (!claimId) {
      notice = null;
      error = 'Select a namespace claim to transfer.';
      return;
    }

    if (!targetOrgSlug) {
      notice = null;
      error = 'Select a target organization.';
      return;
    }

    if (!namespaceTransferConfirmed) {
      notice = null;
      error = NAMESPACE_TRANSFER_CONFIRMATION_MESSAGE;
      return;
    }

    transferringNamespaceOwnership = true;
    notice = null;
    error = null;

    try {
      const result: NamespaceTransferOwnershipResult = await transferNamespaceClaim(
        claimId,
        {
          targetOrgSlug,
        }
      );
      const namespace =
        result.namespace_claim?.namespace ||
        namespaceClaims.find((claim) => claim.id === claimId)?.namespace ||
        'namespace claim';
      await loadOrganizationPage({
        notice: `Transferred ${namespace} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
      });
    } catch (caughtError: unknown) {
      error = toErrorMessage(
        caughtError,
        'Failed to transfer namespace claim ownership.'
      );
      transferringNamespaceOwnership = false;
    }
  }

  function openNamespaceTransferConfirmation(): void {
    namespaceTransferConfirmationOpen = true;
    namespaceTransferConfirmed = false;
    transferringNamespaceOwnership = false;
    notice = null;
    error = null;
  }

  function cancelNamespaceTransferConfirmation(): void {
    namespaceTransferConfirmationOpen = false;
    namespaceTransferConfirmed = false;
    transferringNamespaceOwnership = false;
    error = null;
  }

  async function handleCreateRepository(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!org?.id?.trim()) {
      await loadOrganizationPage({
        error:
          'Failed to create the repository because the organization id is unavailable.',
      });
      return;
    }

    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await createRepository({
        name: formData.get('name')?.toString().trim() || '',
        slug: formData.get('slug')?.toString().trim() || '',
        kind: formData.get('kind')?.toString().trim() || 'public',
        visibility: formData.get('visibility')?.toString().trim() || 'public',
        description: normalizeFormOptionalText(formData.get('description')),
        upstreamUrl: normalizeFormOptionalText(formData.get('upstream_url')),
        ownerOrgId: org.id,
      });

      (event.currentTarget as HTMLFormElement).reset();
      await loadOrganizationPage({
        notice: 'Repository created successfully.',
      });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to create repository.'),
      });
    }
  }

  async function handleCreatePackage(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!selectedPackageCreationRepository) {
      notice = null;
      error =
        creatableRepositories.length === 0
          ? 'Create an eligible repository before creating a package.'
          : 'Select a repository for the new package.';
      return;
    }

    const packageName = newPackageName.trim();
    if (!packageName) {
      notice = null;
      error = 'Enter a package name.';
      return;
    }

    const ecosystem = newPackageEcosystem.trim().toLowerCase();
    const repositorySlug = selectedPackageCreationRepository.slug;
    const repositoryName =
      selectedPackageCreationRepository.name ||
      selectedPackageCreationRepository.slug;

    creatingPackage = true;
    notice = null;
    error = null;

    try {
      const result = await createPackage({
        ecosystem,
        name: packageName,
        repositorySlug,
        visibility: newPackageVisibility || undefined,
        displayName: newPackageDisplayName,
        description: newPackageDescription,
      });

      newPackageEcosystem = DEFAULT_PACKAGE_ECOSYSTEM;
      newPackageName = '';
      newPackageVisibility = '';
      newPackageDisplayName = '';
      newPackageDescription = '';

      await loadOrganizationPage({
        notice: `Created ${ecosystemLabel(result.ecosystem || ecosystem)} package ${result.name || packageName} in ${repositoryName}.`,
      });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to create package.'),
      });
    } finally {
      creatingPackage = false;
    }
  }

  async function handleUpdateRepository(
    event: SubmitEvent,
    repositorySlug: string
  ): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);

    try {
      await updateRepository(repositorySlug, {
        description: formData.get('description')?.toString().trim() || '',
        visibility: formData.get('visibility')?.toString().trim() || 'public',
        upstreamUrl: formData.get('upstream_url')?.toString().trim() || '',
      });

      await loadOrganizationPage({
        notice: `Updated repository ${repositorySlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadOrganizationPage({
        error: toErrorMessage(caughtError, 'Failed to update repository.'),
      });
    }
  }

  async function handleRepositoryTransfer(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const repositorySlug =
      formData.get('repository_slug')?.toString().trim() || '';
    const targetOrgSlug =
      formData.get('target_org_slug')?.toString().trim() || '';

    if (!repositorySlug) {
      notice = null;
      error = 'Select a repository to transfer.';
      return;
    }

    if (!targetOrgSlug) {
      notice = null;
      error = 'Select a target organization.';
      return;
    }

    if (!repositoryTransferConfirmed) {
      notice = null;
      error = REPOSITORY_TRANSFER_CONFIRMATION_MESSAGE;
      return;
    }

    transferringRepositoryOwnership = true;
    notice = null;
    error = null;

    try {
      const result = await transferRepositoryOwnership(repositorySlug, {
        targetOrgSlug,
      });

      await loadOrganizationPage({
        notice: `Transferred ${result.repository?.name || result.repository?.slug || repositorySlug} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
      });
    } catch (caughtError: unknown) {
      error = toErrorMessage(
        caughtError,
        'Failed to transfer repository ownership.'
      );
      transferringRepositoryOwnership = false;
    }
  }

  function openRepositoryTransferConfirmation(): void {
    repositoryTransferConfirmationOpen = true;
    repositoryTransferConfirmed = false;
    transferringRepositoryOwnership = false;
    notice = null;
    error = null;
  }

  function cancelRepositoryTransferConfirmation(): void {
    repositoryTransferConfirmationOpen = false;
    repositoryTransferConfirmed = false;
    transferringRepositoryOwnership = false;
    error = null;
  }

  async function handlePackageTransfer(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const packageTarget = decodePackageSelection(
      formData.get('package_key')?.toString().trim() || ''
    );
    const targetOrgSlug =
      formData.get('target_org_slug')?.toString().trim() || '';

    if (!packageTarget) {
      notice = null;
      error = 'Select a package to transfer.';
      return;
    }

    if (!targetOrgSlug) {
      notice = null;
      error = 'Select a target organization.';
      return;
    }

    if (!packageTransferConfirmed) {
      notice = null;
      error = PACKAGE_TRANSFER_CONFIRMATION_MESSAGE;
      return;
    }

    transferringPackageOwnershipFlow = true;
    notice = null;
    error = null;

    try {
      const result = await transferPackageOwnership(
        packageTarget.ecosystem,
        packageTarget.name,
        {
          targetOrgSlug,
        }
      );

      await loadOrganizationPage({
        notice: `Transferred ${packageTarget.name} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
      });
    } catch (caughtError: unknown) {
      error = toErrorMessage(
        caughtError,
        'Failed to transfer package ownership.'
      );
      transferringPackageOwnershipFlow = false;
    }
  }

  function openPackageTransferConfirmation(): void {
    packageTransferConfirmationOpen = true;
    packageTransferConfirmed = false;
    transferringPackageOwnershipFlow = false;
    notice = null;
    error = null;
  }

  function cancelPackageTransferConfirmation(): void {
    packageTransferConfirmationOpen = false;
    packageTransferConfirmed = false;
    transferringPackageOwnershipFlow = false;
    error = null;
  }

  function getEligibleTeamMemberOptions(
    teamSlug: string
  ): OrgMemberPickerOption[] {
    const teamMembers = teamMembersBySlug[teamSlug]?.members || [];
    return buildEligibleTeamMemberOptions(members, teamMembers);
  }

  function hasRepositorySlug(
    repository: OrgRepositorySummary
  ): repository is OrgRepositorySummary & { slug: string } {
    return (
      typeof repository.slug === 'string' && repository.slug.trim().length > 0
    );
  }

  function hasRepositoryCoverageSlug(
    entry: OrgRepositoryPackageCoverageResponse['repositories'][number]
  ): entry is {
    repository_slug: string;
    packages: RepositoryPackageSummary[];
  } {
    return (
      typeof entry.repository_slug === 'string' &&
      entry.repository_slug.trim().length > 0
    );
  }

  function normalizeFormOptionalText(
    value: FormDataEntryValue | null | undefined
  ): string | null {
    if (typeof value !== 'string') {
      return null;
    }

    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }

  const teamManagement = createTeamManagementController({
    getOrgSlug: () => slug,
    reload: loadOrganizationPage,
    resolveEligibleTeamMemberOptions: getEligibleTeamMemberOptions,
    toErrorMessage,
  });

  function formatRole(role: string): string {
    return role
      .split('_')
      .filter(Boolean)
      .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
      .join(' ');
  }

  function formatIdentifierLabel(value: string): string {
    return value
      .split('_')
      .filter(Boolean)
      .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
      .join(' ');
  }

  function formatAuditActor(log: OrgAuditLog): string | null {
    const displayName = log.actor_display_name?.trim();
    const username = log.actor_username?.trim();

    if (displayName && username && displayName !== username) {
      return `${displayName} (@${username})`;
    }
    if (displayName) {
      return displayName;
    }
    if (username) {
      return `@${username}`;
    }

    return null;
  }

  function formatAuditFilterSummary(): string {
    const base = `Showing page ${auditView.page} with up to ${ORG_AUDIT_PAGE_SIZE} events`;
    const filters: string[] = [];

    if (auditView.action) {
      filters.push(formatAuditActionLabel(auditView.action).toLowerCase());
    }
    if (auditView.actorUserId) {
      filters.push(
        `actor ${formatAuditActorQueryLabel(auditView.actorUsername)}`
      );
    }
    if (auditView.occurredFrom || auditView.occurredUntil) {
      if (auditView.occurredFrom && auditView.occurredUntil) {
        filters.push(
          `UTC dates ${auditView.occurredFrom} through ${auditView.occurredUntil}`
        );
      } else if (auditView.occurredFrom) {
        filters.push(`UTC dates from ${auditView.occurredFrom}`);
      } else if (auditView.occurredUntil) {
        filters.push(`UTC dates through ${auditView.occurredUntil}`);
      }
    }

    return filters.length > 0
      ? `${base}, filtered by ${filters.join(', ')}.`
      : `${base}.`;
  }

  function downloadTextFile(
    filename: string,
    contents: string,
    contentType: string
  ): void {
    const blob = new Blob([contents], { type: contentType });
    const objectUrl = window.URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = objectUrl;
    link.download = filename;
    link.style.display = 'none';
    document.body.appendChild(link);
    link.click();
    link.remove();
    window.URL.revokeObjectURL(objectUrl);
  }

  function formatOpenFindingLabel(count: number): string {
    return count === 1
      ? '1 open finding'
      : `${formatNumber(count)} open findings`;
  }

  function formatSecurityFilterSummary(): string {
    const filters: string[] = [];

    if (securityView.severities.length > 0) {
      filters.push(
        `severity ${securityView.severities.map(formatIdentifierLabel).join(', ')}`
      );
    }
    if (securityView.ecosystem) {
      filters.push(`${ecosystemLabel(securityView.ecosystem)} packages`);
    }
    if (securityView.packageQuery) {
      filters.push(`package matching "${securityView.packageQuery}"`);
    }

    return filters.length > 0
      ? `Showing unresolved findings filtered by ${filters.join(', ')}.`
      : 'Showing unresolved findings across all visible packages.';
  }

  async function searchAuditActors(query: string): Promise<void> {
    const requestId = ++auditActorSearchRequest;
    auditActorSearchInFlight = true;

    try {
      const response = await searchOrgMembers(slug, query);
      if (requestId !== auditActorSearchRequest) {
        return;
      }

      auditActorRemoteOptions = buildRemoteAuditActorOptions(response.members);
    } catch (caughtError: unknown) {
      if (requestId === auditActorSearchRequest) {
        auditActorRemoteOptions = [];
      }
    } finally {
      if (requestId === auditActorSearchRequest) {
        auditActorSearchInFlight = false;
      }
    }
  }

  async function handleToggleOrgSecurityFindings(
    securityPackage: OrgSecurityPackageSummary
  ): Promise<void> {
    const packageKey = getSecurityPackageKey(securityPackage);
    const currentState = getOrgSecurityFindingState(securityPackage);

    if (currentState.expanded) {
      updateOrgSecurityFindingState(packageKey, { expanded: false });
      return;
    }

    updateOrgSecurityFindingState(packageKey, {
      expanded: true,
      loading: true,
      load_error: null,
      error: null,
      notice: null,
    });

    if (!securityPackage.ecosystem || !securityPackage.name) {
      updateOrgSecurityFindingState(packageKey, {
        loading: false,
        load_error:
          'Failed to load findings because the package identity is unavailable.',
      });
      return;
    }

    try {
      const findings = await listSecurityFindings(
        securityPackage.ecosystem,
        securityPackage.name,
        {
          includeResolved: true,
        }
      );
      updateOrgSecurityFindingState(packageKey, {
        findings: sortOrgSecurityFindings(findings),
        loading: false,
        load_error: null,
      });
    } catch (caughtError: unknown) {
      updateOrgSecurityFindingState(packageKey, {
        findings: [],
        loading: false,
        load_error: toErrorMessage(
          caughtError,
          'Failed to load package findings.'
        ),
      });
    }
  }

  async function handleToggleOrgFindingResolution(
    securityPackage: OrgSecurityPackageSummary,
    finding: SecurityFinding
  ): Promise<void> {
    const packageKey = getSecurityPackageKey(securityPackage);
    const currentState = getOrgSecurityFindingState(securityPackage);
    if (currentState.updatingFindingId) {
      return;
    }
    if (!securityPackage.ecosystem || !securityPackage.name) {
      updateOrgSecurityFindingState(packageKey, {
        error:
          'Failed to update the security finding because the package identity is unavailable.',
      });
      return;
    }

    const targetIsResolved = !finding.is_resolved;
    const rawNote = currentState.findingNotes[finding.id] ?? '';
    const trimmedNote = rawNote.trim();
    if (trimmedNote.length > 2000) {
      updateOrgSecurityFindingState(packageKey, {
        error: 'Security finding note must be 2000 characters or fewer.',
      });
      return;
    }

    updateOrgSecurityFindingState(packageKey, {
      updatingFindingId: finding.id,
      error: null,
      notice: null,
    });

    try {
      const updated = await updateSecurityFinding(
        securityPackage.ecosystem,
        securityPackage.name,
        finding.id,
        {
          isResolved: targetIsResolved,
          note: trimmedNote.length > 0 ? trimmedNote : undefined,
        }
      );
      const latestState = getOrgSecurityFindingState(securityPackage);
      updateOrgSecurityFindingState(packageKey, {
        findings: mergeUpdatedOrgSecurityFinding(latestState.findings, updated, {
          includeResolved: true,
        }),
        updatingFindingId: null,
        notice: targetIsResolved
          ? 'Finding marked as resolved.'
          : 'Finding reopened.',
        findingNotes: {
          ...latestState.findingNotes,
          [finding.id]: '',
        },
      });
      await reloadSecurityOverview();
    } catch (caughtError: unknown) {
      updateOrgSecurityFindingState(packageKey, {
        updatingFindingId: null,
        error:
          caughtError instanceof ApiError
            ? caughtError.message
            : 'Failed to update the security finding.',
      });
    }
  }
</script>

<svelte:head>
  <title>Organization — Publaryn</title>
</svelte:head>

{#if loading}
  <div class="loading"><span class="spinner"></span> Loading organization…</div>
{:else if notFound}
  <div class="empty-state mt-6">
    <h2>Organization not found</h2>
    <p>@{slug} does not exist or is no longer available.</p>
    <a
      href="/search"
      class="btn btn-primary mt-4"
      data-sveltekit-preload-data="hover">Search packages</a
    >
  </div>
{:else if loadError || !org}
  <div class="mt-6">
    <div class="alert alert-error">
      {loadError || 'Failed to load organization.'}
    </div>
  </div>
{:else}
  <div class="mt-6 settings-page">
    {#if notice}<div class="alert alert-success">{notice}</div>{/if}
    {#if error}<div class="alert alert-error">{error}</div>{/if}

    <section class="card org-hero">
      <div class="org-hero__header">
        <div class="org-hero__copy">
          <div class="org-hero__eyebrow">Organization workspace</div>
          <div class="pkg-header">
            <h1 class="pkg-header__name">{org.name || slug}</h1>
            {#if org.is_verified}<span class="badge badge-verified"
                >Verified</span
              >{/if}
            {#if org.mfa_required}<span class="badge badge-ecosystem"
                >MFA required</span
              >{/if}
          </div>
          <p class="text-muted">@{org.slug || slug}</p>
          <p class="settings-copy">
            {org.description || 'No organization description yet.'}
          </p>
          <p class="settings-copy">
            {#if membership}
              You are a <strong
                >{formatRole(membership.role || 'viewer')}</strong
              > in this organization.
            {:else if isAuthenticated}
              You are signed in but not currently a member of this organization.
            {:else}
              You are viewing this organization as a public visitor.
            {/if}
          </p>
        </div>

        <div class="org-hero__meta">
          {#if org.website}<a
              href={org.website}
              target="_blank"
              rel="noopener noreferrer">{org.website}</a
            >{/if}
          {#if org.email}<a href={`mailto:${org.email}`}>{org.email}</a>{/if}
          {#if org.created_at}<span>Created {formatDate(org.created_at)}</span
            >{/if}
        </div>
      </div>

      <div class="org-kpi-grid">
        <div class="org-kpi">
          <span class="org-kpi__value"
            >{canViewPeopleWorkspace ? members.length : '—'}</span
          ><span class="org-kpi__label"
            >{canViewPeopleWorkspace ? 'Members' : 'Member directory'}</span
          >
        </div>
        <div class="org-kpi">
          <span class="org-kpi__value"
            >{canViewPeopleWorkspace ? teams.length : '—'}</span
          ><span class="org-kpi__label"
            >{canViewPeopleWorkspace ? 'Teams' : 'Team directory'}</span
          >
        </div>
        <div class="org-kpi">
          <span class="org-kpi__value">{packages.length}</span><span
            class="org-kpi__label">Visible packages</span
          >
        </div>
        <div class="org-kpi">
          <span class="org-kpi__value"
            >{formatRole(membership?.role || 'public')}</span
          ><span class="org-kpi__label">Your access</span>
        </div>
      </div>
    </section>

    {#if canViewAudit}
      <section class="card settings-section">
        <div class="org-section-header">
          <div>
            <h2>Activity log</h2>
            <p class="settings-copy">
              Organization governance history with filters and CSV export.
            </p>
          </div>
        </div>

        <OrgAuditFilterControls
          actionOptions={ORG_AUDIT_ACTION_OPTIONS}
          actionValue={auditView.action}
          bind:actorInput={auditActorInput}
          actorOptions={auditActorOptions}
          occurredFrom={auditView.occurredFrom}
          occurredUntil={auditView.occurredUntil}
          exporting={exportingAudit}
          summary={formatAuditFilterSummary()}
          showActionClear={Boolean(auditView.action)}
          showActorClear={Boolean(auditView.actorUserId)}
          showDateClear={Boolean(auditView.occurredFrom || auditView.occurredUntil)}
          handleSubmit={handleAuditFilterSubmit}
          handleExport={handleExportAudit}
          clearAction={clearAuditActionFilter}
          clearActor={clearAuditActorFilter}
          clearDates={clearAuditDateFilter}
        />

        {#if auditError}
          <div class="alert alert-error">{auditError}</div>
        {:else if auditLogs.length === 0}
          <div class="empty-state">
            <h3>No matching activity</h3>
            <p>Try adjusting the filters or checking earlier pages.</p>
          </div>
        {:else}
          <div class="token-list">
            {#each auditLogs as log}
              {@const actor = formatAuditActor(log)}
              {@const target = formatAuditTarget(log)}
              {@const summary = formatAuditSummary(log)}
              {@const actorUserId = normalizeAuditActorUserId(
                log.actor_user_id
              )}
              {@const actorUsername = normalizeAuditActorUsername(
                log.actor_username
              )}
              <div class="token-row">
                <div class="token-row__main">
                  <div class="token-row__title">
                    {formatAuditActionLabel(log.action || 'activity')}
                  </div>
                  <div class="token-row__meta">
                    {#if actor}<span>by {actor}</span>{/if}
                    {#if target}<span>{target}</span>{/if}
                    {#if log.occurred_at}<span
                        >{formatDate(log.occurred_at)}</span
                      >{/if}
                  </div>
                  {#if summary}<p class="settings-copy">{summary}</p>{/if}
                </div>
                {#if actorUserId && actorUserId !== auditView.actorUserId}
                  <div class="token-row__actions">
                    <button
                      class="btn btn-secondary btn-sm"
                      type="button"
                      on:click={() =>
                        focusAuditActor(actorUserId, actorUsername)}
                      >Only this actor</button
                    >
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if !auditError && (auditView.page > 1 || auditHasNext)}
          <div class="pagination">
            {#if auditView.page > 1}<button
                class="btn btn-secondary btn-sm"
                type="button"
                on:click={() => goToAuditPage(auditView.page - 1)}
                >← Prev</button
              >{/if}
            <span class="current">Page {auditView.page}</span>
            {#if auditHasNext}<button
                class="btn btn-secondary btn-sm"
                type="button"
                on:click={() => goToAuditPage(auditView.page + 1)}
                >Next →</button
              >{/if}
          </div>
        {/if}
      </section>
    {/if}

    {#if canAdminister}
      <section class="card settings-section">
        <h2>Organization profile</h2>
        <form on:submit={handleProfileUpdate}>
          <div class="grid gap-4 xl:grid-cols-2">
            <div class="form-group">
              <label for="org-profile-name">Organization name</label>
              <input
                id="org-profile-name"
                class="form-input"
                value={org.name || slug}
                disabled
              />
            </div>
            <div class="form-group">
              <label for="org-profile-slug">Organization slug</label>
              <input
                id="org-profile-slug"
                class="form-input"
                value={org.slug || slug}
                disabled
              />
            </div>
          </div>
          <div class="form-group">
            <label for="org-profile-description">Description</label>
            <textarea
              id="org-profile-description"
              name="description"
              class="form-input"
              rows="3">{org.description || ''}</textarea
            >
          </div>
          <div class="grid gap-4 xl:grid-cols-2">
            <div class="form-group">
              <label for="org-profile-website">Website</label>
              <input
                id="org-profile-website"
                name="website"
                class="form-input"
                type="url"
                value={org.website || ''}
                placeholder="https://example.com"
              />
            </div>
            <div class="form-group">
              <label for="org-profile-email">Email</label>
              <input
                id="org-profile-email"
                name="email"
                class="form-input"
                type="email"
                value={org.email || ''}
                placeholder="registry@example.com"
              />
            </div>
          </div>
          <div class="form-group">
            <label for="org-profile-mfa-required"
              >Organization security policy</label
            >
            <label
              for="org-profile-mfa-required"
              class="settings-copy"
              style="display:flex; gap:12px; align-items:flex-start; margin-top:0;"
            >
              <input
                id="org-profile-mfa-required"
                name="mfa_required"
                type="checkbox"
                checked={Boolean(org.mfa_required)}
              />
              <span>
                <strong>Require MFA for maintainers</strong><br />
                Record an organization-level MFA requirement for elevated roles
                including owners, admins, maintainers, publishers, and security
                managers.
              </span>
            </label>
          </div>
          <button type="submit" class="btn btn-primary">Save profile</button>
        </form>
      </section>
    {/if}

    {#if canManageInvitations || canManageMembers}
      <div class="settings-grid">
        {#if canManageInvitations}
          <section class="card settings-section">
            <h2>Invite a member</h2>
            <form on:submit={handleInviteMember}>
              <div class="form-group">
                <label for="org-invite-target">Username or email</label>
                <input
                  id="org-invite-target"
                  name="username_or_email"
                  class="form-input"
                  placeholder="alice or alice@example.com"
                  required
                />
              </div>
              <div class="grid gap-4 xl:grid-cols-2">
                <div class="form-group">
                  <label for="org-invite-role">Role</label>
                  <select id="org-invite-role" name="role" class="form-input">
                    {#each ORG_ROLE_OPTIONS as role}
                      <option value={role.value}>{role.label}</option>
                    {/each}
                  </select>
                </div>
                <div class="form-group">
                  <label for="org-invite-expiry">Expires in days</label>
                  <input
                    id="org-invite-expiry"
                    name="expires_in_days"
                    type="number"
                    min="1"
                    max="30"
                    class="form-input"
                    value="7"
                  />
                </div>
              </div>
              <button type="submit" class="btn btn-primary"
                >Send invitation</button
              >
            </form>
          </section>
        {/if}

        {#if canManageMembers}
          <section class="card settings-section">
            <h2>Add member directly</h2>
            <form on:submit={handleAddMember}>
              <div class="form-group">
                <label for="org-member-username">Username</label>
                <input
                  id="org-member-username"
                  name="username"
                  class="form-input"
                  placeholder="alice"
                  required
                />
              </div>
              <div class="form-group">
                <label for="org-member-role">Role</label>
                <select id="org-member-role" name="role" class="form-input">
                  {#each ORG_ROLE_OPTIONS as role}
                    <option value={role.value}>{role.label}</option>
                  {/each}
                </select>
              </div>
              <button type="submit" class="btn btn-primary">Add member</button>
            </form>
          </section>
        {/if}
      </div>
    {/if}

    {#if canTransferOwnership}
      <section class="card settings-section">
        <h2>Transfer ownership</h2>
        <div class="alert alert-warning">
          <strong>This action is immediate.</strong> You will be demoted to Admin.
        </div>
        {#if ownershipMemberOptions.length === 0}
          <p class="settings-copy">
            Add another organization member before transferring ownership.
          </p>
        {:else}
          <form on:submit={handleTransferOwnership}>
            <div class="form-group">
              <label for="org-transfer-owner">New owner username</label>
              <input
                id="org-transfer-owner"
                name="username"
                class="form-input"
                list="org-transfer-owner-options"
                placeholder="Search member username or paste user id"
                autocomplete="off"
                required
              />
              <datalist id="org-transfer-owner-options">
                {#each ownershipMemberOptions as option}
                  <option value={option.username}>{option.label}</option>
                  <option value={option.userId}>{option.label}</option>
                {/each}
              </datalist>
            </div>
            {#if ownershipTransferConfirmationOpen}
              <div
                class="alert alert-warning"
                id="org-ownership-transfer-confirmation"
                style="margin-bottom:12px;"
              >
                <label class="flex items-start gap-2">
                  <input
                    id="org-ownership-transfer-confirm"
                    bind:checked={ownershipTransferConfirmed}
                    type="checkbox"
                    name="confirm"
                    disabled={transferringOwnership}
                  />
                  <span
                    >I understand this transfer is immediate and irreversible.</span
                  >
                </label>
                <div class="token-row__actions" style="margin-top:12px;">
                  <button
                    id="org-ownership-transfer-submit"
                    type="submit"
                    class="btn btn-danger"
                    disabled={transferringOwnership}
                  >
                    {transferringOwnership
                      ? 'Transferring...'
                      : 'Transfer ownership'}
                  </button>
                  <button
                    type="button"
                    class="btn btn-secondary"
                    on:click={cancelOwnershipTransferConfirmation}
                    disabled={transferringOwnership}
                  >
                    Keep current owner
                  </button>
                </div>
              </div>
            {:else}
              <button
                id="org-ownership-transfer-toggle"
                type="button"
                class="btn btn-danger"
                aria-label="Review organization ownership transfer"
                on:click={openOwnershipTransferConfirmation}
              >
                Transfer ownership...
              </button>
            {/if}
          </form>
        {/if}
      </section>
    {/if}

    {#if canManageInvitations}
      <section class="card settings-section">
        <div class="org-section-header">
          <div>
            <h2>Invitations</h2>
            <p class="settings-copy">
              Track active invitations and recent outcomes from one place.
            </p>
          </div>
          {#if historicalInvitations.length > 0}
            <button
              type="button"
              class="btn btn-secondary btn-sm"
              on:click={() => (showInvitationHistory = !showInvitationHistory)}
            >
              {showInvitationHistory
                ? 'Hide history'
                : `Show history (${historicalInvitations.length})`}
            </button>
          {/if}
        </div>

        <div class="token-row__scopes" style="margin-bottom:1rem;">
          <span class="badge badge-ecosystem"
            >{activeInvitations.length} active</span
          >
          {#if invitationStatusCounts.accepted > 0}<span
              class="badge badge-ecosystem"
              >{invitationStatusCounts.accepted} accepted</span
            >{/if}
          {#if invitationStatusCounts.declined > 0}<span
              class="badge badge-ecosystem"
              >{invitationStatusCounts.declined} declined</span
            >{/if}
          {#if invitationStatusCounts.revoked > 0}<span
              class="badge badge-ecosystem"
              >{invitationStatusCounts.revoked} revoked</span
            >{/if}
          {#if invitationStatusCounts.expired > 0}<span
              class="badge badge-ecosystem"
              >{invitationStatusCounts.expired} expired</span
            >{/if}
        </div>

        <div class="settings-subsection">
          <h3>Active invitations</h3>
          {#if invitationsError}
            <div class="alert alert-error">{invitationsError}</div>
          {:else if activeInvitations.length === 0}
            <div class="empty-state">
              <h3>No active invitations</h3>
              <p>
                New invitations will appear here until they are accepted,
                declined, revoked, or expire.
              </p>
            </div>
          {:else}
              <div class="token-list">
                {#each activeInvitations as invitation}
                  {@const inviteeLabel = formatOrgInvitationInvitee(invitation)}
                  {@const invitationEvent =
                    describeOrgInvitationEvent(invitation)}
                  <div>
                    <div class="token-row">
                      <div class="token-row__main">
                        <div class="token-row__title">{inviteeLabel}</div>
                        <div class="token-row__meta">
                          {#if invitation.invited_user?.email}<span
                              >{invitation.invited_user?.email}</span
                            >{/if}
                          <span>{formatRole(invitation.role || 'viewer')}</span>
                          <span
                            >sent by @{invitation.invited_by?.username ||
                              'unknown'}</span
                          >
                          <span>sent {formatDate(invitation.created_at)}</span>
                          {#if invitationEvent?.occurredAt}<span
                              >{invitationEvent.label.toLowerCase()}
                              {formatDate(invitationEvent.occurredAt)}</span
                            >{/if}
                        </div>
                        <div class="token-row__scopes">
                          <span class="badge badge-ecosystem"
                            >{formatOrgInvitationStatusLabel(
                              invitation.status
                            )}</span
                          >
                        </div>
                      </div>
                      {#if invitation.id}
                        <div class="token-row__actions">
                          {#if invitationRevokeTargetId === invitation.id}
                            <button
                              class="btn btn-secondary btn-sm"
                              type="button"
                              on:click={cancelInvitationRevokeConfirmation}
                              disabled={revokingInvitationId === invitation.id}
                              >Cancel</button
                            >
                          {:else}
                            <button
                              class="btn btn-secondary btn-sm"
                              id={`invitation-revoke-toggle-${invitation.id}`}
                              type="button"
                              aria-label={`Revoke invitation for ${inviteeLabel}`}
                              on:click={() =>
                                openInvitationRevokeConfirmation(invitation.id || '')}
                              >Revoke...</button
                            >
                          {/if}
                        </div>
                      {/if}
                    </div>
                    {#if invitation.id &&
                      invitationRevokeTargetId === invitation.id}
                      <form
                        class="alert alert-warning mt-4"
                        id={`invitation-revoke-form-${invitation.id}`}
                        on:submit={(event) =>
                          handleRevokeInvitation(event, invitation.id || '')}
                      >
                        <p class="mb-3">
                          Revoking this invitation immediately removes the
                          recipient's ability to accept it.
                        </p>
                        <label class="mb-3 flex items-start gap-2">
                          <input
                            id={`invitation-revoke-confirm-${invitation.id}`}
                            bind:checked={invitationRevokeConfirmed}
                            type="checkbox"
                            name="confirm_revoke"
                            disabled={revokingInvitationId === invitation.id}
                          />
                          <span>
                            I understand revoking this invitation is immediate.
                          </span>
                        </label>
                        <div class="token-row__actions">
                          <button
                            class="btn btn-danger btn-sm"
                            id={`invitation-revoke-submit-${invitation.id}`}
                            type="submit"
                            disabled={revokingInvitationId === invitation.id}
                          >
                            {revokingInvitationId === invitation.id
                              ? 'Revoking…'
                              : 'Revoke invitation'}
                          </button>
                          <button
                            class="btn btn-secondary btn-sm"
                            type="button"
                            on:click={cancelInvitationRevokeConfirmation}
                            disabled={revokingInvitationId === invitation.id}
                          >
                            Keep invitation
                          </button>
                        </div>
                      </form>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>

        {#if showInvitationHistory && historicalInvitations.length > 0}
          <div class="settings-subsection">
            <h3>Recent invitation history</h3>
            <p class="settings-copy">
              Accepted, declined, revoked, and expired invitations stay visible
              here for admin follow-up.
            </p>

            <div class="token-list">
              {#each historicalInvitations as invitation}
                {@const inviteeLabel = formatOrgInvitationInvitee(invitation)}
                {@const invitationEvent =
                  describeOrgInvitationEvent(invitation)}
                <div class="token-row">
                  <div class="token-row__main">
                    <div class="token-row__title">{inviteeLabel}</div>
                    <div class="token-row__meta">
                      {#if invitation.invited_user?.email}<span
                          >{invitation.invited_user?.email}</span
                        >{/if}
                      <span>{formatRole(invitation.role || 'viewer')}</span>
                      <span
                        >sent by @{invitation.invited_by?.username ||
                          'unknown'}</span
                      >
                      <span>sent {formatDate(invitation.created_at)}</span>
                      <span>expires {formatDate(invitation.expires_at)}</span>
                    </div>
                    {#if invitationEvent}<p class="settings-copy">
                        {invitationEvent.label}{#if invitationEvent.occurredAt}
                          {' '}{formatDate(invitationEvent.occurredAt)}{/if}.
                      </p>{/if}
                  </div>
                  <div class="token-row__actions">
                    <span class="badge badge-ecosystem"
                      >{formatOrgInvitationStatusLabel(invitation.status)}</span
                    >
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </section>
    {/if}

    {#if canViewPeopleWorkspace}
      <section class="card settings-section">
        <h2>Members</h2>
        {#if membersError}
          <div class="alert alert-error">{membersError}</div>
        {:else if members.length === 0}
          <div class="empty-state">
            <h3>No members yet</h3>
            <p>This organization has not added any members yet.</p>
          </div>
        {:else}
          <div class="token-list">
            {#each members as member}
              <div>
                <div class="token-row">
                  <div class="token-row__main">
                    <div class="token-row__title">
                      {member.display_name || member.username || 'Unknown member'}
                    </div>
                    <div class="token-row__meta">
                      <span>@{member.username || 'unknown'}</span>
                      <span>{formatRole(member.role || 'viewer')}</span>
                      <span>joined {formatDate(member.joined_at)}</span>
                    </div>
                  </div>
                  {#if canManageMembers && member.role !== 'owner' && member.username}
                    <div class="token-row__actions">
                      <form
                        class="flex flex-wrap items-center gap-2"
                        on:submit={(event) =>
                          handleUpdateMemberRole(
                            event,
                            member.username || '',
                            member.role || 'viewer'
                          )}
                      >
                        <label
                          class="text-sm text-muted"
                          for={`member-role-${member.username || 'member'}`}
                          >Role</label
                        >
                        <select
                          id={`member-role-${member.username || 'member'}`}
                          name="role"
                          class="form-input"
                          style="width:auto; min-width:150px;"
                        >
                          {#each ORG_ROLE_OPTIONS as role}
                            <option
                              value={role.value}
                              selected={role.value === (member.role || 'viewer')}
                              >{role.label}</option
                            >
                          {/each}
                        </select>
                        <button class="btn btn-secondary btn-sm" type="submit"
                          >Save</button
                        >
                      </form>
                      {#if memberRemoveTargetUsername === member.username}
                        <button
                          class="btn btn-secondary btn-sm"
                          type="button"
                          on:click={cancelMemberRemoveConfirmation}
                          disabled={removingMemberUsername === member.username}
                          >Cancel</button
                        >
                      {:else}
                        <button
                          class="btn btn-danger btn-sm"
                          id={`member-remove-toggle-${member.username}`}
                          type="button"
                          aria-label={`Remove member ${member.username}`}
                          on:click={() =>
                            openMemberRemoveConfirmation(member.username || '')}
                          >Remove...</button
                        >
                      {/if}
                    </div>
                  {/if}
                </div>
                {#if canManageMembers &&
                  member.role !== 'owner' &&
                  member.username &&
                  memberRemoveTargetUsername === member.username}
                  <form
                    class="alert alert-warning mt-4"
                    id={`member-remove-form-${member.username}`}
                    on:submit={(event) =>
                      handleRemoveMember(event, member.username || '')}
                  >
                    <p class="mb-3">
                      Removing this member immediately revokes their
                      organization access and delegated team access.
                    </p>
                    <label class="mb-3 flex items-start gap-2">
                      <input
                        id={`member-remove-confirm-${member.username}`}
                        bind:checked={memberRemoveConfirmed}
                        type="checkbox"
                        name="confirm_remove"
                        disabled={removingMemberUsername === member.username}
                      />
                      <span>
                        I understand this member will immediately lose access to
                        the organization.
                      </span>
                    </label>
                    <div class="token-row__actions">
                      <button
                        class="btn btn-danger btn-sm"
                        id={`member-remove-submit-${member.username}`}
                        type="submit"
                        disabled={removingMemberUsername === member.username}
                      >
                        {removingMemberUsername === member.username
                          ? 'Removing…'
                          : 'Remove member'}
                      </button>
                      <button
                        class="btn btn-secondary btn-sm"
                        type="button"
                        on:click={cancelMemberRemoveConfirmation}
                        disabled={removingMemberUsername === member.username}
                      >
                        Keep member
                      </button>
                    </div>
                  </form>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </section>

      <div class="settings-grid">
        <section class="card settings-section">
          <h2>Teams</h2>
          {#if canManageTeams}
            <form class="settings-subsection" on:submit={handleCreateTeam}>
              <div class="grid gap-4 xl:grid-cols-2">
                <div class="form-group">
                  <label for="team-create-name">Team name</label>
                  <input
                    id="team-create-name"
                    name="name"
                    class="form-input"
                    placeholder="Release engineering"
                    required
                  />
                </div>
                <div class="form-group">
                  <label for="team-create-slug">Team slug</label>
                  <input
                    id="team-create-slug"
                    name="team_slug"
                    class="form-input"
                    placeholder="release-engineering"
                    required
                  />
                </div>
              </div>
              <div class="form-group">
                <label for="team-create-description">Description</label>
                <textarea
                  id="team-create-description"
                  name="description"
                  class="form-input"
                  rows="3"
                ></textarea>
              </div>
              <button type="submit" class="btn btn-primary">Create team</button>
            </form>
          {/if}

          {#if teamsError}
            <div class="alert alert-error">{teamsError}</div>
          {:else if teams.length === 0}
            <div class="empty-state">
              <h3>No teams yet</h3>
              <p>
                Create the first team to delegate package work more clearly.
              </p>
            </div>
          {:else}
            <div class="settings-section">
              {#each teams as team}
                {@const teamSlug = team.slug || ''}
                {@const teamMembers =
                  teamMembersBySlug[teamSlug]?.members || []}
                {@const teamMembersError =
                  teamMembersBySlug[teamSlug]?.load_error || null}
                {@const teamRepositoryGrants =
                  teamRepositoryAccessBySlug[teamSlug]?.grants || []}
                {@const teamRepositoryGrantsError =
                  teamRepositoryAccessBySlug[teamSlug]?.load_error || null}
                {@const teamNamespaceGrants =
                  teamNamespaceAccessBySlug[teamSlug]?.grants || []}
                {@const teamNamespaceGrantsError =
                  teamNamespaceAccessBySlug[teamSlug]?.load_error || null}
                {@const teamGrants =
                  teamPackageAccessBySlug[teamSlug]?.grants || []}
                {@const teamGrantsError =
                  teamPackageAccessBySlug[teamSlug]?.load_error || null}
                {@const eligibleTeamMemberOptions =
                  getEligibleTeamMemberOptions(teamSlug)}
                <div class="settings-subsection" id={teamSlug ? `team-${teamSlug}` : undefined}>
                  <div class="org-section-header">
                    <div>
                      <h3>{team.name || team.slug || 'Team'}</h3>
                      <p class="settings-copy">
                        {team.description || 'No team description yet.'}
                      </p>
                      <div class="token-row__meta">
                        <span>@{team.slug || 'no-slug'}</span>
                        <span>created {formatDate(team.created_at)}</span>
                      </div>
                    </div>
                    {#if canManageTeams && teamSlug}
                      <div class="token-row__actions">
                        <a
                          class="btn btn-secondary btn-sm"
                          href={`/orgs/${encodeURIComponent(slug)}/teams/${encodeURIComponent(teamSlug)}`}
                          data-sveltekit-preload-data="hover">Open workspace</a
                        >
                        {#if teamDeleteTargetSlug === teamSlug}
                          <button
                            class="btn btn-secondary btn-sm"
                            type="button"
                            on:click={cancelTeamDeleteConfirmation}
                            disabled={deletingTeamSlug === teamSlug}>Cancel</button
                          >
                        {:else}
                          <button
                            class="btn btn-danger btn-sm"
                            id={`team-delete-toggle-${teamSlug}`}
                            type="button"
                            aria-label={`Delete team ${teamSlug}`}
                            on:click={() => openTeamDeleteConfirmation(teamSlug)}
                            >Delete...</button
                          >
                        {/if}
                      </div>
                    {/if}
                  </div>

                  {#if canManageTeams && teamSlug && teamDeleteTargetSlug === teamSlug}
                    <form
                      class="alert alert-warning mt-4"
                      id={`team-delete-form-${teamSlug}`}
                      on:submit={(event) => handleDeleteTeam(event, teamSlug)}
                    >
                      <p class="mb-3">
                        Deleting this team immediately removes its memberships and delegated
                        package, repository, and namespace access.
                      </p>
                      <label class="mb-3 flex items-start gap-2">
                        <input
                          id={`team-delete-confirm-${teamSlug}`}
                          bind:checked={teamDeleteConfirmed}
                          type="checkbox"
                          name="confirm_delete"
                          disabled={deletingTeamSlug === teamSlug}
                        />
                        <span>
                          I understand deleting this team revokes its delegated access and cannot be
                          undone.
                        </span>
                      </label>
                      <div class="token-row__actions">
                        <button
                          class="btn btn-danger btn-sm"
                          id={`team-delete-submit-${teamSlug}`}
                          type="submit"
                          disabled={deletingTeamSlug === teamSlug}
                        >
                          {deletingTeamSlug === teamSlug ? 'Deleting…' : 'Delete team'}
                        </button>
                        <button
                          class="btn btn-secondary btn-sm"
                          type="button"
                          on:click={cancelTeamDeleteConfirmation}
                          disabled={deletingTeamSlug === teamSlug}
                        >
                          Keep team
                        </button>
                      </div>
                    </form>
                  {/if}

                  {#if canManageTeams && teamSlug}
                    <div class="grid gap-6 xl:grid-cols-2">
                      <TeamSettingsEditor
                        {team}
                        {teamSlug}
                        nameFieldId={`team-name-${teamSlug}`}
                        descriptionFieldId={`team-description-${teamSlug}`}
                        submitLabel="Save changes"
                        submitClass="btn btn-secondary"
                        handleSubmit={(event) => teamManagement.updateTeam(teamSlug, event)}
                      />

                      <div>
                        <h4>Team members</h4>
                        <TeamMembersEditor
                          members={teamMembers}
                          membersError={teamMembersError}
                          eligibleOptions={eligibleTeamMemberOptions}
                          inputId={`team-member-${teamSlug}`}
                          datalistId={`team-member-options-${teamSlug}`}
                          emptyMembersMessage="No members in this team yet."
                          eligibleOptionsError={null}
                          handleSubmit={(event) =>
                            teamManagement.addTeamMember(teamSlug, event)}
                          handleRemoveMember={(username) =>
                            teamManagement.removeTeamMember(teamSlug, username)}
                        />
                      </div>
                    </div>

                    {#if canManageRepositories}
                      <div class="mt-6">
                        <h4>Repository access</h4>
                        <p class="settings-copy">
                          Repository grants apply across current and future
                        packages in the selected repository. The <strong
                          >Admin</strong
                        >
                        permission also unlocks repository setting updates.
                      </p>
                      <TeamRepositoryAccessEditor
                        grants={teamRepositoryGrants}
                        grantsError={teamRepositoryGrantsError}
                        optionsError={repositoriesError}
                        options={repositoryGrantOptions}
                        permissionOptions={TEAM_PERMISSION_OPTIONS}
                        fieldId={`team-repository-${teamSlug}`}
                        emptyGrantsMessage="No repository grants assigned yet."
                        handleSubmit={(event) =>
                          teamManagement.replaceTeamRepositoryAccess(teamSlug, event)}
                        handleRevoke={(repositorySlug) =>
                          teamManagement.removeTeamRepositoryAccess(
                            teamSlug,
                            repositorySlug
                          )}
                      />
                      </div>
                    {/if}

                    <div class="mt-6">
                      <h4>Package access</h4>
                      <TeamPackageAccessEditor
                        grants={teamGrants}
                        grantsError={teamGrantsError}
                        optionsError={packagesError}
                        options={packageGrantOptions}
                        permissionOptions={TEAM_PERMISSION_OPTIONS}
                        fieldId={`team-package-${teamSlug}`}
                        emptyGrantsMessage="No package grants assigned yet."
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

                    {#if canManageNamespaces}
                      <div class="mt-6">
                      <h4>Namespace access</h4>
                      <p class="settings-copy">
                        Namespace grants let a team delete or transfer specific
                        organization-owned namespace claims without broader
                        organization roles.
                      </p>
                      <TeamNamespaceAccessEditor
                        grants={teamNamespaceGrants}
                        grantsError={teamNamespaceGrantsError}
                        optionsError={namespaceError}
                        options={namespaceGrantOptions}
                        permissionOptions={TEAM_NAMESPACE_PERMISSION_OPTIONS}
                        fieldId={`team-namespace-${teamSlug}`}
                        emptyGrantsMessage="No namespace grants assigned yet."
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
                    {/if}
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </section>

        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Security overview</h2>
              <p class="settings-copy">
                Filter unresolved findings across the packages currently visible
                to you and export the current rollup as CSV.
              </p>
            </div>
          </div>
          <OrgSecurityFilterControls
            formClass="settings-subsection"
            severityOptions={SECURITY_FILTER_SEVERITY_OPTIONS}
            selectedSeverities={securityView.severities}
            ecosystemOptions={SECURITY_FILTER_ECOSYSTEM_OPTIONS}
            ecosystemValue={securityView.ecosystem}
            packageValue={securityView.packageQuery}
            packageOptions={securityPackageOptions}
            exporting={exportingSecurity}
            summary={formatSecurityFilterSummary()}
            showSeverityClear={securityView.severities.length > 0}
            showEcosystemClear={Boolean(securityView.ecosystem)}
            showPackageClear={Boolean(securityView.packageQuery)}
            handleSubmit={handleSecurityFilterSubmit}
            handleExport={handleExportSecurity}
            clearSeverity={clearSecuritySeverityFilter}
            clearEcosystem={clearSecurityEcosystemFilter}
            clearPackage={clearSecurityPackageFilter}
          />
          {#if securityError}
            <div class="alert alert-error">{securityError}</div>
          {:else if openFindingCount === 0 || securityPackages.length === 0}
            <div class="empty-state">
              <h3>
                {hasSecurityFilters
                  ? 'No matching open security findings'
                  : 'No open security findings'}
              </h3>
              <p>
                {#if hasSecurityFilters}
                  Try adjusting or clearing the current filters.
                {:else}
                  The packages currently visible to you do not have any
                  unresolved findings.
                {/if}
              </p>
            </div>
          {:else}
            <div class="org-kpi-grid">
              <div class="org-kpi">
                <span class="org-kpi__value"
                  >{formatNumber(openFindingCount)}</span
                ><span class="org-kpi__label">Open findings</span>
              </div>
              <div class="org-kpi">
                <span class="org-kpi__value"
                  >{formatNumber(affectedPackageCount)}</span
                ><span class="org-kpi__label">Affected packages</span>
              </div>
              <div class="org-kpi">
                <span class="org-kpi__value"
                  >{formatNumber(severityCounts.critical)}</span
                ><span class="org-kpi__label">Critical</span>
              </div>
              <div class="org-kpi">
                <span class="org-kpi__value"
                  >{formatNumber(severityCounts.high)}</span
                ><span class="org-kpi__label">High</span>
              </div>
            </div>

            <div
              class="token-row__scopes"
              style="margin-top:1rem; margin-bottom:1rem;"
            >
              {#each SECURITY_SEVERITIES.filter((severity) => severityCounts[severity] > 0) as severity}
                <span class={`badge badge-severity-${severity}`}
                  >{formatNumber(severityCounts[severity])} {severity}</span
                >
              {/each}
            </div>

            <div class="token-list">
              {#each sortedSecurityPackages as pkg}
                {@const pkgCounts = normalizeSecuritySeverityCounts(
                  pkg.severities
                )}
                {@const pkgOpenFindings =
                  typeof pkg.open_findings === 'number' &&
                  Number.isFinite(pkg.open_findings)
                    ? Math.max(0, Math.trunc(pkg.open_findings))
                    : totalSecuritySeverityCounts(pkg.severities)}
                {@const pkgWorstSeverity = pkg.worst_severity
                  ? normalizeSecuritySeverity(pkg.worst_severity)
                  : worstSecuritySeverityFromCounts(pkg.severities)}
                {@const reviewerTeams = pkg.reviewer_teams || []}
                {@const packageKey = getSecurityPackageKey(pkg)}
                {@const packageFindingState = getOrgSecurityFindingState(pkg)}
                {@const packageSecurityPath = buildPackageSecurityPath(
                  pkg.ecosystem || 'unknown',
                  pkg.name || '',
                  {
                    severities: securityView.severities,
                  }
                )}
                {@const packageDetailsPath = buildPackageDetailsPath(
                  pkg.ecosystem || 'unknown',
                  pkg.name || ''
                )}
                <div class="token-row">
                  <div class="token-row__main">
                    <div class="token-row__title">
                      <a
                        href={packageSecurityPath}
                        data-sveltekit-preload-data="hover"
                        >{pkg.name || 'Unnamed package'}</a
                      >
                    </div>
                    <div class="token-row__meta">
                      <span>{ecosystemLabel(pkg.ecosystem)}</span>
                      <span
                        >{formatIdentifierLabel(
                          pkg.visibility || 'public'
                        )}</span
                      >
                      <span>{formatOpenFindingLabel(pkgOpenFindings)}</span>
                      {#if pkg.latest_detected_at}<span
                          >latest {formatDate(pkg.latest_detected_at)}</span
                        >{/if}
                    </div>
                    <div class="token-row__scopes">
                      <span class={`badge badge-severity-${pkgWorstSeverity}`}
                        >{formatIdentifierLabel(pkgWorstSeverity)} highest</span
                      >
                      {#each SECURITY_SEVERITIES.filter((severity) => pkgCounts[severity] > 0) as severity}
                        <span class={`badge badge-severity-${severity}`}
                          >{formatNumber(pkgCounts[severity])} {severity}</span
                        >
                      {/each}
                      {#if pkg.can_manage_security}
                        <span class="badge badge-verified"
                          >You can triage findings</span
                        >
                      {/if}
                    </div>
                    {#if reviewerTeams.length > 0}
                      <div
                        class="token-row__meta"
                        style="margin-top:0.5rem; flex-wrap:wrap;"
                      >
                        <span>Review teams</span>
                          {#each reviewerTeams as team}
                          <span class="badge badge-ecosystem"
                            >{team.name || team.slug || REVIEW_TEAM_FALLBACK_LABEL}</span
                          >
                        {/each}
                      </div>
                    {/if}
                  </div>
                  {#if pkg.ecosystem && pkg.name}
                    <div class="token-row__actions">
                      {#if pkg.can_manage_security}
                        <button
                          type="button"
                          class="btn btn-secondary btn-sm"
                          on:click={() => handleToggleOrgSecurityFindings(pkg)}
                        >
                          {packageFindingState.expanded
                            ? 'Hide findings'
                            : 'Show findings'}
                        </button>
                      {/if}
                        <a
                          class="btn btn-secondary btn-sm"
                          href={buildPackageSecurityPath(pkg.ecosystem, pkg.name, {
                            severities: securityView.severities,
                          })}
                          data-sveltekit-preload-data="hover"
                          >{pkg.can_manage_security ? 'Review findings' : 'Open findings'}</a
                        >
                        <a
                          class="btn btn-secondary btn-sm"
                          href={packageDetailsPath}
                          data-sveltekit-preload-data="hover"
                          >Open package details</a
                        >
                    </div>
                  {/if}
                  {#if pkg.can_manage_security && packageFindingState.expanded}
                    <div
                      class="card"
                      style="margin-top:1rem; width:100%;"
                    >
                      <div class="settings-subsection" style="margin-bottom:0;">
                        <h3 style="margin-bottom:0.5rem;">Inline findings</h3>
                        <p class="settings-copy" style="margin-top:0;">
                          Resolve or reopen findings here without leaving the
                          organization workspace.
                        </p>
                        {#if packageFindingState.notice}
                          <div class="alert alert-success">
                            {packageFindingState.notice}
                          </div>
                        {/if}
                        {#if packageFindingState.error}
                          <div class="alert alert-error">
                            {packageFindingState.error}
                          </div>
                        {/if}
                        {#if packageFindingState.load_error}
                          <div class="alert alert-error">
                            {packageFindingState.load_error}
                          </div>
                        {:else if packageFindingState.loading}
                          <div class="loading">
                            <span class="spinner"></span> Loading findings…
                          </div>
                        {:else if packageFindingState.findings.length === 0}
                          <div class="empty-state">
                            <p>No findings available for inline triage.</p>
                          </div>
                        {:else}
                          <OrgSecurityFindingTriageControls
                            findings={packageFindingState.findings}
                            findingNotes={packageFindingState.findingNotes}
                            updatingFindingId={packageFindingState.updatingFindingId}
                            notePlaceholder={SECURITY_FINDING_NOTE_PLACEHOLDER}
                            formatDateValue={formatDate}
                            normalizeSeverity={normalizeSecuritySeverity}
                            formatKindLabel={formatIdentifierLabel}
                            buildPackageSecurityHref={(finding) =>
                              buildPackageSecurityFindingPath(
                                pkg.ecosystem || 'unknown',
                                pkg.name || '',
                                finding
                              )}
                            handleNoteInput={(findingId, value) =>
                              updateOrgSecurityFindingNote(
                                packageKey,
                                findingId,
                                value
                              )}
                            handleToggleResolution={(finding) =>
                              handleToggleOrgFindingResolution(pkg, finding)}
                          />
                        {/if}
                      </div>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        </section>
      </div>
    {:else}
      <section class="card settings-section">
        <h2>People and teams</h2>
        <p class="settings-copy">
          Organization membership and team structure are only visible to current
          organization members.
        </p>
      </section>
    {/if}

    <div class="settings-grid">
      <section class="card settings-section">
        <h2>Repositories</h2>
        {#if repositoriesError}
          <div class="alert alert-error">{repositoriesError}</div>
        {:else if repositories.length === 0}
          <div class="empty-state">
            <h3>No repositories yet</h3>
            <p>This organization has not exposed any repositories yet.</p>
          </div>
        {:else}
          <div class="settings-section">
            {#each [...repositories].sort( (left, right) => `${left.name || left.slug || ''}`.localeCompare(`${right.name || right.slug || ''}`) ) as repository}
              {@const repositorySlug = repository.slug || ''}
              {@const repositoryPackages =
                repositoryPackagesBySlug[repositorySlug]?.packages || []}
              {@const repositoryPackagesError =
                repositoryPackagesBySlug[repositorySlug]?.load_error || null}
              <div class="settings-subsection">
                <div class="org-section-header">
                  <div>
                    <h3>
                      {#if repositorySlug}
                        <a
                          href={`/repositories/${encodeURIComponent(repositorySlug)}`}
                          data-sveltekit-preload-data="hover"
                          >{repository.name || repositorySlug}</a
                        >
                      {:else}
                        {repository.name || 'Repository'}
                      {/if}
                    </h3>
                    <div class="token-row__meta">
                      <span>@{repositorySlug || 'no-slug'}</span>
                      <span>{formatRepositoryKindLabel(repository.kind)}</span>
                      <span
                        >{formatRepositoryVisibilityLabel(
                          repository.visibility
                        )}</span
                      >
                      <span
                        >{formatNumber(repository.package_count)} packages</span
                      >
                    </div>
                    {#if repository.description}<p class="settings-copy">
                        {repository.description}
                      </p>{/if}
                    {#if repository.upstream_url}<p class="settings-copy">
                        <a
                          href={repository.upstream_url}
                          target="_blank"
                          rel="noopener noreferrer">{repository.upstream_url}</a
                        >
                      </p>{/if}
                  </div>
                </div>

                <div>
                  <h4>Visible packages</h4>
                  <p class="settings-copy">
                    {formatRepositoryPackageCoverageLabel(
                      repositoryPackages.length,
                      repository.package_count
                    )}
                  </p>
                  {#if repositoryPackagesError}
                    <div class="alert alert-error">
                      {repositoryPackagesError}
                    </div>
                  {:else if repositoryPackages.length === 0}
                    <p class="settings-copy">
                      No visible packages belong to this repository yet.
                    </p>
                  {:else}
                    <div class="token-list">
                      {#each repositoryPackages as pkg}
                        {@const packageSecurityPath = buildPackageSecurityPath(
                          pkg.ecosystem || 'unknown',
                          pkg.name || '',
                          {
                            severities: securityView.severities,
                          }
                        )}
                        {@const packageDetailsPath = buildPackageDetailsPath(
                          pkg.ecosystem || 'unknown',
                          pkg.name || ''
                        )}
                        <div class="token-row">
                          <div class="token-row__main">
                            <div class="token-row__title">
                              <a
                                href={packageSecurityPath}
                                data-sveltekit-preload-data="hover"
                                >{pkg.name || 'Unnamed package'}</a
                              >
                            </div>
                            <div class="token-row__meta">
                              <span>{pkg.ecosystem || 'unknown'}</span>
                              <span
                                >{formatRepositoryVisibilityLabel(
                                  pkg.visibility
                                )}</span
                              >
                              <span
                                >{formatNumber(pkg.download_count)} downloads</span
                              >
                            </div>
                          </div>
                          <div class="token-row__actions">
                            <a
                              href={packageDetailsPath}
                              class="btn btn-secondary btn-sm"
                              data-sveltekit-preload-data="hover"
                              >Open package details</a
                            >
                          </div>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>

                {#if canManageRepositories && repositorySlug}
                  <form
                    class="mt-4"
                    on:submit={(event) =>
                      handleUpdateRepository(event, repositorySlug)}
                  >
                    <div class="grid gap-4 xl:grid-cols-2">
                      <div class="form-group">
                        <label for={`repository-kind-${repositorySlug}`}
                          >Repository kind</label
                        >
                        <input
                          id={`repository-kind-${repositorySlug}`}
                          class="form-input"
                          value={formatRepositoryKindLabel(repository.kind)}
                          disabled
                        />
                      </div>
                      <div class="form-group">
                        <label for={`repository-visibility-${repositorySlug}`}
                          >Visibility</label
                        >
                        <select
                          id={`repository-visibility-${repositorySlug}`}
                          name="visibility"
                          class="form-input"
                        >
                          {#each REPOSITORY_VISIBILITY_OPTIONS as option}
                            <option
                              value={option.value}
                              selected={option.value ===
                                (repository.visibility || 'public')}
                              >{option.label}</option
                            >
                          {/each}
                        </select>
                      </div>
                    </div>
                    <div class="form-group">
                      <label for={`repository-upstream-${repositorySlug}`}
                        >Upstream URL</label
                      >
                      <input
                        id={`repository-upstream-${repositorySlug}`}
                        name="upstream_url"
                        class="form-input"
                        type="url"
                        value={repository.upstream_url || ''}
                        placeholder="https://registry.npmjs.org"
                      />
                    </div>
                    <div class="form-group">
                      <label for={`repository-description-${repositorySlug}`}
                        >Description</label
                      >
                      <textarea
                        id={`repository-description-${repositorySlug}`}
                        name="description"
                        class="form-input"
                        rows="3">{repository.description || ''}</textarea
                      >
                    </div>
                    <button type="submit" class="btn btn-secondary"
                      >Save repository</button
                    >
                  </form>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if canManageRepositories}
          <div class="settings-subsection">
            <h3>Transfer repository ownership</h3>
            {#if repositoriesError}
              <p class="settings-copy">
                Repositories must load successfully before you can transfer one.
              </p>
            {:else if transferableRepositories.length === 0}
              <p class="settings-copy">
                No visible repositories are currently transferable with this
                credential.
              </p>
            {:else if repositoryTransferTargets.length === 0}
              <p class="settings-copy">
                You do not administer another organization that can receive one
                of these repositories.
              </p>
            {:else}
              <div class="alert alert-warning" style="margin-bottom:12px;">
                This transfer is immediate and revokes existing team grants on
                the repository.
              </div>
              <form on:submit={handleRepositoryTransfer}>
                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="org-repository-transfer-repository"
                      >Organization repository</label
                    >
                    <select
                      id="org-repository-transfer-repository"
                      name="repository_slug"
                      class="form-input"
                      required
                    >
                      <option value="">Select a repository</option>
                      {#each transferableRepositories as repository}
                        <option value={repository.slug || ''}
                          >{`${repository.name || repository.slug || ''} · ${formatRepositoryKindLabel(repository.kind)} · ${formatRepositoryVisibilityLabel(repository.visibility)}`}</option
                        >
                      {/each}
                    </select>
                  </div>
                  <div class="form-group">
                    <label for="org-repository-transfer-target"
                      >Target organization</label
                    >
                    <select
                      id="org-repository-transfer-target"
                      name="target_org_slug"
                      class="form-input"
                      required
                    >
                      <option value="">Select an organization</option>
                      {#each repositoryTransferTargets as target}
                        <option value={target.slug || ''}
                          >{target.name ||
                            target.slug ||
                            'Unnamed organization'}</option
                        >
                      {/each}
                    </select>
                  </div>
                </div>
                {#if repositoryTransferConfirmationOpen}
                  <div
                    class="alert alert-warning"
                    id="org-repository-transfer-confirmation"
                    style="margin-bottom:12px;"
                  >
                    <label class="flex items-start gap-2">
                      <input
                        id="org-repository-transfer-confirm"
                        bind:checked={repositoryTransferConfirmed}
                        type="checkbox"
                        name="confirm"
                        disabled={transferringRepositoryOwnership}
                      />
                      <span
                        >I understand this repository transfer is immediate and
                        existing team grants will be removed.</span
                      >
                    </label>
                    <div class="token-row__actions" style="margin-top:12px;">
                      <button
                        id="org-repository-transfer-submit"
                        type="submit"
                        class="btn btn-danger"
                        disabled={transferringRepositoryOwnership}
                      >
                        {transferringRepositoryOwnership
                          ? 'Transferring...'
                          : 'Transfer repository'}
                      </button>
                      <button
                        type="button"
                        class="btn btn-secondary"
                        on:click={cancelRepositoryTransferConfirmation}
                        disabled={transferringRepositoryOwnership}
                      >
                        Keep repository
                      </button>
                    </div>
                  </div>
                {:else}
                  <button
                    id="org-repository-transfer-toggle"
                    type="button"
                    class="btn btn-danger"
                    aria-label="Review repository ownership transfer"
                    on:click={openRepositoryTransferConfirmation}
                  >
                    Transfer repository...
                  </button>
                {/if}
              </form>
            {/if}
          </div>
        {/if}

        {#if canAdminister}
          <div class="settings-subsection">
            <h3>Create a package</h3>
            <p class="settings-copy">
              Package ownership is derived from the selected repository.
              Visibility cannot be broader than the repository visibility, and
              matching namespace claims currently constrain npm/Bun scopes,
              Composer vendors, and Maven group IDs.
            </p>

            {#if repositoriesError}
              <p class="settings-copy">
                Repositories must load successfully before you can create a
                package.
              </p>
            {:else if repositories.length === 0}
              <p class="settings-copy">
                Create an organization-owned repository before creating the
                first package.
              </p>
            {:else if creatableRepositories.length === 0}
              <p class="settings-copy">
                Only public, private, staging, and release repositories can host
                directly created packages. The current repository set is limited
                to proxy or virtual repositories.
              </p>
            {:else}
              <form class="mt-4" on:submit={handleCreatePackage}>
                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="package-create-repository">Repository</label>
                    <select
                      id="package-create-repository"
                      name="repository_slug"
                      class="form-input"
                      bind:value={newPackageRepositorySlug}
                      required
                    >
                      {#each creatableRepositories as repository}
                        <option value={repository.slug}>
                          {formatPackageCreationRepositoryLabel(repository)}
                        </option>
                      {/each}
                    </select>
                  </div>

                  <div class="form-group">
                    <label for="package-create-ecosystem">Ecosystem</label>
                    <select
                      id="package-create-ecosystem"
                      name="ecosystem"
                      class="form-input"
                      bind:value={newPackageEcosystem}
                    >
                      {#each ECOSYSTEMS as ecosystem}
                        <option value={ecosystem.id}>{ecosystem.label}</option>
                      {/each}
                    </select>
                  </div>
                </div>

                <div class="form-group">
                  <label for="package-create-name">Package name</label>
                  <input
                    id="package-create-name"
                    name="name"
                    class="form-input"
                    bind:value={newPackageName}
                    placeholder="acme-widget, @acme/widget, acme/widget, com.acme:artifact"
                    required
                  />
                </div>

                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="package-create-display-name">Display name</label
                    >
                    <input
                      id="package-create-display-name"
                      name="display_name"
                      class="form-input"
                      bind:value={newPackageDisplayName}
                      placeholder="Optional friendly title"
                    />
                  </div>

                  <div class="form-group">
                    <label for="package-create-visibility">Visibility</label>
                    <select
                      id="package-create-visibility"
                      name="visibility"
                      class="form-input"
                      bind:value={newPackageVisibility}
                    >
                      <option value="">
                        {selectedPackageCreationRepository
                          ? `Use repository default (${formatRepositoryVisibilityLabel(selectedPackageCreationRepository.visibility)})`
                          : 'Use repository default'}
                      </option>
                      {#each explicitPackageVisibilityOptions as option}
                        <option value={option.value}>{option.label}</option>
                      {/each}
                    </select>
                  </div>
                </div>

                <div class="form-group">
                  <label for="package-create-description">Description</label>
                  <textarea
                    id="package-create-description"
                    name="description"
                    class="form-input"
                    rows="3"
                    bind:value={newPackageDescription}
                    placeholder="Optional package summary"
                  ></textarea>
                </div>

                {#if selectedPackageCreationRepository}
                  <p class="settings-copy" style="margin-bottom:12px;">
                    The new package will inherit ownership from
                    <strong
                      >{selectedPackageCreationRepository.name ||
                        selectedPackageCreationRepository.slug}</strong
                    >
                    and stay within
                    <strong
                      >{formatRepositoryVisibilityLabel(
                        selectedPackageCreationRepository.visibility
                      )}</strong
                    > visibility rules.
                  </p>
                {/if}

                {#if repositoryDefaultPackageVisibility === 'quarantined'}
                  <div class="alert alert-warning" style="margin-bottom:12px;">
                    Quarantined repositories can only create quarantined
                    packages.
                  </div>
                {/if}

                <button
                  type="submit"
                  class="btn btn-primary"
                  disabled={creatingPackage}
                >
                  {creatingPackage ? 'Creating…' : 'Create package'}
                </button>
              </form>
            {/if}
          </div>
        {/if}

        {#if canManageRepositories && org.id}
          <form class="settings-subsection" on:submit={handleCreateRepository}>
            <h3>Create a repository</h3>
            <div class="grid gap-4 xl:grid-cols-2">
              <div class="form-group">
                <label for="repository-create-name">Repository name</label>
                <input
                  id="repository-create-name"
                  name="name"
                  class="form-input"
                  placeholder="Acme Public"
                  required
                />
              </div>
              <div class="form-group">
                <label for="repository-create-slug">Repository slug</label>
                <input
                  id="repository-create-slug"
                  name="slug"
                  class="form-input"
                  placeholder="acme-public"
                  required
                />
              </div>
            </div>
            <div class="grid gap-4 xl:grid-cols-2">
              <div class="form-group">
                <label for="repository-create-kind">Repository kind</label>
                <select
                  id="repository-create-kind"
                  name="kind"
                  class="form-input"
                >
                  {#each REPOSITORY_KIND_OPTIONS as option}
                    <option value={option.value}>{option.label}</option>
                  {/each}
                </select>
              </div>
              <div class="form-group">
                <label for="repository-create-visibility">Visibility</label>
                <select
                  id="repository-create-visibility"
                  name="visibility"
                  class="form-input"
                >
                  {#each REPOSITORY_VISIBILITY_OPTIONS as option}
                    <option value={option.value}>{option.label}</option>
                  {/each}
                </select>
              </div>
            </div>
            <div class="form-group">
              <label for="repository-create-upstream">Upstream URL</label>
              <input
                id="repository-create-upstream"
                name="upstream_url"
                class="form-input"
                type="url"
                placeholder="https://registry.npmjs.org"
              />
            </div>
            <div class="form-group">
              <label for="repository-create-description">Description</label>
              <textarea
                id="repository-create-description"
                name="description"
                class="form-input"
                rows="3"
              ></textarea>
            </div>
            <button type="submit" class="btn btn-primary"
              >Create repository</button
            >
          </form>
        {/if}
      </section>

      <section class="card settings-section">
        <h2>Namespace claims</h2>
        {#if namespaceError}
          <div class="alert alert-error">{namespaceError}</div>
        {:else if namespaceClaims.length === 0}
          <div class="empty-state">
            <h3>No namespace claims yet</h3>
            <p>
              This organization has not claimed any ecosystem namespaces yet.
            </p>
          </div>
        {:else}
          <div class="token-list">
            {#each [...namespaceClaims].sort( (left, right) => `${left.ecosystem || ''}:${left.namespace || ''}`.localeCompare(`${right.ecosystem || ''}:${right.namespace || ''}`) ) as claim}
              <div>
                <div class="token-row">
                  <div class="token-row__main">
                    <div class="token-row__title">
                      {claim.namespace || 'Unnamed claim'}
                    </div>
                    <div class="token-row__meta">
                      <span>{ecosystemLabel(claim.ecosystem)}</span>
                      {#if claim.created_at}<span
                          >created {formatDate(claim.created_at)}</span
                        >{/if}
                    </div>
                  </div>
                  <div class="token-row__actions">
                    {#if claim.is_verified}
                      <span class="badge badge-verified">Verified</span>
                    {:else}
                      <span class="badge badge-ecosystem"
                        >Pending verification</span
                      >
                    {/if}
                    {#if claim.can_manage && claim.id}
                      {#if namespaceDeleteTargetId === claim.id}
                        <button
                          class="btn btn-secondary btn-sm"
                          type="button"
                          on:click={cancelNamespaceDeleteConfirmation}
                          disabled={deletingNamespaceClaimId === claim.id}
                          >Cancel</button
                        >
                      {:else}
                        <button
                          class="btn btn-secondary btn-sm"
                          id={`namespace-delete-toggle-${claim.id}`}
                          type="button"
                          aria-label={`Delete namespace claim ${claim.namespace || 'Unnamed claim'}`}
                          on:click={() => openNamespaceDeleteConfirmation(claim.id || '')}
                          >Delete...</button
                        >
                      {/if}
                    {/if}
                  </div>
                </div>

                {#if claim.can_manage && claim.id && namespaceDeleteTargetId === claim.id}
                  <form
                    class="alert alert-warning mt-4"
                    id={`namespace-delete-form-${claim.id}`}
                    on:submit={(event) =>
                      handleDeleteNamespace(
                        event,
                        claim.id,
                        claim.namespace || 'this claim'
                      )}
                  >
                    <p class="mb-3">
                      Deleting this namespace claim immediately removes the organization's claim to
                      this ecosystem namespace.
                    </p>
                    <label class="mb-3 flex items-start gap-2">
                      <input
                        id={`namespace-delete-confirm-${claim.id}`}
                        bind:checked={namespaceDeleteConfirmed}
                        type="checkbox"
                        name="confirm_delete"
                        disabled={deletingNamespaceClaimId === claim.id}
                      />
                      <span>
                        I understand deleting this namespace claim is immediate and cannot be
                        undone.
                      </span>
                    </label>
                    <div class="token-row__actions">
                      <button
                        class="btn btn-danger btn-sm"
                        id={`namespace-delete-submit-${claim.id}`}
                        type="submit"
                        disabled={deletingNamespaceClaimId === claim.id}
                      >
                        {deletingNamespaceClaimId === claim.id
                          ? 'Deleting…'
                          : 'Delete namespace claim'}
                      </button>
                      <button
                        class="btn btn-secondary btn-sm"
                        type="button"
                        on:click={cancelNamespaceDeleteConfirmation}
                        disabled={deletingNamespaceClaimId === claim.id}
                      >
                        Keep claim
                      </button>
                    </div>
                  </form>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if canManageNamespaces && org.id}
          <form class="settings-subsection" on:submit={handleCreateNamespace}>
            <h3>Claim a namespace</h3>
            <div class="grid gap-4 xl:grid-cols-2">
              <div class="form-group">
                <label for="namespace-ecosystem">Ecosystem</label>
                <select
                  id="namespace-ecosystem"
                  name="ecosystem"
                  class="form-input"
                >
                  {#each ECOSYSTEMS as ecosystem}
                    <option
                      value={ecosystem.id}
                      selected={ecosystem.id === DEFAULT_NAMESPACE_ECOSYSTEM}
                      >{ecosystem.label}</option
                    >
                  {/each}
                </select>
              </div>
              <div class="form-group">
                <label for="namespace-value">Namespace</label>
                <input
                  id="namespace-value"
                  name="namespace"
                  class="form-input"
                  placeholder="@acme, acme, com.acme"
                  required
                />
              </div>
            </div>
            <button type="submit" class="btn btn-primary"
              >Create namespace claim</button
            >
          </form>
        {/if}

        {#if org.id}
          <div class="settings-subsection">
            <h3>Transfer a namespace</h3>
            <p class="settings-copy">
              Move an organization-owned namespace claim into another
              organization you already administer.
            </p>
            {#if transferableNamespaceClaims.length === 0}
              <p class="settings-copy">
                No namespace claims are currently transferable with this
                credential.
              </p>
            {:else if namespaceTransferTargets.length === 0}
              <p class="settings-copy">
                You do not administer another organization that can receive one
                of these namespace claims.
              </p>
            {:else}
              <div class="alert alert-warning" style="margin-bottom:12px;">
                This transfer is immediate and keeps the claim's verification
                state unchanged.
              </div>
              <form class="settings-subsection" on:submit={handleNamespaceTransfer}>
                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="org-namespace-transfer-claim"
                      >Organization namespace claim</label
                    >
                    <select
                      id="org-namespace-transfer-claim"
                      name="claim_id"
                      class="form-input"
                      required
                    >
                      <option value="">Select a namespace claim</option>
                      {#each transferableNamespaceClaims as claim}
                        <option value={claim.id || ''}
                          >{`${claim.namespace || 'Unnamed claim'} · ${ecosystemLabel(claim.ecosystem)}`}</option
                        >
                      {/each}
                    </select>
                  </div>
                  <div class="form-group">
                    <label for="org-namespace-transfer-target"
                      >Target organization</label
                    >
                    <select
                      id="org-namespace-transfer-target"
                      name="target_org_slug"
                      class="form-input"
                      required
                    >
                      <option value="">Select an organization</option>
                      {#each namespaceTransferTargets as target}
                        <option value={target.slug || ''}
                          >{target.name ||
                            target.slug ||
                            'Unnamed organization'}</option
                        >
                      {/each}
                    </select>
                  </div>
                </div>
                {#if namespaceTransferConfirmationOpen}
                  <div
                    class="alert alert-warning"
                    id="org-namespace-transfer-confirmation"
                    style="margin-bottom:12px;"
                  >
                    <label class="flex items-start gap-2">
                      <input
                        id="org-namespace-transfer-confirm"
                        bind:checked={namespaceTransferConfirmed}
                        type="checkbox"
                        name="confirm"
                        disabled={transferringNamespaceOwnership}
                      />
                      <span
                        >I understand this namespace transfer is immediate.</span
                      >
                    </label>
                    <div class="token-row__actions" style="margin-top:12px;">
                      <button
                        id="org-namespace-transfer-submit"
                        type="submit"
                        class="btn btn-danger"
                        disabled={transferringNamespaceOwnership}
                      >
                        {transferringNamespaceOwnership
                          ? 'Transferring...'
                          : 'Transfer namespace'}
                      </button>
                      <button
                        type="button"
                        class="btn btn-secondary"
                        on:click={cancelNamespaceTransferConfirmation}
                        disabled={transferringNamespaceOwnership}
                      >
                        Keep namespace claim
                      </button>
                    </div>
                  </div>
                {:else}
                  <button
                    id="org-namespace-transfer-toggle"
                    type="button"
                    class="btn btn-danger"
                    aria-label="Review namespace claim transfer"
                    on:click={openNamespaceTransferConfirmation}
                  >
                    Transfer namespace...
                  </button>
                {/if}
              </form>
            {/if}
          </div>
        {/if}
      </section>
    </div>

    <section class="card settings-section">
      <h2>Visible packages</h2>
      {#if packagesError}
        <div class="alert alert-error">{packagesError}</div>
      {:else if packages.length === 0}
        <div class="empty-state">
          <h3>No packages yet</h3>
          <p>No packages are currently visible for this organization.</p>
        </div>
      {:else}
        <div class="token-list">
          {#each packages as pkg}
            {@const packageSecurityPath = buildPackageSecurityPath(
              pkg.ecosystem || 'unknown',
              pkg.name || '',
              {
                severities: securityView.severities,
              }
            )}
            {@const packageDetailsPath = buildPackageDetailsPath(
              pkg.ecosystem || 'unknown',
              pkg.name || ''
            )}
            <div class="token-row">
              <div class="token-row__main">
                <div class="token-row__title">
                  <a
                    href={packageSecurityPath}
                    data-sveltekit-preload-data="hover"
                    >{pkg.name || 'Unnamed package'}</a
                  >
                </div>
                <div class="token-row__meta">
                  <span>{pkg.ecosystem || 'unknown'}</span>
                  <span>{formatNumber(pkg.download_count)} downloads</span>
                  <span>created {formatDate(pkg.created_at)}</span>
                </div>
                {#if pkg.description}<p class="settings-copy">
                    {pkg.description}
                  </p>{/if}
              </div>
              <div class="token-row__actions">
                <a
                  href={packageDetailsPath}
                  class="btn btn-secondary btn-sm"
                  data-sveltekit-preload-data="hover"
                  >Open package details</a
                >
              </div>
            </div>
          {/each}
        </div>
      {/if}

      {#if canAdminister}
        <div class="settings-subsection">
          <h3>Transfer package ownership</h3>
          {#if packagesError}
            <p class="settings-copy">
              Packages must load successfully before you can transfer one.
            </p>
          {:else if transferablePackages.length === 0}
            <p class="settings-copy">
              No visible packages are currently transferable with this
              credential.
            </p>
          {:else if packageTransferTargets.length === 0}
            <p class="settings-copy">
              You do not administer another organization that can receive one of
              these packages.
            </p>
          {:else}
            <div class="alert alert-warning" style="margin-bottom:12px;">
              This transfer is immediate and revokes existing team grants on the
              package.
            </div>
            <form on:submit={handlePackageTransfer}>
              <div class="grid gap-4 xl:grid-cols-2">
                <div class="form-group">
                  <label for="org-package-transfer-package"
                    >Organization package</label
                  >
                  <select
                    id="org-package-transfer-package"
                    name="package_key"
                    class="form-input"
                    required
                  >
                    <option value="">Select a package</option>
                    {#each transferablePackages as pkg}
                      <option
                        value={renderPackageSelectionValue(
                          pkg.ecosystem,
                          pkg.name
                        )}
                        >{`${pkg.ecosystem || ''} · ${pkg.name || ''}`}</option
                      >
                    {/each}
                  </select>
                </div>
                <div class="form-group">
                  <label for="org-package-transfer-target"
                    >Target organization</label
                  >
                  <select
                    id="org-package-transfer-target"
                    name="target_org_slug"
                    class="form-input"
                    required
                  >
                    <option value="">Select an organization</option>
                    {#each packageTransferTargets as target}
                      <option value={target.slug || ''}
                        >{target.name ||
                          target.slug ||
                          'Unnamed organization'}</option
                      >
                    {/each}
                  </select>
                </div>
              </div>
              {#if packageTransferConfirmationOpen}
                <div
                  class="alert alert-warning"
                  id="org-package-transfer-confirmation"
                  style="margin-bottom:12px;"
                >
                  <label class="flex items-start gap-2">
                    <input
                      id="org-package-transfer-confirm"
                      bind:checked={packageTransferConfirmed}
                      type="checkbox"
                      name="confirm"
                      disabled={transferringPackageOwnershipFlow}
                    />
                    <span
                      >I understand this package transfer is immediate and
                      existing team grants will be removed.</span
                    >
                  </label>
                  <div class="token-row__actions" style="margin-top:12px;">
                    <button
                      id="org-package-transfer-submit"
                      type="submit"
                      class="btn btn-danger"
                      disabled={transferringPackageOwnershipFlow}
                    >
                      {transferringPackageOwnershipFlow
                        ? 'Transferring...'
                        : 'Transfer package'}
                    </button>
                    <button
                      type="button"
                      class="btn btn-secondary"
                      on:click={cancelPackageTransferConfirmation}
                      disabled={transferringPackageOwnershipFlow}
                    >
                      Keep package
                    </button>
                  </div>
                </div>
              {:else}
                <button
                  id="org-package-transfer-toggle"
                  type="button"
                  class="btn btn-danger"
                  aria-label="Review package ownership transfer"
                  on:click={openPackageTransferConfirmation}
                >
                  Transfer package...
                </button>
              {/if}
            </form>
          {/if}
        </div>
      {/if}
    </section>
  </div>
{/if}
