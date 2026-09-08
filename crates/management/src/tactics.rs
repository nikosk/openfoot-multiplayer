//! Persistent club instructions used to seed each match. Match AI may adapt its
//! own copy without changing this saved plan.

use engine::{
    BreakSpeed, CounterPressDuration, DefensiveLine, DefensiveShape, MarkingStyle, PlayStyle,
    PressingIntensity, TacticsBuildUpStyle, TacticsConfig, TacticsPitchWidth, Tempo,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchPlan {
    pub play_style: PlayStyle,
    pub pressing_intensity: PressingIntensity,
    pub defensive_line: DefensiveLine,
    pub width: TacticsPitchWidth,
    pub build_up_style: TacticsBuildUpStyle,
    pub marking_style: MarkingStyle,
    pub tempo: Tempo,
    pub defensive_shape: DefensiveShape,
    pub counter_press_duration: CounterPressDuration,
    pub break_speed: BreakSpeed,
}

impl Default for MatchPlan {
    fn default() -> Self {
        Self {
            play_style: PlayStyle::Balanced,
            pressing_intensity: PressingIntensity::default(),
            defensive_line: DefensiveLine::default(),
            width: TacticsPitchWidth::default(),
            build_up_style: TacticsBuildUpStyle::default(),
            marking_style: MarkingStyle::default(),
            tempo: Tempo::default(),
            defensive_shape: DefensiveShape::default(),
            counter_press_duration: CounterPressDuration::default(),
            break_speed: BreakSpeed::default(),
        }
    }
}

impl MatchPlan {
    /// Convert phase dials to the engine representation. Play style is supplied
    /// separately through `engine::TeamData::play_style`.
    pub fn engine_tactics(&self) -> TacticsConfig {
        TacticsConfig {
            pressing_intensity: self.pressing_intensity,
            defensive_line: self.defensive_line,
            width: self.width,
            build_up_style: self.build_up_style,
            marking_style: self.marking_style,
            tempo: self.tempo,
            defensive_shape: self.defensive_shape,
            counter_press_duration: self.counter_press_duration,
            break_speed: self.break_speed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attacking_plan() -> MatchPlan {
        MatchPlan {
            play_style: PlayStyle::HighPress,
            pressing_intensity: PressingIntensity::Aggressive,
            defensive_line: DefensiveLine::High,
            width: TacticsPitchWidth::Wide,
            build_up_style: TacticsBuildUpStyle::Short,
            marking_style: MarkingStyle::ManToMan,
            tempo: Tempo::Patient,
            defensive_shape: DefensiveShape::Compact,
            counter_press_duration: CounterPressDuration::Long,
            break_speed: BreakSpeed::Fast,
        }
    }

    #[test]
    fn defaults_match_the_engine() {
        let plan = MatchPlan::default();
        assert_eq!(plan.play_style, PlayStyle::Balanced);
        assert_eq!(
            serde_json::to_value(plan.engine_tactics()).unwrap(),
            serde_json::to_value(TacticsConfig::default()).unwrap(),
        );
    }

    #[test]
    fn converts_every_nondefault_dial_exactly() {
        let plan = attacking_plan();
        let tactics = plan.engine_tactics();
        assert_eq!(tactics.pressing_intensity, PressingIntensity::Aggressive);
        assert_eq!(tactics.defensive_line, DefensiveLine::High);
        assert_eq!(tactics.width, TacticsPitchWidth::Wide);
        assert_eq!(tactics.build_up_style, TacticsBuildUpStyle::Short);
        assert_eq!(tactics.marking_style, MarkingStyle::ManToMan);
        assert_eq!(tactics.tempo, Tempo::Patient);
        assert_eq!(tactics.defensive_shape, DefensiveShape::Compact);
        assert_eq!(tactics.counter_press_duration, CounterPressDuration::Long);
        assert_eq!(tactics.break_speed, BreakSpeed::Fast);
        assert_eq!(plan.play_style, PlayStyle::HighPress);
    }

    #[test]
    fn plans_round_trip_with_all_play_styles() {
        for play_style in [
            PlayStyle::Balanced,
            PlayStyle::Attacking,
            PlayStyle::Defensive,
            PlayStyle::Possession,
            PlayStyle::Counter,
            PlayStyle::HighPress,
        ] {
            let plan = MatchPlan {
                play_style,
                ..attacking_plan()
            };
            let json = serde_json::to_string(&plan).unwrap();
            assert_eq!(serde_json::from_str::<MatchPlan>(&json).unwrap(), plan);
        }
        let default = MatchPlan::default();
        assert_eq!(
            serde_json::from_value::<MatchPlan>(serde_json::to_value(&default).unwrap()).unwrap(),
            default,
        );
    }

    #[test]
    fn invalid_enum_values_are_rejected_for_every_field() {
        let json = serde_json::to_value(attacking_plan()).unwrap();
        for field in json.as_object().unwrap().keys() {
            let mut invalid = json.clone();
            invalid[field] = serde_json::json!("NotATacticalOption");
            assert!(
                serde_json::from_value::<MatchPlan>(invalid).is_err(),
                "invalid value accepted for {field}",
            );
        }
    }
}
