use std::collections::HashSet;

use serde_json::Value;

const SENSITIVE_KEYS: &[&str] = &[
    "access_token",
    "device_id",
    "endpoint_pin",
    "license",
    "private_key",
    "token",
    "warp_secret",
];

#[derive(Debug, Clone, Default)]
pub struct SecretRedactor {
    exact_values: HashSet<String>,
}

impl SecretRedactor {
    pub fn with_secrets(values: impl IntoIterator<Item = String>) -> Self {
        Self {
            exact_values: values
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
        }
    }

    pub fn redact_text(&self, input: &str) -> String {
        let mut output = input.to_owned();
        let mut secrets: Vec<&String> = self.exact_values.iter().collect();
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        for secret in secrets {
            output = output.replace(secret, "[REDACTED]");
        }
        output
    }

    pub fn redact_json(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if is_sensitive_key(key) {
                        *value = Value::String("[REDACTED]".to_owned());
                    } else {
                        self.redact_json(value);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.redact_json(item);
                }
            }
            Value::String(text) => *text = self.redact_text(text),
            _ => {}
        }
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    SENSITIVE_KEYS
        .iter()
        .any(|candidate| normalized == *candidate || normalized.ends_with(candidate))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn redacts_keys_and_exact_values() {
        let redactor = SecretRedactor::with_secrets(["super-secret".to_owned()]);
        let mut value = json!({
            "profile": {
                "access_token": "token",
                "message": "failed with super-secret",
                "endpoint": "162.159.198.2"
            }
        });
        redactor.redact_json(&mut value);
        assert_eq!(value["profile"]["access_token"], "[REDACTED]");
        assert_eq!(value["profile"]["message"], "failed with [REDACTED]");
        assert_eq!(value["profile"]["endpoint"], "162.159.198.2");
    }
}
