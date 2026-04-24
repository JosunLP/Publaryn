import type {
  OrgAuditQuery,
  OrgSecurityPackageSummary,
  OrgSecurityQuery,
} from '../api/orgs';
import {
  exportOrgAuditLogsCsv,
  exportOrgSecurityFindingsCsv,
} from '../api/orgs';
import type { SecurityFinding } from '../api/packages';
import {
  listSecurityFindings,
  updateSecurityFinding,
} from '../api/packages';
import type { OrgAuditActorOption } from './org-audit-actors';
import {
  buildOrgAuditExportFilename,
  buildOrgAuditPath,
  type OrgAuditView,
} from './org-audit-query';
import {
  buildOrgSecurityExportFilename,
  buildOrgSecurityPath,
  type OrgSecurityView,
} from './org-security-query';
import {
  buildOrgSecurityPackageKey,
  mergeUpdatedOrgSecurityFinding,
  sortOrgSecurityFindings,
} from './org-security-triage';
import {
  buildAuditExportQuery,
  buildSecurityExportQuery,
  resolveAuditFilterSubmission,
  resolveSecurityFilterSubmission,
} from './org-workspace-actions';

export interface OrgObservabilityReloadOptions {
  notice?: string | null;
  error?: string | null;
}

export interface OrgSecurityFindingState {
  findings: SecurityFinding[];
  load_error: string | null;
  loading: boolean;
  expanded: boolean;
  updatingFindingId: string | null;
  notice: string | null;
  error: string | null;
  findingNotes: Record<string, string>;
}

export interface OrgObservabilityMutations {
  exportOrgAuditLogsCsv: (
    slug: string,
    query?: OrgAuditQuery
  ) => Promise<string>;
  exportOrgSecurityFindingsCsv: (
    slug: string,
    query?: OrgSecurityQuery
  ) => Promise<string>;
  listSecurityFindings: typeof listSecurityFindings;
  updateSecurityFinding: typeof updateSecurityFinding;
}

export interface OrgObservabilityControllerOptions {
  getOrgSlug: () => string;
  getCurrentSearchParams: () => URLSearchParams;
  goto: (path: string) => Promise<void>;
  reload: (options?: OrgObservabilityReloadOptions) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  downloadTextFile: (
    filename: string,
    contents: string,
    contentType: string
  ) => void;
  getAuditActorOptions: () => OrgAuditActorOption[];
  getAuditView: () => OrgAuditView;
  getSecurityView: () => OrgSecurityView;
  setExportingAudit: (value: boolean) => void;
  setExportingSecurity: (value: boolean) => void;
  getSecurityFindingState: (
    securityPackage: Pick<OrgSecurityPackageSummary, 'ecosystem' | 'name'>
  ) => OrgSecurityFindingState;
  updateSecurityFindingState: (
    packageKey: string,
    updates: Partial<OrgSecurityFindingState>
  ) => void;
  reloadSecurityOverview: () => Promise<void>;
  mutations?: OrgObservabilityMutations;
}

const DEFAULT_ORG_OBSERVABILITY_MUTATIONS: OrgObservabilityMutations = {
  exportOrgAuditLogsCsv,
  exportOrgSecurityFindingsCsv,
  listSecurityFindings,
  updateSecurityFinding,
};

