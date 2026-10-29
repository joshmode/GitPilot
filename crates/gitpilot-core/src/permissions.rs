use crate::domain::PermissionPolicy;
use crate::error::{GitPilotError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ReadGitHub,
    IndexRepository,
    ExternalAi,
    LocalExecution,
    ModifyLocalFiles,
    ModifyGitHub,
    PublishReviews,
    ApproveOrMerge,
    ScheduledAutomations,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadGitHub => "Read GitHub data",
            Self::IndexRepository => "Index repository content",
            Self::ExternalAi => "Send context to external AI",
            Self::LocalExecution => "Execute local tools",
            Self::ModifyLocalFiles => "Modify local files",
            Self::ModifyGitHub => "Modify GitHub resources",
            Self::PublishReviews => "Publish review comments",
            Self::ApproveOrMerge => "Approve or merge pull requests",
            Self::ScheduledAutomations => "Run scheduled automations",
        }
    }
}

pub const ALL_PERMISSIONS: [Permission; 9] = [
    Permission::ReadGitHub,
    Permission::IndexRepository,
    Permission::ExternalAi,
    Permission::LocalExecution,
    Permission::ModifyLocalFiles,
    Permission::ModifyGitHub,
    Permission::PublishReviews,
    Permission::ApproveOrMerge,
    Permission::ScheduledAutomations,
];

pub fn require(policy: &PermissionPolicy, permission: Permission) -> Result<()> {
    let allowed = match permission {
        Permission::ReadGitHub => policy.read_github,
        Permission::IndexRepository => policy.index_repository,
        // `local_only` constrains provider selection in the routing layer; it
        // does not substitute for the explicit external-transfer grant.
        Permission::ExternalAi => policy.external_ai,
        Permission::LocalExecution => policy.local_execution,
        Permission::ModifyLocalFiles => policy.modify_local_files,
        Permission::ModifyGitHub => policy.modify_github,
        Permission::PublishReviews => policy.publish_reviews,
        Permission::ApproveOrMerge => policy.approve_or_merge,
        Permission::ScheduledAutomations => policy.scheduled_automations,
    };
    if allowed {
        Ok(())
    } else {
        Err(GitPilotError::PermissionDenied(format!(
            "{} is disabled in the active permission policy",
            permission.label()
        )))
    }
}

/// Apply a scoped policy only after the global policy allows it. A scoped grant
/// can never exceed the user's global grant.
pub fn require_scoped(
    application: &PermissionPolicy,
    scoped: &PermissionPolicy,
    permission: Permission,
) -> Result<()> {
    require(application, permission)?;
    require(scoped, permission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_denied_by_default() {
        let policy = PermissionPolicy::default();
        assert!(require(&policy, Permission::ModifyGitHub).is_err());
        assert!(require(&policy, Permission::PublishReviews).is_err());
        assert!(require(&policy, Permission::ReadGitHub).is_ok());
    }

    #[test]
    fn local_only_does_not_implicitly_grant_external_ai() {
        let policy = PermissionPolicy {
            external_ai: true,
            local_only: true,
            ..Default::default()
        };
        assert!(require(&policy, Permission::ExternalAi).is_ok());
    }

    #[test]
    fn scoped_policy_cannot_exceed_application_policy() {
        let application = PermissionPolicy::default();
        let scoped = PermissionPolicy {
            modify_github: true,
            ..Default::default()
        };
        assert!(require_scoped(&application, &scoped, Permission::ModifyGitHub).is_err());
    }
}
