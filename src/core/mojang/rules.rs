use crate::core::mojang::version::{OsRule, Rule};

pub fn current_os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

pub fn current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        "x86" => "x86",
        other => other,
    }
}

/// Mojang's name for an architecture ("arm64", "x86_64", "x86") from any
/// spelling of it ("aarch64", "amd64" …).
pub fn mojang_arch(arch: &str) -> &str {
    match crate::core::platform::normalize_arch(arch) {
        "aarch64" => "arm64",
        other => other,
    }
}

/// What a launch looks like to the rules: the architecture the game runs as
/// (an Intel game on Apple Silicon runs under Rosetta) and the optional
/// features it turns on.
#[derive(Debug, Clone, Copy)]
pub struct RuleEnv<'a> {
    pub arch: &'a str,
    pub custom_resolution: bool,
}

impl RuleEnv<'static> {
    pub fn host() -> Self {
        RuleEnv { arch: std::env::consts::ARCH, custom_resolution: false }
    }
}

fn os_matches(os: &OsRule, env: RuleEnv) -> bool {
    if let Some(name) = &os.name {
        if name != current_os_name() {
            return false;
        }
    }
    if let Some(arch) = &os.arch {
        if arch != mojang_arch(env.arch) {
            return false;
        }
    }
    // os.version rules are rare; ignoring them is safer than rejecting a
    // library that would otherwise work on the current macOS.
    true
}

fn rule_matches(rule: &Rule, env: RuleEnv) -> bool {
    if let Some(os) = &rule.os {
        if !os_matches(os, env) {
            return false;
        }
    }
    if let Some(features) = &rule.features {
        // Demo mode and the quick play arguments stay off (quick play is
        // added by hand when asked for); a custom window size is optional.
        let on = |name: &str| name == "has_custom_resolution" && env.custom_resolution;
        if features.iter().any(|(name, wanted)| on(name) != *wanted) {
            return false;
        }
    }
    true
}

/// Minecraft rule semantics: no rules means allowed; otherwise the last
/// matching rule decides, and the default is disallowed.
pub fn rules_allow(rules: &[Rule]) -> bool {
    rules_allow_in(rules, RuleEnv::host())
}

pub fn rules_allow_in(rules: &[Rule], env: RuleEnv) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, env) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mojang::version::{OsRule, Rule};
    use std::collections::HashMap;

    #[test]
    fn custom_resolution_rules_follow_the_window_setting() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some(HashMap::from([("has_custom_resolution".to_string(), true)])),
        }];
        assert!(!rules_allow(&rules));
        assert!(rules_allow_in(&rules, RuleEnv { arch: std::env::consts::ARCH, custom_resolution: true }));
        let demo = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some(HashMap::from([("is_demo_user".to_string(), true)])),
        }];
        assert!(!rules_allow_in(&demo, RuleEnv { arch: std::env::consts::ARCH, custom_resolution: true }));
    }

    #[test]
    fn empty_rules_allow() {
        assert!(rules_allow(&[]));
    }

    #[test]
    fn os_rule_filters_other_platforms() {
        let other = if current_os_name() == "osx" {
            "windows"
        } else {
            "osx"
        };
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some(other.into()),
                arch: None,
                version: None,
            }),
            features: None,
        }];
        assert!(!rules_allow(&rules));
    }

    #[test]
    fn last_matching_rule_wins() {
        let rules = vec![
            Rule {
                action: "allow".into(),
                os: None,
                features: None,
            },
            Rule {
                action: "disallow".into(),
                os: Some(OsRule {
                    name: Some(current_os_name().into()),
                    arch: None,
                    version: None,
                }),
                features: None,
            },
        ];
        assert!(!rules_allow(&rules));
    }

    #[test]
    fn false_features_do_not_filter() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some(HashMap::from([("is_demo_user".to_string(), false)])),
        }];
        assert!(rules_allow(&rules));
    }
}
