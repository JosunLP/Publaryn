import type { PackageTransferOwnershipResult } from '../api/packages';
import { transferPackageOwnership } from '../api/packages';
import type { NamespaceTransferOwnershipResult } from '../api/namespaces';
import {
  deleteNamespaceClaim,
  transferNamespaceClaim,
} from '../api/namespaces';
import type { Team } from '../api/orgs';
import { deleteTeam } from '../api/orgs';
import type { RepositoryTransferOwnershipResult } from '../api/repositories';
import { transferRepositoryOwnership } from '../api/repositories';
import { decodePackageSelection } from './org-workspace-actions';
import { TEAM_DELETE_CONFIRMATION_MESSAGE } from './team-management';

export const ORG_NAMESPACE_DELETE_CONFIRMATION_MESSAGE =
  'Please confirm that you understand deleting this namespace claim is immediate and cannot be undone.';
export const ORG_NAMESPACE_TRANSFER_CONFIRMATION_MESSAGE =
  'Please confirm the namespace transfer.';
export const ORG_REPOSITORY_TRANSFER_CONFIRMATION_MESSAGE =
  'Please confirm the repository transfer.';
export const ORG_PACKAGE_TRANSFER_CONFIRMATION_MESSAGE =
  'Please confirm the package transfer.';

export interface OrgDestructiveActionsReloadOptions {
  notice?: string | null;
  error?: string | null;
}

export interface OrgDestructiveActionsMutations {
  deleteTeam: typeof deleteTeam;
  deleteNamespaceClaim: typeof deleteNamespaceClaim;
  transferNamespaceClaim: typeof transferNamespaceClaim;
  transferRepositoryOwnership: typeof transferRepositoryOwnership;
  transferPackageOwnership: typeof transferPackageOwnership;
}

export interface OrgDestructiveActionsControllerOptions {
  getOrgSlug: () => string;
  reload: (options?: OrgDestructiveActionsReloadOptions) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  clearFlash: () => void;
  setError: (value: string | null) => void;
  setTeamDeleteTargetSlug: (value: string | null) => void;
  getTeamDeleteConfirmed: () => boolean;
  setTeamDeleteConfirmed: (value: boolean) => void;
  setDeletingTeamSlug: (value: string | null) => void;
  setNamespaceDeleteTargetId: (value: string | null) => void;
  getNamespaceDeleteConfirmed: () => boolean;
  setNamespaceDeleteConfirmed: (value: boolean) => void;
  setDeletingNamespaceClaimId: (value: string | null) => void;
  setNamespaceTransferConfirmationOpen: (value: boolean) => void;
  getNamespaceTransferConfirmed: () => boolean;
  setNamespaceTransferConfirmed: (value: boolean) => void;
  setTransferringNamespaceOwnership: (value: boolean) => void;
  resolveNamespaceLabel: (claimId: string) => string | null;
  setRepositoryTransferConfirmationOpen: (value: boolean) => void;
  getRepositoryTransferConfirmed: () => boolean;
  setRepositoryTransferConfirmed: (value: boolean) => void;
  setTransferringRepositoryOwnership: (value: boolean) => void;
  setPackageTransferConfirmationOpen: (value: boolean) => void;
  getPackageTransferConfirmed: () => boolean;
  setPackageTransferConfirmed: (value: boolean) => void;
  setTransferringPackageOwnershipFlow: (value: boolean) => void;
  mutations?: OrgDestructiveActionsMutations;
}

const DEFAULT_ORG_DESTRUCTIVE_ACTIONS_MUTATIONS: OrgDestructiveActionsMutations =
  {
    deleteTeam,
    deleteNamespaceClaim,
    transferNamespaceClaim,
    transferRepositoryOwnership,
    transferPackageOwnership,
  };

