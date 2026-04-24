<script lang="ts">
  import { page } from '$app/stores';

  import { ApiError } from '../../../../../../api/client';
  import type { Artifact, Release } from '../../../../../../api/packages';
  import {
    deprecateRelease,
    getRelease,
    listArtifacts,
    publishRelease,
    undeprecateRelease,
    unyankRelease,
    uploadReleaseArtifact,
    yankRelease,
  } from '../../../../../../api/packages';
  import {
    ecosystemIcon,
    ecosystemLabel,
    formatVersionLabel,
    installCommand,
  } from '../../../../../../utils/ecosystem';
  import {
    copyToClipboard,
    formatDate,
    formatFileSize,
  } from '../../../../../../utils/format';
  import {
    buildBundleAnalysisHighlights,
    buildBundleAnalysisStats,
    bundleAnalysisNotes,
  } from '../../../../../../utils/package-analysis';
  import {
    ARTIFACT_KIND_OPTIONS,
    describeReleaseReadiness,
    formatArtifactKindLabel,
    formatReleaseStatusLabel,
    getDefaultArtifactKindForEcosystem,
    getReleaseActionAvailability,
    getReleaseTimestampLabel,
    getRestoreReleaseLabel,
  } from '../../../../../../utils/releases';

  let lastLoadKey = '';
  let loading = true;
  let notFound = false;
  let loadError: string | null = null;
  let notice: string | null = null;
  let error: string | null = null;
  let release: Release | null = null;
  let artifacts: Artifact[] = [];

  let artifactKind = 'tarball';
  let artifactFile: File | null = null;
  let artifactSha256 = '';
  let artifactSigned = false;
  let signatureKeyId = '';
  let uploadingArtifact = false;

  let yankReason = '';
  let deprecationMessage = '';
  let publishing = false;
  let yanking = false;
  let restoring = false;
  let deprecating = false;
  let undeprecating = false;

  $: ecosystem = $page.params.ecosystem ?? '';
  $: name = $page.params.name ?? '';
  $: version = $page.params.version ?? '';
  $: loadKey = `${ecosystem}|${name}|${version}|${$page.url.search}`;
  $: if (ecosystem && name && version && loadKey !== lastLoadKey) {
    lastLoadKey = loadKey;
    void loadVersionPage();
  }

  async function loadVersionPage(): Promise<void> {
    loading = true;
    notFound = false;
    loadError = null;
    release = null;
    artifacts = [];
    error = null;
    notice = $page.url.searchParams.get('notice') || null;
    artifactKind = getDefaultArtifactKindForEcosystem(eecosystem());
    artifactFile = null;
    artifactSha256 = '';
    artifactSigned = false;
    signatureKeyId = '';
    yankReason = '';
    deprecationMessage = '';

    try {
      release = await getRelease(eecosystem(), ename(), eversion());
    } catch (caughtError: unknown) {
      if (caughtError instanceof ApiError && caughtError.status === 404) {
        notFound = true;
      } else {
        loadError =
          caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Failed to load version.';
      }
      loading = false;
      return;
    }

    try {
      artifacts = await listArtifacts(eecosystem(), ename(), eversion());
    } catch {
      artifacts = [];
    } finally {
      loading = false;
    }
  }

  async function handleCopyInstall(): Promise<void> {
    await copyToClipboard(installCommand(eecosystem(), ename(), eversion()));
  }

  async function handleUploadArtifact(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!artifactFile) {
      error = 'Choose an artifact file to upload.';
      notice = null;
      return;
    }

    uploadingArtifact = true;
    error = null;
    notice = null;

    try {
      await uploadReleaseArtifact(eecosystem(), ename(), eversion(), {
        filename: artifactFile.name,
        kind: artifactKind,
        file: artifactFile,
        sha256: artifactSha256.trim() || undefined,
        isSigned: artifactSigned,
        signatureKeyId: signatureKeyId.trim() || undefined,
      });

      notice = `Uploaded ${artifactFile.name}.`;
      await loadVersionPage();
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to upload artifact.';
    } finally {
      uploadingArtifact = false;
    }
  }

  async function handlePublish(): Promise<void> {
    publishing = true;
    error = null;
    notice = null;

    try {
      const result = await publishRelease(eecosystem(), ename(), eversion());
      notice = result.message || 'Release submitted for scanning.';
      await loadVersionPage();
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to publish release.';
    } finally {
      publishing = false;
    }
  }

  async function handleYank(): Promise<void> {
    yanking = true;
    error = null;
    notice = null;

    try {
      const result = await yankRelease(eecosystem(), ename(), eversion(), {
        reason: yankReason.trim() || undefined,
      });
      notice = result.message || 'Release yanked successfully.';
      await loadVersionPage();
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to yank release.';
    } finally {
      yanking = false;
    }
  }

  async function handleRestore(): Promise<void> {
    restoring = true;
    error = null;
    notice = null;

    try {
      const result = await unyankRelease(eecosystem(), ename(), eversion());
      notice = result.message || 'Release restored successfully.';
      await loadVersionPage();
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to restore release.';
    } finally {
      restoring = false;
    }
  }

  async function handleDeprecate(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    deprecating = true;
    error = null;
    notice = null;

    try {
      const result = await deprecateRelease(eecosystem(), ename(), eversion(), {
        message: deprecationMessage.trim() || undefined,
      });
      notice = result.message || 'Release deprecated successfully.';
      await loadVersionPage();
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to deprecate release.';
    } finally {
      deprecating = false;
    }
  }

  async function handleUndeprecate(): Promise<void> {
    undeprecating = true;
    error = null;
    notice = null;

    try {
      const result = await undeprecateRelease(
        eecosystem(),
        ename(),
        eversion()
      );
      await loadVersionPage();
      notice = result.message || 'Release undeprecated successfully.';
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to remove release deprecation.';
    } finally {
      undeprecating = false;
    }
  }
  function formatJson(value: unknown): string {
    return JSON.stringify(value ?? null, null, 2);
  }

  function isRecord(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
  }

  function stringValue(value: unknown): string | null {
    return typeof value === 'string' && value.trim().length > 0 ? value : null;
  }

  function stringArrayValue(value: unknown): string[] {
    return Array.isArray(value)
      ? value.filter(
          (entry): entry is string =>
            typeof entry === 'string' && entry.trim().length > 0
        )
      : [];
  }

  function numberValue(value: unknown): number | null {
    return typeof value === 'number' && Number.isFinite(value) ? value : null;
  }

  function booleanLabel(value: boolean | null | undefined): string {
    return value ? 'Yes' : 'No';
  }

  function hasJsonContent(value: unknown): boolean {
    if (Array.isArray(value)) {
      return value.length > 0;
    }

    if (isRecord(value)) {
      return Object.keys(value).length > 0;
    }

    return (
      value !== null && value !== undefined && `${value}`.trim().length > 0
    );
  }

  function formatOciReferenceKind(kind: string | null | undefined): string {
    switch ((kind || '').toLowerCase()) {
      case 'config':
        return 'Config';
      case 'layer':
        return 'Layer';
      case 'subject':
        return 'Subject';
      default:
        return kind || 'Reference';
    }
  }

  function eecosystem(): string {
    return ecosystem;
  }

  function ename(): string {
    return name;
  }

  function eversion(): string {
    return version;
  }

  $: artifactCount = artifacts.length;
  $: bundleAnalysis = release?.bundle_analysis ?? null;
  $: releaseMetadata = release?.ecosystem_metadata ?? null;
  $: cargoMetadata =
    releaseMetadata?.kind === 'cargo' ? releaseMetadata.details : null;
  $: nugetMetadata =
    releaseMetadata?.kind === 'nuget' ? releaseMetadata.details : null;
  $: rubygemsMetadata =
    releaseMetadata?.kind === 'rubygems' ? releaseMetadata.details : null;
  $: mavenMetadata =
    releaseMetadata?.kind === 'maven' && isRecord(releaseMetadata.details)
      ? releaseMetadata.details
      : null;
  $: composerMetadata =
    releaseMetadata?.kind === 'composer' && isRecord(releaseMetadata.details)
      ? releaseMetadata.details
      : null;
  $: ociMetadata =
    releaseMetadata?.kind === 'oci' ? releaseMetadata.details : null;
  $: mavenLicenses = stringArrayValue(mavenMetadata?.licenses);
  $: composerLicenses = stringArrayValue(composerMetadata?.license);
  $: composerKeywords = stringArrayValue(composerMetadata?.keywords);
  $: composerRequire = isRecord(composerMetadata?.require)
    ? composerMetadata.require
    : null;
  $: composerAutoload = isRecord(composerMetadata?.autoload)
    ? composerMetadata.autoload
    : null;
  $: composerSupport = isRecord(composerMetadata?.support)
    ? composerMetadata.support
    : null;
  $: ociManifest =
    ociMetadata && isRecord(ociMetadata.manifest) ? ociMetadata.manifest : null;
  $: ociConfig = isRecord(ociManifest?.config) ? ociManifest.config : null;
  $: ociLayers = Array.isArray(ociManifest?.layers) ? ociManifest.layers : [];
  $: ociSubject = isRecord(ociManifest?.subject) ? ociManifest.subject : null;
  $: actionAvailability = release
    ? getReleaseActionAvailability(release, artifactCount)
    : {
        canUploadArtifact: false,
        canPublish: false,
        canYank: false,
        canRestore: false,
        canDeprecate: false,
        canUndeprecate: false,
      };
  $: releaseStatus = (release?.status || '').trim().toLowerCase();
  $: readiness = release
    ? describeReleaseReadiness(release, artifactCount)
    : { tone: 'info', message: '' };
