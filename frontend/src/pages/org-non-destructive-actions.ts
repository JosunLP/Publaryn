import { createNamespaceClaim } from '../api/namespaces';
import { createTeam } from '../api/orgs';
import { createPackage } from '../api/packages';
import { createRepository, updateRepository } from '../api/repositories';

export interface OrgNonDestructiveActionsReloadOptions {
  notice?: string | null;
  error?: string | null;
}

export interface PackageCreationRepository {
  slug: string;
  name?: string | null;
  visibility?: string | null;
}

export interface OrgNonDestructiveActionsMutations {
  createTeam: typeof createTeam;
  createNamespaceClaim: typeof createNamespaceClaim;
  createRepository: typeof createRepository;
  updateRepository: typeof updateRepository;
  createPackage: typeof createPackage;
}

export interface OrgNonDestructiveActionsControllerOptions {
  getOrgSlug: () => string;
  getOrgId: () => string | null;
  reload: (
    options?: OrgNonDestructiveActionsReloadOptions
  ) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  ecosystemLabel: (ecosystem: string) => string;
  clearFlash: () => void;
  setError: (value: string | null) => void;
  setCreatingPackage: (value: boolean) => void;
  getCreatableRepositoriesCount: () => number;
  resolvePackageCreationRepository: (
    repositorySlug: string
  ) => PackageCreationRepository | null;
  resetPackageDraft: () => void;
  mutations?: OrgNonDestructiveActionsMutations;
}

const DEFAULT_ORG_NON_DESTRUCTIVE_ACTIONS_MUTATIONS: OrgNonDestructiveActionsMutations =
  {
    createTeam,
    createNamespaceClaim,
    createRepository,
    updateRepository,
    createPackage,
  };

export function createOrgNonDestructiveActionsController(
  options: OrgNonDestructiveActionsControllerOptions
) {
  const mutations =
    options.mutations || DEFAULT_ORG_NON_DESTRUCTIVE_ACTIONS_MUTATIONS;

  return {
    async submitTeamCreate(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const form = event.currentTarget as HTMLFormElement;
      const formData = new FormData(form);

      try {
        await mutations.createTeam(options.getOrgSlug(), {
          name: formData.get('name')?.toString().trim() || '',
          slug: formData.get('team_slug')?.toString().trim() || '',
          description:
            normalizeOptionalFormText(formData.get('description')) || undefined,
        });

        form.reset();
        await options.reload({ notice: 'Team created successfully.' });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to create team.'),
        });
      }
    },

    async submitNamespaceCreate(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const orgId = options.getOrgId();

      if (!orgId?.trim()) {
        await options.reload({
          error:
            'Failed to create the namespace claim because the organization id is unavailable.',
        });
        return;
      }

      const form = event.currentTarget as HTMLFormElement;
      const formData = new FormData(form);
      const ecosystem =
        formData.get('ecosystem')?.toString().trim().toLowerCase() || '';
      const namespace = formData.get('namespace')?.toString().trim() || '';

      if (!ecosystem || !namespace) {
        await options.reload({
          error: 'Select an ecosystem and namespace first.',
        });
        return;
      }

      try {
        await mutations.createNamespaceClaim({
          ecosystem,
          namespace,
          ownerOrgId: orgId,
        });
        form.reset();
        await options.reload({
          notice: `Created the ${options.ecosystemLabel(ecosystem)} namespace claim ${namespace}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to create namespace claim.'
          ),
        });
      }
    },

    async submitRepositoryCreate(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const orgId = options.getOrgId();

      if (!orgId?.trim()) {
        await options.reload({
          error:
            'Failed to create the repository because the organization id is unavailable.',
        });
        return;
      }

      const form = event.currentTarget as HTMLFormElement;
      const formData = new FormData(form);

      try {
        await mutations.createRepository({
          name: formData.get('name')?.toString().trim() || '',
          slug: formData.get('slug')?.toString().trim() || '',
          kind: formData.get('kind')?.toString().trim() || 'public',
          visibility: formData.get('visibility')?.toString().trim() || 'public',
          description: normalizeOptionalFormText(formData.get('description')),
          ownerOrgId: orgId,
        });

        form.reset();
        await options.reload({
          notice: 'Repository created successfully.',
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to create repository.'
          ),
        });
      }
    },

    async submitPackageCreate(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const repositorySlug =
        formData.get('repository_slug')?.toString().trim() || '';
      const selectedRepository =
        options.resolvePackageCreationRepository(repositorySlug);

      if (!selectedRepository) {
        options.clearFlash();
        options.setError(
          options.getCreatableRepositoriesCount() === 0
            ? 'Create an eligible repository before creating a package.'
            : 'Select a repository for the new package.'
        );
        return;
      }

      const packageName = formData.get('name')?.toString().trim() || '';
      if (!packageName) {
        options.clearFlash();
        options.setError('Enter a package name.');
        return;
      }

      const ecosystem =
        formData.get('ecosystem')?.toString().trim().toLowerCase() || '';
      const repositoryName =
        selectedRepository.name || selectedRepository.slug;

      options.setCreatingPackage(true);
      options.clearFlash();

      try {
        const result = await mutations.createPackage({
          ecosystem,
          name: packageName,
          repositorySlug,
          visibility:
            normalizeOptionalFormText(formData.get('visibility')) ?? undefined,
          displayName: formData.get('display_name')?.toString() || '',
          description: formData.get('description')?.toString() || '',
        });

        options.resetPackageDraft();
        await options.reload({
          notice: `Created ${options.ecosystemLabel(result.ecosystem || ecosystem)} package ${result.name || packageName} in ${repositoryName}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to create package.'),
        });
      } finally {
        options.setCreatingPackage(false);
      }
    },

    async submitRepositoryUpdate(
      event: SubmitEvent,
      repositorySlug: string
    ): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);

      try {
        await mutations.updateRepository(repositorySlug, {
          description: formData.get('description')?.toString().trim() || '',
          visibility: formData.get('visibility')?.toString().trim() || 'public',
        });

        await options.reload({
          notice: `Updated repository ${repositorySlug}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to update repository.'
          ),
        });
      }
    },
  };
}

function normalizeOptionalFormText(
  value: FormDataEntryValue | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}
