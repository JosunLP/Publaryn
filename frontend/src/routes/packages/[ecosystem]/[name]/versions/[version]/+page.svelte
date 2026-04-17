<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';

  import { ApiError } from '../../../../../../api/client';
  import type { Artifact, Release } from '../../../../../../api/packages';
  import {
    deprecateRelease,
    getRelease,
    listArtifacts,
    publishRelease,
    unyankRelease,
    uploadReleaseArtifact,
    yankRelease,
  } from '../../../../../../api/packages';
  import {
    ecosystemIcon,
    ecosystemLabel,
    installCommand,
  } from '../../../../../../utils/ecosystem';
  import { copyToClipboard, formatDate } from '../../../../../../utils/format';
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
      notice = result.message || 'Release published successfully.';
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

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) {
      return `${bytes} B`;
    }
    if (bytes < 1024 * 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
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
  $: actionAvailability = release
    ? getReleaseActionAvailability(release, artifactCount)
    : {
        canUploadArtifact: false,
        canPublish: false,
        canYank: false,
        canRestore: false,
        canDeprecate: false,
      };
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
  <div class="mt-6">
    <nav style="font-size:0.875rem; margin-bottom:16px;">
      <a
        href={`/packages/${encodeURIComponent(eecosystem())}/${encodeURIComponent(ename())}`}
        data-sveltekit-preload-data="hover"
        >{ecosystemIcon(eecosystem())} {ename()}</a
      >
      <span>&rsaquo; </span>
      <span style="color:var(--color-text-secondary);">v{eversion()}</span>
    </nav>

    <div class="pkg-header">
      <h1 class="pkg-header__name">{ename()}</h1>
      <span class="badge badge-ecosystem"
        >{ecosystemIcon(eecosystem())} {ecosystemLabel(eecosystem())}</span
      >
      <span class="pkg-header__version">v{eversion()}</span>
      {#if release.is_yanked}<span class="badge badge-yanked">yanked</span>{/if}
      {#if release.is_deprecated}<span class="badge badge-deprecated"
          >deprecated</span
        >{/if}
      {#if release.status}<span class="badge badge-ecosystem"
          >{formatReleaseStatusLabel(release.status)}</span
        >{/if}
    </div>

    {#if notice}<div class="alert alert-success mt-4">{notice}</div>{/if}
    {#if error}<div class="alert alert-error mt-4">{error}</div>{/if}

    <div class="card mt-4 mb-4">
      <h3
        style="font-size:0.8125rem; font-weight:600; color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0.05em; margin-bottom:8px;"
      >
        Install
      </h3>
      <div class="code-block">
        <code>{installCommand(eecosystem(), ename(), eversion())}</code>
        <button class="copy-btn" type="button" on:click={handleCopyInstall}
          >Copy</button
        >
      </div>
    </div>

    <div class="pkg-detail">
      <div class="pkg-detail__main">
        <div class="card mb-4">
          <h3 style="margin-bottom:8px;">Lifecycle state</h3>
          <p
            class={`alert alert-${readiness.tone === 'warning' ? 'warning' : readiness.tone === 'success' ? 'success' : 'info'}`}
          >
            {readiness.message}
          </p>
          <div class="token-row__meta" style="margin-top:12px;">
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
            <h3 style="margin-bottom:8px;">Description</h3>
            <p>{release.description}</p>
          </div>
        {/if}

        {#if release.changelog}
          <div class="card mb-4">
            <h3 style="margin-bottom:8px;">Changelog</h3>
            <pre style="white-space:pre-wrap;">{release.changelog}</pre>
          </div>
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

      <div class="pkg-detail__sidebar">
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
              {:else if !actionAvailability.canPublish && actionAvailability.canUploadArtifact}
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
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
