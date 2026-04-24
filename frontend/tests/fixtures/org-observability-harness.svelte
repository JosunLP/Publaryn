<script lang="ts">
  import type { OrgSecurityPackageSummary } from '../../src/api/orgs';
  import type { SecurityFinding } from '../../src/api/packages';
  import {
    createOrgObservabilityController,
    type OrgObservabilityMutations,
    type OrgSecurityFindingState,
  } from '../../src/pages/org-observability';
  import { getAuditViewFromQuery } from '../../src/pages/org-audit-query';
  import { getOrgSecurityViewFromQuery } from '../../src/pages/org-security-query';
  import { buildOrgSecurityPackageKey } from '../../src/pages/org-security-triage';

  export let slug = 'source-org';
  export let initialSearch = '';
  export let mutations: OrgObservabilityMutations | undefined = undefined;

  const auditActorOptions = [
    {
      userId: '11111111-1111-4111-8111-111111111111',
      username: 'admin-user',
      label: 'Admin User (@admin-user)',
    },
  ];

  const securityPackages: OrgSecurityPackageSummary[] = [
    {
      package_id: 'pkg-1',
      ecosystem: 'npm',
      name: 'security-package',
      open_findings: 1,
      worst_severity: 'high',
    },
  ];

  let notice: string | null = null;
  let error: string | null = null;
  let exportingAudit = false;
  let exportingSecurity = false;
  let currentSearchParams = new URLSearchParams(initialSearch);
  let lastNavigation = '';
  let lastDownloadFilename = '';
  let lastDownloadContents = '';
  let lastDownloadContentType = '';
  let securityOverviewReloads = 0;
  let securityFindingsByPackageKey: Record<string, OrgSecurityFindingState> = {};
  let findingState = createSecurityFindingState();

  $: auditView = getAuditViewFromQuery(currentSearchParams);
  $: securityView = getOrgSecurityViewFromQuery(currentSearchParams);

  function createSecurityFindingState(
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

  function getSecurityFindingState(
    securityPackage: Pick<OrgSecurityPackageSummary, 'ecosystem' | 'name'>
  ): OrgSecurityFindingState {
    return (
      securityFindingsByPackageKey[
        buildOrgSecurityPackageKey(securityPackage.ecosystem, securityPackage.name)
      ] || createSecurityFindingState()
    );
  }

  function updateSecurityFindingState(
    packageKey: string,
    updates: Partial<OrgSecurityFindingState>
  ): void {
    securityFindingsByPackageKey = {
      ...securityFindingsByPackageKey,
      [packageKey]: {
        ...(securityFindingsByPackageKey[packageKey] ||
          createSecurityFindingState()),
        ...updates,
      },
    };
  }

  function updateSecurityFindingNote(
    packageKey: string,
    findingId: string,
    value: string
  ): void {
    const currentState =
      securityFindingsByPackageKey[packageKey] || createSecurityFindingState();
    updateSecurityFindingState(packageKey, {
      findingNotes: {
        ...currentState.findingNotes,
        [findingId]: value,
      },
    });
  }

  async function goto(path: string): Promise<void> {
    lastNavigation = path;
    currentSearchParams = new URL(path, 'https://example.test').searchParams;
  }

  async function reload(
    options: { notice?: string | null; error?: string | null } = {}
  ): Promise<void> {
    notice = options.notice ?? null;
    error = options.error ?? null;
  }

  async function reloadSecurityOverview(): Promise<void> {
    securityOverviewReloads += 1;
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }

  function downloadTextFile(
    filename: string,
    contents: string,
    contentType: string
  ): void {
    lastDownloadFilename = filename;
    lastDownloadContents = contents;
    lastDownloadContentType = contentType;
  }

  const controller = createOrgObservabilityController({
    getOrgSlug: () => slug,
    getCurrentSearchParams: () => currentSearchParams,
    goto,
    reload,
    toErrorMessage,
    downloadTextFile,
    getAuditActorOptions: () => auditActorOptions,
    getAuditView: () => auditView,
    getSecurityView: () => securityView,
    setExportingAudit: (value) => {
      exportingAudit = value;
    },
    setExportingSecurity: (value) => {
      exportingSecurity = value;
    },
    getSecurityFindingState,
    updateSecurityFindingState,
    reloadSecurityOverview,
    mutations,
  });

  const packageSummary = securityPackages[0];
  const packageKey = buildOrgSecurityPackageKey(
    packageSummary.ecosystem,
    packageSummary.name
  );
  $: findingState =
    securityFindingsByPackageKey[packageKey] || createSecurityFindingState();
</script>

{#if notice}<div class="alert alert-success">{notice}</div>{/if}
{#if error}<div class="alert alert-error">{error}</div>{/if}

<div data-test="last-navigation">{lastNavigation}</div>
<div data-test="last-download-filename">{lastDownloadFilename}</div>
<div data-test="last-download-contents">{lastDownloadContents}</div>
<div data-test="last-download-content-type">{lastDownloadContentType}</div>
<div data-test="security-overview-reloads">{securityOverviewReloads}</div>

<form id="audit-filter-form" on:submit={(event) => controller.submitAuditFilter(event)}>
  <select id="audit-filter-action" name="action">
    <option value="">All actions</option>
    <option value="org_update">org_update</option>
    <option value="team_create">team_create</option>
  </select>
  <input id="audit-filter-actor" name="actor_query" />
  <input id="audit-filter-from" name="occurred_from" />
  <input id="audit-filter-until" name="occurred_until" />
  <button type="submit">Apply audit filters</button>
</form>
<button id="audit-clear-action" type="button" on:click={controller.clearAuditActionFilter}>
  Clear action
</button>
<button id="audit-clear-actor" type="button" on:click={controller.clearAuditActorFilter}>
  Clear actor
</button>
<button id="audit-clear-date" type="button" on:click={controller.clearAuditDateFilter}>
  Clear date
</button>
<button
  id="audit-focus-actor"
  type="button"
  on:click={() =>
    controller.focusAuditActor(
      '11111111-1111-4111-8111-111111111111',
      'admin-user'
    )}
>
  Focus actor
</button>
<button id="audit-next-page" type="button" on:click={() => controller.goToAuditPage(2)}>
  Next audit page
</button>
<button id="audit-export" type="button" on:click={controller.exportAudit}>
  {exportingAudit ? 'Exporting audit…' : 'Export audit'}
</button>

<form
  id="security-filter-form"
  on:submit={(event) => controller.submitSecurityFilter(event)}
>
  <label>
    <input
      id="security-filter-severity-high"
      type="checkbox"
      name="security_severity"
      value="high"
      checked={securityView.severities.includes('high')}
    />
    High
  </label>
  <select id="security-filter-ecosystem" name="security_ecosystem">
    <option value="">All ecosystems</option>
    <option value="npm">npm</option>
    <option value="cargo">cargo</option>
  </select>
  <input id="security-filter-package" name="security_package" />
  <button type="submit">Apply security filters</button>
</form>
<button
  id="security-clear-severity"
  type="button"
  on:click={controller.clearSecuritySeverityFilter}
>
  Clear severity
</button>
<button
  id="security-clear-ecosystem"
  type="button"
  on:click={controller.clearSecurityEcosystemFilter}
>
  Clear ecosystem
</button>
<button
  id="security-clear-package"
  type="button"
  on:click={controller.clearSecurityPackageFilter}
>
  Clear package
</button>
<button id="security-export" type="button" on:click={controller.exportSecurity}>
  {exportingSecurity ? 'Exporting security…' : 'Export security'}
</button>

<section data-test="security-package">
  <button
    id="security-findings-toggle"
    type="button"
    on:click={() => controller.toggleSecurityFindings(packageSummary)}
  >
    Toggle findings
  </button>

  <div data-test="security-load-error">{findingState.load_error || ''}</div>
  <div data-test="security-state-error">{findingState.error || ''}</div>
  <div data-test="security-state-notice">{findingState.notice || ''}</div>
  <div data-test="security-state-loading">{findingState.loading ? 'loading' : 'idle'}</div>

  {#if findingState.expanded}
    <div data-test="security-findings-expanded">
      {#each findingState.findings as finding (finding.id)}
        <div data-test={`finding-${finding.id}`}>
          <span>{finding.title}</span>
          <span>{finding.is_resolved ? 'resolved' : 'open'}</span>
          <textarea
            id={`finding-note-${finding.id}`}
            on:input={(event) =>
              updateSecurityFindingNote(
                packageKey,
                finding.id,
                (event.currentTarget as HTMLTextAreaElement).value
              )}
          >{findingState.findingNotes[finding.id] || ''}</textarea>
          <button
            id={`finding-toggle-${finding.id}`}
            type="button"
            on:click={() =>
              controller.toggleSecurityFindingResolution(packageSummary, finding)}
          >
            {findingState.updatingFindingId === finding.id
              ? 'Updating…'
              : finding.is_resolved
                ? 'Reopen finding'
                : 'Resolve finding'}
          </button>
        </div>
      {/each}
    </div>
  {/if}
</section>
