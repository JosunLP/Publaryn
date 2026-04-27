import { getAuthToken } from '../api/client';
import type { OrganizationMembership } from '../api/orgs';
import { listMyOrganizations } from '../api/orgs';
import type { RepositoryDetail } from '../api/repositories';
import { transferRepositoryOwnership } from '../api/repositories';
import { selectRepositoryTransferTargets } from '../utils/repositories';

export interface RepositoryTransferState {
  showTransfer: boolean;
  organizations: OrganizationMembership[];
  loadError: string | null;
}

export interface RepositoryTransferDependencies {
  getAuthToken: typeof getAuthToken;
  listMyOrganizations: typeof listMyOrganizations;
  transferRepositoryOwnership: typeof transferRepositoryOwnership;
}

export interface RepositoryTransferReloadOptions {
  notice?: string | null;
  error?: string | null;
}

export interface RepositoryTransferControllerOptions {
  getRepository: () => RepositoryDetail | null;
  getSlug: () => string;
  getTargetOrgSlug: () => string;
  getTransferConfirmed: () => boolean;
  setNotice: (value: string | null) => void;
  setError: (value: string | null) => void;
  setTransferringRepository: (value: boolean) => void;
  loadRepositoryPage: (
    options?: RepositoryTransferReloadOptions
  ) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  dependencies?: RepositoryTransferDependencies;
}

const DEFAULT_REPOSITORY_TRANSFER_DEPENDENCIES: RepositoryTransferDependencies = {
  getAuthToken,
  listMyOrganizations,
  transferRepositoryOwnership,
};

export async function loadRepositoryTransferState(options: {
  isBrowser: boolean;
  repository: RepositoryDetail | null;
  dependencies?: Pick<
    RepositoryTransferDependencies,
    'getAuthToken' | 'listMyOrganizations'
  >;
}): Promise<RepositoryTransferState> {
  const dependencies = {
    getAuthToken: DEFAULT_REPOSITORY_TRANSFER_DEPENDENCIES.getAuthToken,
    listMyOrganizations:
      DEFAULT_REPOSITORY_TRANSFER_DEPENDENCIES.listMyOrganizations,
    ...(options.dependencies || {}),
  };

  if (
    !options.isBrowser ||
    !options.repository ||
    !dependencies.getAuthToken() ||
    options.repository.can_transfer !== true
  ) {
    return {
      showTransfer: false,
      organizations: [],
      loadError: null,
    };
  }

  try {
    const response = await dependencies.listMyOrganizations();
    return {
      showTransfer: true,
      organizations: selectRepositoryTransferTargets(
        response.organizations || [],
        options.repository.owner_org_slug
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
          : 'Failed to load your organizations for repository transfer.',
    };
  }
}

export function createRepositoryTransferController(
  options: RepositoryTransferControllerOptions
) {
  const dependencies =
    options.dependencies || DEFAULT_REPOSITORY_TRANSFER_DEPENDENCIES;

  return {
    async submit(event: SubmitEvent): Promise<void> {
      event.preventDefault();

      const repository = options.getRepository();
      if (!repository || repository.can_transfer !== true) {
        return;
      }

      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const repositorySlug = repository.slug?.trim() || options.getSlug();
      const targetOrgSlug =
        formData.get('target_org_slug')?.toString().trim() ||
        options.getTargetOrgSlug().trim();
      const confirmed =
        formData.get('confirm') !== null || options.getTransferConfirmed();

      if (!targetOrgSlug) {
        options.setNotice(null);
        options.setError('Select a target organization.');
        return;
      }

      if (!confirmed) {
        options.setNotice(null);
        options.setError('Please confirm the repository transfer.');
        return;
      }

      options.setTransferringRepository(true);
      options.setNotice(null);
      options.setError(null);

      try {
        const result = await dependencies.transferRepositoryOwnership(
          repositorySlug,
          {
            targetOrgSlug,
          }
        );

        options.setTransferringRepository(false);
        await options.loadRepositoryPage({
          notice: `Repository ownership transferred to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(
            caughtError,
            'Failed to transfer repository ownership.'
          )
        );
        options.setTransferringRepository(false);
      }
    },
  };
}
