//! Operator-authored profile settings, not session/model input.
use crate::Id;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexSettings {
    /// Selects an operator-installed runner execution template, never a path,
    /// image digest, executable, credential reference, or arbitrary environment.
    pub template: Id,
    pub model: String,
    pub reasoning_effort: Option<String>,
}
impl CodexSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.model.is_empty()
            || !self.model.as_bytes()[0].is_ascii_alphanumeric()
            || self.model.len() > 128
            || !self
                .model
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
            || self.reasoning_effort.as_deref().is_some_and(|s| {
                !["minimal", "low", "medium", "high", "xhigh", "max", "ultra"].contains(&s)
            })
        {
            return Err("invalid Codex settings");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_data_not_configuration_injection() {
        let mut s = CodexSettings {
            template: Id::new("codex-standard").unwrap(),
            model: "gpt-5.6-luna".into(),
            reasoning_effort: Some("low".into()),
        };
        assert!(s.validate().is_ok());
        for model in ["", "x\nmodel_provider='other'", "../../auth", "--config"] {
            s.model = model.into();
            assert!(s.validate().is_err());
        }
        s.model = "gpt-5.6-luna".into();
        s.reasoning_effort = Some("other".into());
        assert!(s.validate().is_err());
    }
}