</script>

<svelte:head>
  <title>Package version — Publaryn</title>
</svelte:head>

{#if loading}
  <div class="loading"><span class="spinner"></span> Loading…</div>
{:else if notFound}
  <div class="empty-state mt-6">
    <h2>Version not found</h2>
    <p>{ecosystem}/{name}@{version} does not exist.</p>
    <a
      href={`/packages/${encodeURIComponent(eecosystem())}/${encodeURIComponent(ename())}`}
      class="btn btn-primary mt-4"
      data-sveltekit-preload-data="hover">Back to package</a
    >
  </div>
{:else if loadError || !release}
  <div class="alert alert-error mt-6">
    Failed to load version: {loadError || 'Unknown error.'}
  </div>
{:else}
  <div class="page-shell">
    <nav class="page-breadcrumbs">
      <a
        href={`/packages/${encodeURIComponent(eecosystem())}/${encodeURIComponent(ename())}`}
        data-sveltekit-preload-data="hover"
        >{ecosystemIcon(eecosystem())} {ename()}</a
      >
      <span>&rsaquo; </span>
      <span>{formatVersionLabel(eecosystem(), eversion())}</span>
    </nav>

    <section class="page-hero">
      <div class="page-hero__header">
        <div class="page-hero__copy">
          <span class="page-hero__eyebrow">
            <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
            Release
          </span>
          <h1 class="page-hero__title">{ename()}</h1>
          <p class="page-hero__subtitle">
            Release details, artifacts, provenance, and publication controls for
            {formatVersionLabel(eecosystem(), eversion())}.
          </p>
          <div class="page-hero__meta">
            <span class="badge badge-ecosystem"
              >{ecosystemIcon(eecosystem())} {ecosystemLabel(eecosystem())}</span
            >
            <span class="badge badge-ecosystem"
              >{formatVersionLabel(eecosystem(), eversion())}</span
            >
            {#if release.is_yanked}<span class="badge badge-yanked">yanked</span>{/if}
            {#if release.is_deprecated}<span class="badge badge-deprecated"
                >deprecated</span
              >{/if}
            {#if release.status}<span class="badge badge-ecosystem"
                >{formatReleaseStatusLabel(release.status)}</span
              >{/if}
          </div>
        </div>
      </div>
    </section>

    {#if notice}<div class="alert alert-success mt-4">{notice}</div>{/if}
    {#if error}<div class="alert alert-error mt-4">{error}</div>{/if}

    <section class="detail-summary">
      <div class="detail-summary__header">
        <div>
          <div class="detail-summary__title">Install</div>
          <p class="detail-summary__copy">
            Copy the exact client command for this release.
          </p>
        </div>
      </div>
      <div class="code-block">
        <code>{installCommand(eecosystem(), ename(), eversion())}</code>
        <button class="copy-btn" type="button" on:click={handleCopyInstall}
          >Copy</button
        >
      </div>
    </section>

    <div class="detail-grid">
      <div class="detail-main">
        <div class="card mb-4">
          <h3 class="metadata-block__title">Lifecycle state</h3>
          <p
            class={`alert alert-${readiness.tone === 'warning' ? 'warning' : readiness.tone === 'success' ? 'success' : 'info'}`}
          >
            {readiness.message}
          </p>
          <div class="token-row__meta mt-4">
            <span
              >{getReleaseTimestampLabel(release.status)}
              {formatDate(release.published_at || release.created_at)}</span
            >
            {#if release.source_ref}<span>source {release.source_ref}</span
              >{/if}
            {#if release.yank_reason}<span
                >yank reason: {release.yank_reason}</span
              >{/if}
            {#if release.deprecation_message}<span
                >deprecation: {release.deprecation_message}</span
              >{/if}
          </div>
        </div>

        {#if release.description}
          <div class="card mb-4">
            <h3 class="metadata-block__title">Description</h3>
            <p>{release.description}</p>
          </div>
        {/if}

        {#if release.changelog}
          <div class="card mb-4">
            <h3 class="metadata-block__title">Changelog</h3>
            <pre>{release.changelog}</pre>
          </div>
        {/if}

        {#if cargoMetadata}
          <div class="card mb-4">
            <h3 class="metadata-block__title">Cargo metadata</h3>
            {#if cargoMetadata.rust_version}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Rust version</span>
                <span class="sidebar-row__value"
                  ><code>{cargoMetadata.rust_version}</code></span
                >
              </div>
            {/if}
            {#if cargoMetadata.links}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Links</span>
                <span class="sidebar-row__value"
                  ><code>{cargoMetadata.links}</code></span
                >
              </div>
            {/if}
            {#if hasJsonContent(cargoMetadata.dependencies)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Dependencies
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(cargoMetadata.dependencies)}</code
                  ></pre>
              </div>
            {/if}
            {#if hasJsonContent(cargoMetadata.features)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Features
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(cargoMetadata.features)}</code
                  ></pre>
              </div>
            {/if}
            {#if hasJsonContent(cargoMetadata.features2)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Extended features
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(cargoMetadata.features2)}</code
                  ></pre>
              </div>
            {/if}
          </div>
        {/if}

        {#if nugetMetadata}
          <div class="card mb-4">
            <h3 style="margin-bottom:8px;">NuGet metadata</h3>
            {#if nugetMetadata.title}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Title</span>
                <span class="sidebar-row__value">{nugetMetadata.title}</span>
              </div>
            {/if}
            {#if nugetMetadata.authors}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Authors</span>
                <span class="sidebar-row__value">{nugetMetadata.authors}</span>
              </div>
            {/if}
            <div class="sidebar-row">
              <span class="sidebar-row__label">Listed</span>
              <span class="sidebar-row__value"
                >{booleanLabel(nugetMetadata.is_listed)}</span
              >
            </div>
            {#if nugetMetadata.min_client_version}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Minimum client</span>
                <span class="sidebar-row__value"
                  ><code>{nugetMetadata.min_client_version}</code></span
                >
              </div>
            {/if}
            {#if nugetMetadata.summary}
              <p style="margin-top:12px;">{nugetMetadata.summary}</p>
            {/if}
            {#if nugetMetadata.tags.length > 0}
              <div
                style="margin-top:12px; display:flex; flex-wrap:wrap; gap:6px;"
              >
                {#each nugetMetadata.tags as tag}
                  <span class="badge badge-ecosystem">{tag}</span>
                {/each}
              </div>
            {/if}
            {#if nugetMetadata.project_url || nugetMetadata.icon_url || nugetMetadata.license_url}
              <div
                style="margin-top:12px; display:flex; flex-direction:column; gap:6px;"
              >
                {#if nugetMetadata.project_url}
                  <a
                    href={nugetMetadata.project_url}
                    target="_blank"
                    rel="noopener noreferrer">Project URL</a
                  >
                {/if}
                {#if nugetMetadata.icon_url}
                  <a
                    href={nugetMetadata.icon_url}
                    target="_blank"
                    rel="noopener noreferrer">Icon URL</a
                  >
                {/if}
                {#if nugetMetadata.license_url}
                  <a
                    href={nugetMetadata.license_url}
                    target="_blank"
                    rel="noopener noreferrer">License URL</a
                  >
                {/if}
              </div>
            {/if}
            {#if nugetMetadata.license_expression}
              <div class="sidebar-row" style="margin-top:12px;">
                <span class="sidebar-row__label">License expression</span>
                <span class="sidebar-row__value"
                  ><code>{nugetMetadata.license_expression}</code></span
                >
              </div>
            {/if}
            {#if hasJsonContent(nugetMetadata.dependency_groups)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Dependency groups
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(nugetMetadata.dependency_groups)}</code
                  ></pre>
              </div>
            {/if}
            {#if hasJsonContent(nugetMetadata.package_types)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Package types
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(nugetMetadata.package_types)}</code
                  ></pre>
              </div>
            {/if}
          </div>
        {/if}

        {#if rubygemsMetadata}
          <div class="card mb-4">
            <h3 style="margin-bottom:8px;">RubyGems metadata</h3>
            <div class="sidebar-row">
              <span class="sidebar-row__label">Platform</span>
              <span class="sidebar-row__value"
                ><code>{rubygemsMetadata.platform}</code></span
              >
            </div>
            {#if rubygemsMetadata.summary}
              <p style="margin-top:12px;">{rubygemsMetadata.summary}</p>
            {/if}
            {#if rubygemsMetadata.authors.length > 0}
              <div class="sidebar-row" style="margin-top:12px;">
                <span class="sidebar-row__label">Authors</span>
                <span class="sidebar-row__value"
                  >{rubygemsMetadata.authors.join(', ')}</span
                >
              </div>
            {/if}
            {#if rubygemsMetadata.licenses.length > 0}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Licenses</span>
                <span class="sidebar-row__value"
                  >{rubygemsMetadata.licenses.join(', ')}</span
                >
              </div>
            {/if}
            {#if rubygemsMetadata.required_ruby_version}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Required Ruby</span>
                <span class="sidebar-row__value"
                  ><code>{rubygemsMetadata.required_ruby_version}</code></span
                >
              </div>
            {/if}
            {#if rubygemsMetadata.required_rubygems_version}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Required RubyGems</span>
                <span class="sidebar-row__value"
                  ><code>{rubygemsMetadata.required_rubygems_version}</code
                  ></span
                >
              </div>
            {/if}
            {#if hasJsonContent(rubygemsMetadata.runtime_dependencies)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Runtime dependencies
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(rubygemsMetadata.runtime_dependencies)}</code
                  ></pre>
              </div>
            {/if}
            {#if hasJsonContent(rubygemsMetadata.development_dependencies)}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Development dependencies
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(
                      rubygemsMetadata.development_dependencies
                    )}</code
                  ></pre>
              </div>
            {/if}
          </div>
        {/if}

        {#if mavenMetadata}
          <div class="card mb-4">
            <h3 style="margin-bottom:8px;">Maven provenance</h3>
            {#if stringValue(mavenMetadata.group_id)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Group ID</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(mavenMetadata.group_id)}</code></span
                >
              </div>
            {/if}
            {#if stringValue(mavenMetadata.artifact_id)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Artifact ID</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(mavenMetadata.artifact_id)}</code></span
                >
              </div>
            {/if}
            {#if stringValue(mavenMetadata.version)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Version</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(mavenMetadata.version)}</code></span
                >
              </div>
            {/if}
            {#if stringValue(mavenMetadata.packaging)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Packaging</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(mavenMetadata.packaging)}</code></span
                >
              </div>
            {/if}
            {#if stringValue(mavenMetadata.display_name)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Display name</span>
                <span class="sidebar-row__value"
                  >{stringValue(mavenMetadata.display_name)}</span
                >
              </div>
            {/if}
            {#if stringValue(mavenMetadata.description)}
              <p style="margin-top:12px;">
                {stringValue(mavenMetadata.description)}
              </p>
            {/if}
            {#if stringValue(mavenMetadata.homepage) || stringValue(mavenMetadata.repository_url)}
              <div
                style="margin-top:12px; display:flex; flex-direction:column; gap:6px;"
              >
                {#if stringValue(mavenMetadata.homepage)}
                  <a
                    href={stringValue(mavenMetadata.homepage)}
                    target="_blank"
                    rel="noopener noreferrer">Homepage</a
                  >
                {/if}
                {#if stringValue(mavenMetadata.repository_url)}
                  <a
                    href={stringValue(mavenMetadata.repository_url)}
                    target="_blank"
                    rel="noopener noreferrer">Repository</a
                  >
                {/if}
              </div>
            {/if}
            {#if mavenLicenses.length > 0}
              <div
                style="margin-top:12px; display:flex; flex-wrap:wrap; gap:6px;"
              >
                {#each mavenLicenses as license}
                  <span class="badge badge-ecosystem">{license}</span>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        {#if composerMetadata}
          <div class="card mb-4">
            <h3 style="margin-bottom:8px;">Composer manifest</h3>
            {#if stringValue(composerMetadata.name)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Name</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(composerMetadata.name)}</code></span
                >
              </div>
            {/if}
            {#if stringValue(composerMetadata.type)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Type</span>
                <span class="sidebar-row__value"
                  >{stringValue(composerMetadata.type)}</span
                >
              </div>
            {/if}
            {#if composerLicenses.length > 0}
              <div
                style="margin-top:12px; display:flex; flex-wrap:wrap; gap:6px;"
              >
                {#each composerLicenses as license}
                  <span class="badge badge-ecosystem">{license}</span>
                {/each}
              </div>
            {/if}
            {#if composerKeywords.length > 0}
              <div
                style="margin-top:12px; display:flex; flex-wrap:wrap; gap:6px;"
              >
                {#each composerKeywords as keyword}
                  <span class="badge badge-ecosystem">{keyword}</span>
                {/each}
              </div>
            {/if}
            {#if composerRequire}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Require
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(composerRequire)}</code
                  ></pre>
              </div>
            {/if}
            {#if composerAutoload}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Autoload
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(composerAutoload)}</code
                  ></pre>
              </div>
            {/if}
            {#if composerSupport}
              <div style="margin-top:12px;">
                <h4
                  style="font-size:0.875rem; font-weight:600; margin-bottom:8px;"
                >
                  Support
                </h4>
                <pre style="white-space:pre-wrap; overflow:auto;"><code
                    >{formatJson(composerSupport)}</code
                  ></pre>
              </div>
            {/if}
          </div>
        {/if}

        {#if ociMetadata}
          <div class="card mb-4">
            <h3 style="margin-bottom:8px;">OCI manifest</h3>
            {#if numberValue(ociManifest?.schemaVersion) != null}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Schema version</span>
                <span class="sidebar-row__value"
                  >{numberValue(ociManifest?.schemaVersion)}</span
                >
              </div>
            {/if}
            {#if stringValue(ociManifest?.mediaType)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Media type</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(ociManifest?.mediaType)}</code></span
                >
              </div>
            {/if}
            {#if stringValue(ociConfig?.digest)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Config digest</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(ociConfig?.digest)}</code></span
                >
              </div>
            {/if}
            {#if numberValue(ociConfig?.size) != null}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Config size</span>
                <span class="sidebar-row__value"
                  >{formatFileSize(numberValue(ociConfig?.size) ?? 0)}</span
                >
              </div>
            {/if}
            <div class="sidebar-row">
              <span class="sidebar-row__label">Layers</span>
              <span class="sidebar-row__value">{ociLayers.length}</span>
            </div>
            {#if stringValue(ociSubject?.digest)}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Subject digest</span>
                <span class="sidebar-row__value"
                  ><code>{stringValue(ociSubject?.digest)}</code></span
                >
              </div>
            {/if}
          </div>

          {#if ociMetadata.references.length > 0}
            <div class="card mb-4" style="padding:0;">
              <div style="padding:16px 20px 8px;">
                <h3 style="font-size:0.875rem; font-weight:600;">
                  Referenced blobs
                </h3>
              </div>
              {#each ociMetadata.references as reference}
                <div class="release-row">
                  <div>
                    <div class="release-row__version">
                      {formatOciReferenceKind(reference.kind)}
                    </div>
                    {#if reference.digest}
                      <div class="settings-copy" style="margin-top:6px;">
                        <code>{reference.digest}</code>
                      </div>
                    {/if}
                  </div>
                  <div class="release-row__meta">
                    {#if reference.size != null}
                      {formatFileSize(reference.size)}
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}

          {#if ociMetadata.manifest}
            <div class="card mb-4">
              <h3 style="margin-bottom:8px;">Manifest JSON</h3>
              <pre style="white-space:pre-wrap; overflow:auto;"><code
                  >{formatJson(ociMetadata.manifest)}</code
                ></pre>
            </div>
          {/if}
        {/if}

        <div class="card" style="padding:0;">
          <div style="padding:16px 20px 8px;">
            <h3 style="font-size:0.875rem; font-weight:600;">Artifacts</h3>
          </div>

          {#if artifacts.length === 0}
            <div class="empty-state"><p>No artifacts available.</p></div>
          {:else}
            {#each artifacts as artifact}
              <div class="release-row">
                <div>
                  <a
                    href={`/v1/packages/${encodeURIComponent(eecosystem())}/${encodeURIComponent(ename())}/releases/${encodeURIComponent(eversion())}/artifacts/${encodeURIComponent(artifact.filename)}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    class="release-row__version"
                  >
                    {artifact.filename}
                  </a>
                  <span
                    class="text-muted"
                    style="font-size:0.8125rem; margin-left:8px;"
                    >{formatArtifactKindLabel(
                      artifact.kind
                    )}{artifact.content_type
                      ? ` · ${artifact.content_type}`
                      : ''}</span
                  >
                  {#if artifact.is_signed}<span
                      class="badge badge-verified"
                      style="margin-left:8px;">signed</span
                    >{/if}
                  {#if artifact.sha256}
                    <div class="settings-copy" style="margin-top:6px;">
                      SHA-256 {artifact.sha256}
                    </div>
                  {/if}
                </div>
                <div class="release-row__meta">
                  {#if artifact.size_bytes != null}{formatFileSize(
                      artifact.size_bytes
                    )}{/if}
                  {#if artifact.uploaded_at}
                    {artifact.size_bytes != null ? ' · ' : ''}{formatDate(
                      artifact.uploaded_at
                    )}
                  {/if}
                </div>
              </div>
            {/each}
          {/if}
        </div>
      </div>

      <div class="detail-sidebar">
        <div class="card">
          <div class="sidebar-section">
            <h3>Version info</h3>
            {#if release.status}<div class="sidebar-row">
                <span class="sidebar-row__label">Status</span><span
                  class="sidebar-row__value"
                  >{formatReleaseStatusLabel(release.status)}</span
                >
              </div>{/if}
            {#if release.published_at}<div class="sidebar-row">
                <span class="sidebar-row__label">Published</span><span
                  class="sidebar-row__value"
                  >{formatDate(release.published_at)}</span
                >
              </div>{/if}
            {#if release.created_at}<div class="sidebar-row">
                <span class="sidebar-row__label">Created</span><span
                  class="sidebar-row__value"
                  >{formatDate(release.created_at)}</span
                >
              </div>{/if}
            {#if release.is_prerelease}<div class="sidebar-row">
                <span class="sidebar-row__label">Pre-release</span><span
                  class="sidebar-row__value">Yes</span
                >
              </div>{/if}
            {#if release.sha256}<div class="sidebar-row">
                <span class="sidebar-row__label">SHA-256</span><span
                  class="sidebar-row__value"
                  ><code style="font-size:0.75rem; word-break:break-all;"
                    >{release.sha256}</code
                  ></span
                >
              </div>{/if}
            <div class="sidebar-row">
              <span class="sidebar-row__label">Artifacts</span><span
                class="sidebar-row__value">{artifactCount}</span
              >
            </div>
          </div>
        </div>

        {#if bundleAnalysis}
          <div class="card">
            <div class="sidebar-section">
              <h3>Bundle analysis</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Bundlephobia-inspired metadata derived from stored artifacts and
                ecosystem-specific release metadata.
              </p>
              {#each buildBundleAnalysisStats(bundleAnalysis) as stat}
                <div class="sidebar-row">
                  <span class="sidebar-row__label">{stat.label}</span>
                  <span class="sidebar-row__value">{stat.value}</span>
                </div>
              {/each}
              {#if buildBundleAnalysisHighlights(bundleAnalysis).length > 0}
                <div
                  class="token-row__scopes"
                  style="margin-top:12px; margin-bottom:12px;"
                >
                  {#each buildBundleAnalysisHighlights(bundleAnalysis) as highlight}
                    <span class="badge badge-ecosystem">{highlight}</span>
                  {/each}
                </div>
              {/if}
              {#if bundleAnalysisNotes(bundleAnalysis).length > 0}
                <div class="settings-copy" style="display:grid; gap:6px; margin:0;">
                  {#each bundleAnalysisNotes(bundleAnalysis) as note}
                    <span>{note}</span>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        {/if}

        {#if release.can_manage_releases}
          <div class="card">
            <div class="sidebar-section">
              <h3>Upload artifact</h3>
              <p class="settings-copy" style="margin-bottom:12px;">
                Artifacts are immutable and deduplicated by filename and
                content.
              </p>
              <form on:submit={handleUploadArtifact}>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="artifact-file">Artifact file</label>
                  <input
                    id="artifact-file"
                    type="file"
                    class="form-input"
                    on:change={(event) => {
                      const target = event.currentTarget as HTMLInputElement;
                      artifactFile = target.files?.[0] || null;
                    }}
                  />
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="artifact-kind">Artifact kind</label>
                  <select
                    id="artifact-kind"
                    bind:value={artifactKind}
                    class="form-input"
                  >
                    {#each ARTIFACT_KIND_OPTIONS as option}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                  </select>
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="artifact-sha256">SHA-256 (optional)</label>
                  <input
                    id="artifact-sha256"
                    bind:value={artifactSha256}
                    class="form-input"
                    placeholder="Optional artifact checksum"
                  />
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label class="flex items-start gap-2">
                    <input bind:checked={artifactSigned} type="checkbox" />
                    <span>Artifact is signed</span>
                  </label>
                </div>
                {#if artifactSigned}
                  <div class="form-group" style="margin-bottom:12px;">
                    <label for="signature-key-id">Signature key id</label>
                    <input
                      id="signature-key-id"
                      bind:value={signatureKeyId}
                      class="form-input"
                      placeholder="Optional signing key reference"
                    />
                  </div>
                {/if}
                <button
                  type="submit"
                  class="btn btn-primary"
                  style="width:100%; justify-content:center;"
                  disabled={!actionAvailability.canUploadArtifact ||
                    uploadingArtifact}
                >
                  {uploadingArtifact ? 'Uploading…' : 'Upload artifact'}
                </button>
              </form>
            </div>
          </div>

          <div class="card">
            <div class="sidebar-section">
              <h3>Lifecycle actions</h3>
              {#if actionAvailability.canPublish}
                <button
                  type="button"
                  class="btn btn-primary"
                  style="width:100%; justify-content:center; margin-bottom:12px;"
                  disabled={publishing}
                  on:click={handlePublish}
                >
                  {publishing ? 'Publishing…' : 'Publish release'}
                </button>
              {:else if releaseStatus === 'quarantine' && actionAvailability.canUploadArtifact}
                <p class="settings-copy" style="margin-bottom:12px;">
                  Upload at least one artifact before publishing.
                </p>
              {/if}

              {#if actionAvailability.canYank}
                <div class="form-group" style="margin-bottom:12px;">
                  <label for="yank-reason">Yank reason (optional)</label>
                  <input
                    id="yank-reason"
                    bind:value={yankReason}
                    class="form-input"
                    placeholder="Why should consumers avoid this release?"
                  />
                </div>
                <button
                  type="button"
                  class="btn btn-danger"
                  style="width:100%; justify-content:center; margin-bottom:12px;"
                  disabled={yanking}
                  on:click={handleYank}
                >
                  {yanking ? 'Yanking…' : 'Yank release'}
                </button>
              {/if}

              {#if actionAvailability.canRestore}
                <button
                  type="button"
                  class="btn btn-secondary"
                  style="width:100%; justify-content:center; margin-bottom:12px;"
                  disabled={restoring}
                  on:click={handleRestore}
                >
                  {restoring ? 'Restoring…' : getRestoreReleaseLabel(release)}
                </button>
              {/if}

              {#if actionAvailability.canDeprecate}
                <form on:submit={handleDeprecate}>
                  <div class="form-group" style="margin-bottom:12px;">
                    <label for="deprecation-message"
                      >Deprecation message (optional)</label
                    >
                    <textarea
                      id="deprecation-message"
                      bind:value={deprecationMessage}
                      class="form-input"
                      rows="3"
                      placeholder="Tell consumers what to use instead"
                    ></textarea>
                  </div>
                  <button
                    type="submit"
                    class="btn btn-secondary"
                    style="width:100%; justify-content:center;"
                    disabled={deprecating}
                  >
                    {deprecating ? 'Deprecating…' : 'Deprecate release'}
                  </button>
                </form>
              {/if}

              {#if actionAvailability.canUndeprecate}
                <button
                  id="release-undeprecate"
                  type="button"
                  class="btn btn-secondary"
                  style="width:100%; justify-content:center; margin-top:12px;"
                  disabled={undeprecating}
                  on:click={handleUndeprecate}
                >
                  {undeprecating ? 'Removing deprecation…' : 'Remove deprecation'}
                </button>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
