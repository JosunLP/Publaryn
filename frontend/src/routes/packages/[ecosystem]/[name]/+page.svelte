<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';

  import { ApiError, getAuthToken } from '../../../../api/client';
  import type { OrganizationMembership } from '../../../../api/orgs';
  import { listMyOrganizations } from '../../../../api/orgs';
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
  } from '../../../../api/packages';
  import {
    ecosystemIcon,
    ecosystemLabel,
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
  let releaseNotice: string | null = null;
  let releaseError: string | null = null;
  let transferNotice: string | null = null;
  let transferError: string | null = null;
  let packageSettingsNotice: string | null = null;
  let packageSettingsError: string | null = null;
  let trustedPublisherNotice: string | null = null;
  let trustedPublisherError: string | null = null;
  let includeResolvedFindings = false;
  let activeTab: 'readme' | 'versions' | 'security' = 'readme';

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

  $: ecosystem = $page.params.ecosystem ?? '';
  $: name = $page.params.name ?? '';
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
    releaseNotice = null;
    releaseError = null;
    transferNotice = null;
    transferError = null;
    packageSettingsNotice = null;
    packageSettingsError = null;
    trustedPublisherNotice = null;
    trustedPublisherError = null;
    activeTab = 'readme';
    includeResolvedFindings = false;
    resetReleaseForm();
    resetTransferForm();
    resetPackageSettingsForm();
    resetTrustedPublisherForm();

    try {
      pkg = await getPackage(eecosystem(), ename());
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
    ] = await Promise.all([
      listReleases(eecosystem(), ename(), { perPage: 20 }).catch(
        () => [] as Release[]
      ),
      listTags(eecosystem(), ename()).catch(() => [] as Tag[]),
      listSecurityFindings(eecosystem(), ename()).catch(
        () => [] as SecurityFinding[]
      ),
      loadTransferState(pkg),
      loadTrustedPublisherState(pkg),
    ]);

    releases = loadedReleases;
    tags = loadedTags;
    findings = loadedFindings;
    transferState = loadedTransferState;
    trustedPublisherState = loadedTrustedPublisherState;
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
      const result = await updatePackage(eecosystem(), ename(), input);
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
    if (!currentPackage || eecosystem() !== 'pypi') {
      return {
        publishers: [],
        loadError: null,
      };
    }

    try {
      return {
        publishers: await listTrustedPublishersForPackage(
          eecosystem(),
          ename()
        ),
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

  async function handleCopyInstall(): Promise<void> {
    if (!pkg) {
      return;
    }

    const latestVersion =
      pkg.latest_version ??
      (releases.length > 0 ? (releases[0]?.version ?? null) : null);
    const command = latestVersion
      ? installCommand(eecosystem(), pkg.name, latestVersion)
      : installCommand(eecosystem(), pkg.name);
    await copyToClipboard(command);
  }

  async function handleResolvedToggleChange(): Promise<void> {
    try {
      findings = await listSecurityFindings(eecosystem(), ename(), {
        includeResolved: includeResolvedFindings,
      });
    } catch {
      findings = [];
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
      await createRelease(eecosystem(), ename(), {
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
        `/packages/${encodeURIComponent(eecosystem())}/${encodeURIComponent(ename())}/versions/${encodeURIComponent(newReleaseVersion.trim())}?notice=${notice}`
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
      const result = await transferPackageOwnership(eecosystem(), ename(), {
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
      await createTrustedPublisherForPackage(eecosystem(), ename(), input);
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
      await deleteTrustedPublisherForPackage(
        eecosystem(),
        ename(),
        publisher.id
      );
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

  function eecosystem(): string {
    return ecosystem;
  }

  function ename(): string {
    return name;
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
    ? installCommand(eecosystem(), pkg.name, latestVersion)
    : installCommand(eecosystem(), pkg.name)}

  <div class="mt-6">
    <div class="pkg-header">
      <h1 class="pkg-header__name">{pkg.display_name || pkg.name}</h1>
      <span class="badge badge-ecosystem"
        >{ecosystemIcon(eecosystem())} {ecosystemLabel(eecosystem())}</span
      >
      {#if latestVersion}
        <span class="pkg-header__version">v{latestVersion}</span>
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
            on:click={() => (activeTab = 'readme')}>Readme</button
          >
          <button
            class:active={activeTab === 'versions'}
            class="tab"
            type="button"
            on:click={() => (activeTab = 'versions')}
            >Versions ({releases.length})</button
          >
          <button
            class:active={activeTab === 'security'}
            class="tab"
            type="button"
            on:click={() => (activeTab = 'security')}
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
                    href={`/packages/${encodeURIComponent(eecosystem())}/${encodeURIComponent(ename())}/versions/${encodeURIComponent(release.version)}`}
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
                      >v{finding.release_version}</span
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
              </div>
            {/each}
          {/if}
        {/if}
      </div>

      <div class="pkg-detail__sidebar">
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

        {#if eecosystem() === 'pypi'}
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
