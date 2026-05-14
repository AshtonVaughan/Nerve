//! Text redaction.
//!
//! Used to scrub sensitive text from audit logs (passwords pasted via
//! `type_text`, seed phrases pulled out of clipboards, etc). The redaction
//! patterns come from the active safety policy, plus a built-in set of
//! conservative defaults.

use regex_lite::Regex;

#[derive(Debug, Clone)]
pub struct Redactor {
    patterns: Vec<(Regex, &'static str)>,
    user_patterns: Vec<Regex>,
}

impl Redactor {
    pub fn compile(extra: &[String]) -> Self {
        let mut patterns: Vec<(Regex, &'static str)> = Vec::new();
        // Conservative defaults. We intentionally do not try to match generic
        // secrets — false positives in a screenshot caption are worse than
        // false negatives.
        let defaults: &[(&str, &str)] = &[
            // 12 / 24 word BIP-39 seed phrases (very loose heuristic).
            (r"(?i)(?:[a-z]{3,8}\s+){11,23}[a-z]{3,8}", "<SEED_PHRASE_REDACTED>"),
            // Long hex private keys.
            (r"(?i)0x[a-f0-9]{40,}", "<HEX_KEY_REDACTED>"),
            // AWS access keys.
            (r"AKIA[0-9A-Z]{16}", "<AWS_KEY_REDACTED>"),
            // Stripe live secret keys.
            (r"sk_live_[A-Za-z0-9]{16,}", "<STRIPE_KEY_REDACTED>"),
            // OpenAI keys.
            (r"sk-[A-Za-z0-9]{20,}", "<OPENAI_KEY_REDACTED>"),
            // Anthropic keys.
            (r"sk-ant-[A-Za-z0-9_\-]{20,}", "<ANTHROPIC_KEY_REDACTED>"),
            // GitHub tokens.
            (r"gh[pousr]_[A-Za-z0-9]{16,}", "<GITHUB_TOKEN_REDACTED>"),
        ];
        for (pat, repl) in defaults {
            if let Ok(re) = Regex::new(pat) {
                patterns.push((re, repl));
            }
        }
        let user_patterns: Vec<Regex> = extra
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        Self { patterns, user_patterns }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut out = input.to_string();
        for (re, repl) in &self.patterns {
            out = re.replace_all(&out, *repl).to_string();
        }
        for re in &self.user_patterns {
            out = re.replace_all(&out, "<REDACTED>").to_string();
        }
        out
    }

    /// Returns `true` when input contains anything that the redactor flagged.
    pub fn is_sensitive(&self, input: &str) -> bool {
        self.patterns.iter().any(|(re, _)| re.is_match(input))
            || self.user_patterns.iter().any(|re| re.is_match(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openai_keys() {
        let r = Redactor::compile(&[]);
        let s = "my secret is sk-abc123def456ghi789xyz tail";
        let out = r.redact(s);
        assert!(out.contains("<OPENAI_KEY_REDACTED>"));
        assert!(!out.contains("sk-abc123def456ghi789xyz"));
    }

    #[test]
    fn user_pattern_applies() {
        let r = Redactor::compile(&["[Pp]assword=\\S+".to_string()]);
        let out = r.redact("password=hunter2 frob");
        assert!(out.contains("<REDACTED>"));
        assert!(!out.contains("hunter2"));
    }
}
