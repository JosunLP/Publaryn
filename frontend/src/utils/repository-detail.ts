import type { RepositoryDetail } from '../api/repositories';

import {
  getAllowedPackageVisibilityOptions,
  isRepositoryEligibleForPackageCreation,
} from './package-creation';
import {
  formatRepositoryKindLabel,
  type RepositoryOption,
} from './repositories';

export interface RepositoryDetailCapabilities {
  canManage: boolean;
  canCreatePackages: boolean;
  showPackageCreationSection: boolean;
  packageCreationEligible: boolean;
  packageCreationMessage: string | null;
  packageVisibilityOptions: RepositoryOption[];
  repositoryIsOrgOwned: boolean;
  defaultPackageVisibility: string;
}

export function deriveRepositoryDetailCapabilities(
  repository:
    | Pick<
        RepositoryDetail,
        | 'can_manage'
        | 'can_create_packages'
        | 'kind'
        | 'visibility'
        | 'owner_org_id'
      >
    | null
    | undefined
): RepositoryDetailCapabilities {
  const canManage = repository?.can_manage === true;
  const canCreatePackages = repository?.can_create_packages === true;
  const showPackageCreationSection = canManage || canCreatePackages;
  const packageCreationEligible = isRepositoryEligibleForPackageCreation(
    repository?.kind
  );
  const repositoryIsOrgOwned = Boolean(repository?.owner_org_id?.trim());
  const packageVisibilityOptions =
    canCreatePackages && packageCreationEligible
      ? getAllowedPackageVisibilityOptions(repository?.visibility, {
          repositoryIsOrgOwned,
        })
      : [];
  const defaultPackageVisibility = normalizeRepositoryValue(
    repository?.visibility
  );

  let packageCreationMessage: string | null = null;
  if (showPackageCreationSection && !canCreatePackages) {
    packageCreationMessage =
      'Your current credential can manage this repository but cannot create packages because it does not include the packages:write scope.';
  } else if (canCreatePackages && !packageCreationEligible) {
    packageCreationMessage = `${formatRepositoryKindLabel(repository?.kind)} repositories do not support direct package creation.`;
  } else if (canCreatePackages && packageVisibilityOptions.length === 0) {
    packageCreationMessage =
      'This repository visibility does not allow directly created packages.';
  }

  return {
    canManage,
    canCreatePackages,
    showPackageCreationSection,
    packageCreationEligible,
    packageCreationMessage,
    packageVisibilityOptions,
    repositoryIsOrgOwned,
    defaultPackageVisibility,
  };
}

function normalizeRepositoryValue(value: string | null | undefined): string {
  return value?.trim().toLowerCase().replace(/-/g, '_') || '';
}