export function createOrgObservabilityController(
  options: OrgObservabilityControllerOptions
) {
  const mutations = options.mutations || DEFAULT_ORG_OBSERVABILITY_MUTATIONS;

  return {
    async submitAuditFilter(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const resolution = resolveAuditFilterSubmission(
        new FormData(event.currentTarget as HTMLFormElement),
        options.getAuditActorOptions()
      );

      if (!resolution.ok) {
        await options.reload({
          error: resolution.error,
        });
        return;
      }

      await options.goto(
        buildOrgAuditPath(
          options.getOrgSlug(),
          resolution.value,
          options.getCurrentSearchParams()
        )
      );
    },

    async goToAuditPage(nextPage: number): Promise<void> {
      const auditView = options.getAuditView();
      await options.goto(
        buildOrgAuditPath(
          options.getOrgSlug(),
          {
            action: auditView.action,
            actorUserId: auditView.actorUserId,
            actorUsername: auditView.actorUsername,
            occurredFrom: auditView.occurredFrom,
            occurredUntil: auditView.occurredUntil,
            page: nextPage,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async clearAuditActionFilter(): Promise<void> {
      const auditView = options.getAuditView();
      await options.goto(
        buildOrgAuditPath(
          options.getOrgSlug(),
          {
            action: '',
            actorUserId: auditView.actorUserId,
            actorUsername: auditView.actorUsername,
            occurredFrom: auditView.occurredFrom,
            occurredUntil: auditView.occurredUntil,
            page: 1,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async clearAuditActorFilter(): Promise<void> {
      const auditView = options.getAuditView();
      await options.goto(
        buildOrgAuditPath(
          options.getOrgSlug(),
          {
            action: auditView.action,
            actorUserId: '',
            actorUsername: '',
            occurredFrom: auditView.occurredFrom,
            occurredUntil: auditView.occurredUntil,
            page: 1,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async clearAuditDateFilter(): Promise<void> {
      const auditView = options.getAuditView();
      await options.goto(
        buildOrgAuditPath(
          options.getOrgSlug(),
          {
            action: auditView.action,
            actorUserId: auditView.actorUserId,
            actorUsername: auditView.actorUsername,
            occurredFrom: '',
            occurredUntil: '',
            page: 1,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async focusAuditActor(
      actorUserId: string,
      actorUsername: string
    ): Promise<void> {
      if (!actorUserId) {
        return;
      }

      const auditView = options.getAuditView();
      await options.goto(
        buildOrgAuditPath(
          options.getOrgSlug(),
          {
            action: auditView.action,
            actorUserId,
            actorUsername,
            occurredFrom: auditView.occurredFrom,
            occurredUntil: auditView.occurredUntil,
            page: 1,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async exportAudit(): Promise<void> {
      options.setExportingAudit(true);

      try {
        const auditView = options.getAuditView();
        const csv = await mutations.exportOrgAuditLogsCsv(
          options.getOrgSlug(),
          buildAuditExportQuery(auditView)
        );

        options.downloadTextFile(
          buildOrgAuditExportFilename(
            options.getOrgSlug(),
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
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to export activity log.'
          ),
        });
      } finally {
        options.setExportingAudit(false);
      }
    },

    async submitSecurityFilter(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const nextView = resolveSecurityFilterSubmission(
        new FormData(event.currentTarget as HTMLFormElement)
      );

      await options.goto(
        buildOrgSecurityPath(
          options.getOrgSlug(),
          nextView,
          options.getCurrentSearchParams()
        )
      );
    },

    async clearSecuritySeverityFilter(): Promise<void> {
      const securityView = options.getSecurityView();
      await options.goto(
        buildOrgSecurityPath(
          options.getOrgSlug(),
          {
            severities: [],
            ecosystem: securityView.ecosystem,
            packageQuery: securityView.packageQuery,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async clearSecurityEcosystemFilter(): Promise<void> {
      const securityView = options.getSecurityView();
      await options.goto(
        buildOrgSecurityPath(
          options.getOrgSlug(),
          {
            severities: securityView.severities,
            ecosystem: '',
            packageQuery: securityView.packageQuery,
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async clearSecurityPackageFilter(): Promise<void> {
      const securityView = options.getSecurityView();
      await options.goto(
        buildOrgSecurityPath(
          options.getOrgSlug(),
          {
            severities: securityView.severities,
            ecosystem: securityView.ecosystem,
            packageQuery: '',
          },
          options.getCurrentSearchParams()
        )
      );
    },

    async exportSecurity(): Promise<void> {
      options.setExportingSecurity(true);

      try {
        const securityView = options.getSecurityView();
        const csv = await mutations.exportOrgSecurityFindingsCsv(
          options.getOrgSlug(),
          buildSecurityExportQuery(securityView)
        );

        options.downloadTextFile(
          buildOrgSecurityExportFilename(
            options.getOrgSlug(),
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
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to export security findings.'
          ),
        });
      } finally {
        options.setExportingSecurity(false);
      }
    },

    async toggleSecurityFindings(
      securityPackage: OrgSecurityPackageSummary
    ): Promise<void> {
      const packageKey = buildOrgSecurityPackageKey(
        securityPackage.ecosystem,
        securityPackage.name
      );
      const currentState = options.getSecurityFindingState(securityPackage);

      if (currentState.expanded) {
        options.updateSecurityFindingState(packageKey, { expanded: false });
        return;
      }

      options.updateSecurityFindingState(packageKey, {
        expanded: true,
        loading: true,
        load_error: null,
        error: null,
        notice: null,
      });

      if (!securityPackage.ecosystem || !securityPackage.name) {
        options.updateSecurityFindingState(packageKey, {
          loading: false,
          load_error:
            'Failed to load findings because the package identity is unavailable.',
        });
        return;
      }

      try {
        const findings = await mutations.listSecurityFindings(
          securityPackage.ecosystem,
          securityPackage.name,
          {
            includeResolved: true,
          }
        );
        options.updateSecurityFindingState(packageKey, {
          findings: sortOrgSecurityFindings(findings),
          loading: false,
          load_error: null,
        });
      } catch (caughtError: unknown) {
        options.updateSecurityFindingState(packageKey, {
          findings: [],
          loading: false,
          load_error: options.toErrorMessage(
            caughtError,
            'Failed to load package findings.'
          ),
        });
      }
    },

    async toggleSecurityFindingResolution(
      securityPackage: OrgSecurityPackageSummary,
      finding: SecurityFinding
    ): Promise<void> {
      const packageKey = buildOrgSecurityPackageKey(
        securityPackage.ecosystem,
        securityPackage.name
      );
      const currentState = options.getSecurityFindingState(securityPackage);
      if (currentState.updatingFindingId) {
        return;
      }
      if (!securityPackage.ecosystem || !securityPackage.name) {
        options.updateSecurityFindingState(packageKey, {
          error:
            'Failed to update the security finding because the package identity is unavailable.',
        });
        return;
      }

      const targetIsResolved = !finding.is_resolved;
      const rawNote = currentState.findingNotes[finding.id] ?? '';
      const trimmedNote = rawNote.trim();
      if (trimmedNote.length > 2000) {
        options.updateSecurityFindingState(packageKey, {
          error: 'Security finding note must be 2000 characters or fewer.',
        });
        return;
      }

      options.updateSecurityFindingState(packageKey, {
        updatingFindingId: finding.id,
        error: null,
        notice: null,
      });

      try {
        const updated = await mutations.updateSecurityFinding(
          securityPackage.ecosystem,
          securityPackage.name,
          finding.id,
          {
            isResolved: targetIsResolved,
            note: trimmedNote.length > 0 ? trimmedNote : undefined,
          }
        );
        const latestState = options.getSecurityFindingState(securityPackage);
        options.updateSecurityFindingState(packageKey, {
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
        await options.reloadSecurityOverview();
      } catch (caughtError: unknown) {
        options.updateSecurityFindingState(packageKey, {
          updatingFindingId: null,
          error: options.toErrorMessage(
            caughtError,
            'Failed to update the security finding.'
          ),
        });
      }
    },
  };
}
