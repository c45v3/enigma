use crate::engine::manager::entity::{EntityCommand, EntityOperation};
use crate::engine::skill::{
    behavior::{
        AttackModifierContext, BehaviorOpContext, classify::BehaviorKind, registry::BehaviorHandler,
    },
    effect::ParsedBehavior,
    rule::output::{BattleCommand, RuleOp},
};

pub struct Handler;

impl BehaviorHandler for Handler {
    const VALIDATES_ARGUMENTS: bool = true;

    fn supports(behavior: &ParsedBehavior) -> bool {
        matches!(
            (behavior.spec.kind, behavior.args.as_slice()),
            (BehaviorKind::CareerRatioFix, [bonus]) if *bonus != 0
        ) || matches!(
            (behavior.spec.kind, behavior.args.as_slice()),
            (BehaviorKind::ChangeCareer, [career]) if normalized_career(*career).is_some()
        ) || matches!(
            (behavior.spec.kind, behavior.args.as_slice()),
            (BehaviorKind::ChangeAttackCareer, [career]) if (1..=8).contains(career)
        ) || matches!(behavior.spec.kind, BehaviorKind::SetCareerRestraint)
            && behavior.args.is_empty()
    }

    fn emit_ops(context: BehaviorOpContext<'_>, behavior: &ParsedBehavior) -> Option<Vec<RuleOp>> {
        if let (BehaviorKind::ChangeCareer, [career]) =
            (behavior.spec.kind, behavior.args.as_slice())
            && let Some(career) = normalized_career(*career)
        {
            return Some(vec![RuleOp::Command(BattleCommand::Entity(EntityCommand {
                origin: super::command_origin(behavior).expect("registered behavior"),
                source_uid: context.source_uid,
                target_uid: context.target_uid,
                operation: EntityOperation::ChangeCareer { career },
            }))]);
        }
        apply(context.modifiers, behavior).then(Vec::new)
    }

    fn collect_attack_modifier(
        context: AttackModifierContext<'_>,
        behavior: &ParsedBehavior,
    ) -> bool {
        apply(context.operation.modifiers, behavior)
    }
}

// Enemy skill data also contains legacy 1000-prefixed career values for
// ChangeCareer (e.g. 50013#1002). SkillChangeAttackCareer (100036) only uses
// plain values (1, 2) in skill_effect.json, so it needs no normalization.
fn normalized_career(career: i32) -> Option<i32> {
    match career {
        1..=6 => Some(career),
        1001..=1006 => Some(career - 1000),
        _ => None,
    }
}

fn apply(
    modifiers: &mut crate::engine::skill::action::SkillModifiers,
    behavior: &ParsedBehavior,
) -> bool {
    if !Handler::supports(behavior) {
        return false;
    }
    match behavior.spec.kind {
        BehaviorKind::CareerRatioFix => {
            modifiers.career_ratio_bonus += behavior.args[0];
        }
        BehaviorKind::ChangeAttackCareer => {
            modifiers.attack_career = Some(behavior.args[0]);
        }
        BehaviorKind::SetCareerRestraint => modifiers.force_career_restraint = true,
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn career_ratio_fix_keeps_signed_permille_values() {
        assert!(Handler::supports(&ParsedBehavior::new(
            60058,
            "CareerRatioFix",
            vec![300],
        )));
        assert!(Handler::supports(&ParsedBehavior::new(
            60058,
            "CareerRatioFix",
            vec![-600],
        )));
    }

    #[test]
    fn attack_career_accepts_only_real_afflatuses() {
        assert!(Handler::supports(&ParsedBehavior::new(
            100036,
            "SkillChangeAttackCareer",
            vec![1],
        )));
        assert!(!Handler::supports(&ParsedBehavior::new(
            100036,
            "SkillChangeAttackCareer",
            vec![101],
        )));
    }

    #[test]
    fn career_change_emits_entity_owned_state() {
        let managers = crate::engine::manager::BattleManagers::default();
        let pool = crate::engine::skill::target::TargetPool::default();
        let mut determinism = crate::engine::runtime::determinism::RoundDeterminism::default();
        let mut modifiers = crate::engine::skill::action::SkillModifiers::default();
        let mut target = crate::engine::skill::target::TargetContext::default();

        let ops = Handler::emit_ops(
            BehaviorOpContext {
                source_uid: -1,
                source_team: 2,
                target_uid: -1,
                active_skill_id: 20,
                transfer_count: 1,
                event: None,
                managers: &managers,
                pool: &pool,
                determinism: &mut determinism,
                modifiers: &mut modifiers,
                target: &mut target,
            },
            &ParsedBehavior::new(50013, "ChangeCareer", vec![3]),
        )
        .unwrap();

        assert!(matches!(
            ops.as_slice(),
            [RuleOp::Command(BattleCommand::Entity(EntityCommand {
                target_uid: -1,
                operation: EntityOperation::ChangeCareer { career: 3 },
                ..
            }))]
        ));
    }
    #[test]
    fn legacy_career_codes_are_normalized() {
        assert!(Handler::supports(&ParsedBehavior::new(
            50013,
            "ChangeCareer",
            vec![1002],
        )));
        assert_eq!(normalized_career(1002), Some(2));
    }

    #[test]
    fn attack_career_is_written_to_the_skill_modifier() {
        let managers = crate::engine::manager::BattleManagers::default();
        let pool = crate::engine::skill::target::TargetPool::default();
        let mut determinism = crate::engine::runtime::determinism::RoundDeterminism::default();
        let mut modifiers = crate::engine::skill::action::SkillModifiers::default();
        let mut target = crate::engine::skill::target::TargetContext::default();

        assert!(Handler::collect_attack_modifier(
            AttackModifierContext {
                operation: BehaviorOpContext {
                    source_uid: 10,
                    source_team: 1,
                    target_uid: -1,
                    active_skill_id: 20,
                    transfer_count: 1,
                    event: None,
                    managers: &managers,
                    pool: &pool,
                    determinism: &mut determinism,
                    modifiers: &mut modifiers,
                    target: &mut target,
                },
                owner_skill_id: 0,
                conditions: &[],
            },
            &ParsedBehavior::new(100036, "SkillChangeAttackCareer", vec![1]),
        ));
        assert_eq!(modifiers.attack_career, Some(1));
    }

    #[test]
    fn active_skill_attack_career_uses_the_same_modifier_path() {
        let managers = crate::engine::manager::BattleManagers::default();
        let pool = crate::engine::skill::target::TargetPool::default();
        let mut determinism = crate::engine::runtime::determinism::RoundDeterminism::default();
        let mut modifiers = crate::engine::skill::action::SkillModifiers::default();
        let mut target = crate::engine::skill::target::TargetContext::default();

        assert_eq!(
            Handler::emit_ops(
                BehaviorOpContext {
                    source_uid: 10,
                    source_team: 1,
                    target_uid: -1,
                    active_skill_id: 20,
                    transfer_count: 1,
                    event: None,
                    managers: &managers,
                    pool: &pool,
                    determinism: &mut determinism,
                    modifiers: &mut modifiers,
                    target: &mut target,
                },
                &ParsedBehavior::new(100036, "SkillChangeAttackCareer", vec![1]),
            ),
            Some(Vec::new())
        );
        assert_eq!(modifiers.attack_career, Some(1));
    }
}