export function createOrgDestructiveActionsController(
  options: OrgDestructiveActionsControllerOptions
) {
  const mutations =
    options.mutations || DEFAULT_ORG_DESTRUCTIVE_ACTIONS_MUTATIONS;

  return {
    openTeamDeleteConfirmation(teamSlug: string): void {
      options.setTeamDeleteTargetSlug(teamSlug);
      options.setTeamDeleteConfirmed(false);
      options.setDeletingTeamSlug(null);
      options.clearFlash();
    },

    cancelTeamDeleteConfirmation(): void {
      options.setTeamDeleteTargetSlug(null);
      options.setTeamDeleteConfirmed(false);
      options.setDeletingTeamSlug(null);
      options.setError(null);
    },

    async submitTeamDelete(
      event: SubmitEvent,
      teamSlug: string
    ): Promise<void> {
      event.preventDefault();

      if (!options.getTeamDeleteConfirmed()) {
        options.clearFlash();
        options.setError(TEAM_DELETE_CONFIRMATION_MESSAGE);
        return;
      }

      options.setDeletingTeamSlug(teamSlug);
      options.clearFlash();

      try {
        await mutations.deleteTeam(options.getOrgSlug(), teamSlug);
        await options.reload({ notice: `Deleted team ${teamSlug}.` });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(caughtError, 'Failed to delete team.')
        );
        options.setDeletingTeamSlug(null);
      }
    },

    openNamespaceDeleteConfirmation(claimId: string): void {
      options.setNamespaceDeleteTargetId(claimId);
      options.setNamespaceDeleteConfirmed(false);
      options.setDeletingNamespaceClaimId(null);
      options.clearFlash();
    },

    cancelNamespaceDeleteConfirmation(): void {
      options.setNamespaceDeleteTargetId(null);
      options.setNamespaceDeleteConfirmed(false);
      options.setDeletingNamespaceClaimId(null);
      options.setError(null);
    },

    async submitNamespaceDelete(
      event: SubmitEvent,
      claimId: string | null | undefined,
      namespace: string
    ): Promise<void> {
      event.preventDefault();

      if (!claimId) {
        await options.reload({
          error:
            'Failed to delete namespace claim because the claim id is unavailable.',
        });
        return;
      }

      if (!options.getNamespaceDeleteConfirmed()) {
        options.clearFlash();
        options.setError(ORG_NAMESPACE_DELETE_CONFIRMATION_MESSAGE);
        return;
      }

      options.setDeletingNamespaceClaimId(claimId);
      options.clearFlash();

      try {
        await mutations.deleteNamespaceClaim(claimId);
        await options.reload({
          notice: `Deleted namespace claim ${namespace}.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(
            caughtError,
            'Failed to delete namespace claim.'
          )
        );
        options.setDeletingNamespaceClaimId(null);
      }
    },

    openNamespaceTransferConfirmation(): void {
      options.setNamespaceTransferConfirmationOpen(true);
      options.setNamespaceTransferConfirmed(false);
      options.setTransferringNamespaceOwnership(false);
      options.clearFlash();
    },

    cancelNamespaceTransferConfirmation(): void {
      options.setNamespaceTransferConfirmationOpen(false);
      options.setNamespaceTransferConfirmed(false);
      options.setTransferringNamespaceOwnership(false);
      options.setError(null);
    },

    async submitNamespaceTransfer(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const claimId = formData.get('claim_id')?.toString().trim() || '';
      const targetOrgSlug =
        formData.get('target_org_slug')?.toString().trim() || '';

      if (!claimId) {
        options.clearFlash();
        options.setError('Select a namespace claim to transfer.');
        return;
      }

      if (!targetOrgSlug) {
        options.clearFlash();
        options.setError('Select a target organization.');
        return;
      }

      if (!options.getNamespaceTransferConfirmed()) {
        options.clearFlash();
        options.setError(ORG_NAMESPACE_TRANSFER_CONFIRMATION_MESSAGE);
        return;
      }

      options.setTransferringNamespaceOwnership(true);
      options.clearFlash();

      try {
        const result: NamespaceTransferOwnershipResult =
          await mutations.transferNamespaceClaim(claimId, {
            targetOrgSlug,
          });
        const namespace =
          result.namespace_claim?.namespace ||
          options.resolveNamespaceLabel(claimId) ||
          'namespace claim';
        await options.reload({
          notice: `Transferred ${namespace} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(
            caughtError,
            'Failed to transfer namespace claim ownership.'
          )
        );
        options.setTransferringNamespaceOwnership(false);
      }
    },

    openRepositoryTransferConfirmation(): void {
      options.setRepositoryTransferConfirmationOpen(true);
      options.setRepositoryTransferConfirmed(false);
      options.setTransferringRepositoryOwnership(false);
      options.clearFlash();
    },

    cancelRepositoryTransferConfirmation(): void {
      options.setRepositoryTransferConfirmationOpen(false);
      options.setRepositoryTransferConfirmed(false);
      options.setTransferringRepositoryOwnership(false);
      options.setError(null);
    },

    async submitRepositoryTransfer(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const repositorySlug =
        formData.get('repository_slug')?.toString().trim() || '';
      const targetOrgSlug =
        formData.get('target_org_slug')?.toString().trim() || '';

      if (!repositorySlug) {
        options.clearFlash();
        options.setError('Select a repository to transfer.');
        return;
      }

      if (!targetOrgSlug) {
        options.clearFlash();
        options.setError('Select a target organization.');
        return;
      }

      if (!options.getRepositoryTransferConfirmed()) {
        options.clearFlash();
        options.setError(ORG_REPOSITORY_TRANSFER_CONFIRMATION_MESSAGE);
        return;
      }

      options.setTransferringRepositoryOwnership(true);
      options.clearFlash();

      try {
        const result: RepositoryTransferOwnershipResult =
          await mutations.transferRepositoryOwnership(repositorySlug, {
            targetOrgSlug,
          });
        await options.reload({
          notice: `Transferred ${result.repository?.name || result.repository?.slug || repositorySlug} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(
            caughtError,
            'Failed to transfer repository ownership.'
          )
        );
        options.setTransferringRepositoryOwnership(false);
      }
    },

    openPackageTransferConfirmation(): void {
      options.setPackageTransferConfirmationOpen(true);
      options.setPackageTransferConfirmed(false);
      options.setTransferringPackageOwnershipFlow(false);
      options.clearFlash();
    },

    cancelPackageTransferConfirmation(): void {
      options.setPackageTransferConfirmationOpen(false);
      options.setPackageTransferConfirmed(false);
      options.setTransferringPackageOwnershipFlow(false);
      options.setError(null);
    },

    async submitPackageTransfer(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);
      const packageTarget = decodePackageSelection(
        formData.get('package_key')?.toString().trim() || ''
      );
      const targetOrgSlug =
        formData.get('target_org_slug')?.toString().trim() || '';

      if (!packageTarget) {
        options.clearFlash();
        options.setError('Select a package to transfer.');
        return;
      }

      if (!targetOrgSlug) {
        options.clearFlash();
        options.setError('Select a target organization.');
        return;
      }

      if (!options.getPackageTransferConfirmed()) {
        options.clearFlash();
        options.setError(ORG_PACKAGE_TRANSFER_CONFIRMATION_MESSAGE);
        return;
      }

      options.setTransferringPackageOwnershipFlow(true);
      options.clearFlash();

      try {
        const result: PackageTransferOwnershipResult =
          await mutations.transferPackageOwnership(
            packageTarget.ecosystem,
            packageTarget.name,
            { targetOrgSlug }
          );
        await options.reload({
          notice: `Transferred ${packageTarget.name} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
        });
      } catch (caughtError: unknown) {
        options.setError(
          options.toErrorMessage(
            caughtError,
            'Failed to transfer package ownership.'
          )
        );
        options.setTransferringPackageOwnershipFlow(false);
      }
    },
  };
}
