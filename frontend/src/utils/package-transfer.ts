type NullableString = string | null | undefined;

interface PackageTransferOrganizationLike {
  slug?: NullableString;
  name?: NullableString;
  role?: NullableString;
}

interface TransferablePackageLike {
  ecosystem?: NullableString;
  name?: NullableString;
  can_transfer?: boolean | null;
}

const PACKAGE_TRANSFER_ADMIN_ROLES = new Set(['owner', 'admin']);

export function selectPackageTransferTargets<
  T extends PackageTransferOrganizationLike,
>(organizations: T[], currentOwnerOrgSlug?: NullableString): T[] {
  const normalizedOwnerSlug = normalizeSlug(currentOwnerOrgSlug);

  return [...organizations]
    .filter((organization) => {
      const slug = normalizeSlug(organization.slug);
      const role = normalizeRole(organization.role);

      return (
        Boolean(slug) &&
        PACKAGE_TRANSFER_ADMIN_ROLES.has(role) &&
        slug !== normalizedOwnerSlug
      );
    })
    .sort((left, right) => {
      const leftLabel = (left.name || left.slug || '').toLowerCase();
      const rightLabel = (right.name || right.slug || '').toLowerCase();
      return leftLabel.localeCompare(rightLabel);
    });
}

export function selectTransferablePackages<T extends TransferablePackageLike>(
  packages: T[]
): T[] {
  return [...packages]
    .filter(
      (pkg) =>
        pkg.can_transfer === true &&
        Boolean(normalizeText(pkg.ecosystem)) &&
        Boolean(normalizeText(pkg.name))
    )
    .sort((left, right) => {
      const leftKey = `${normalizeText(left.ecosystem)}:${normalizeText(left.name)}`;
      const rightKey = `${normalizeText(right.ecosystem)}:${normalizeText(right.name)}`;
      return leftKey.localeCompare(rightKey);
    });
}

function normalizeSlug(value: NullableString): string {
  return normalizeText(value).toLowerCase();
}

function normalizeRole(value: NullableString): string {
  return normalizeText(value).toLowerCase();
}

function normalizeText(value: NullableString): string {
  return typeof value === 'string' ? value.trim() : '';
}
